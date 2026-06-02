//! Roland / BOSS Katana SysEx helpers.
//!
//! A byte-for-byte port of the Roland message construction in
//! [`remedy/lib/midi.py`](../../../remedy/lib/midi.py). Every builder
//! produces the **complete on-wire message including the `0xF0`/`0xF7`
//! delimiters** — the CircuitPython code relied on `adafruit_midi` to add
//! them, whereas here we emit them explicitly so the output is directly
//! comparable to bytes captured off the wire.
//!
//! Message layout (Roland "System Exclusive, model-specific"):
//!
//! ```text
//!   F0 41 <dev> <model[4]> <op> <addr[4]> <data[..]> <checksum> F7
//!         └ROLAND_ID                                  └over addr+data
//! ```
//!
//! `op` is `0x12` (DT1, *Data Set 1* — write) or `0x11` (RQ1, *Data
//! Request 1* — read). The checksum covers the address and data bytes
//! only, per the Roland convention.

use heapless::Vec;

use super::sysex::{SysEx, SysExError, SYSEX_END, SYSEX_START};

/// Roland's SysEx manufacturer ID.
pub const ROLAND_ID: u8 = 0x41;

/// Default device ID (unit 0 / broadcast). Matches the CP default.
pub const DEVICE_ID: u8 = 0x00;

/// BOSS Katana model ID (`GT`-series amp address space).
pub const KATANA_MODEL_ID: [u8; 4] = [0x00, 0x00, 0x00, 0x33];

/// DT1 — *Data Set 1*: write/set a parameter.
pub const OP_DT1: u8 = 0x12;
/// RQ1 — *Data Request 1*: read/query a parameter.
pub const OP_RQ1: u8 = 0x11;

// ── Parameter addresses ────────────────────────────────────────────────
// The address space the firmware touches, named once here so the outbound
// builders and the inbound [`parse_dt1`] reverse-map share one definition
// (no magic-number drift between send and receive). Ported from
// `remedy/lib/midi.py`.

/// Recall a preset — data `[0x00, preset]` (`0` = Panel, `1..=4` = CH1..CH4).
pub const ADDR_RECALL_PRESET: [u8; 4] = [0x00, 0x01, 0x00, 0x00];
/// Amp type — data `[amp_type]` (`0..=4`).
pub const ADDR_AMP_TYPE: [u8; 4] = [0x00, 0x00, 0x04, 0x20];
/// Gain — data `[value]` (`0..=100`).
pub const ADDR_GAIN: [u8; 4] = [0x00, 0x00, 0x04, 0x21];
/// Master volume — data `[value]` (`0..=100`).
pub const ADDR_VOLUME: [u8; 4] = [0x00, 0x00, 0x04, 0x22];

// Effect-switch block on/off addresses — data `[0/1]`. Named here (rather than
// inline in the `set_*` builders) so the builders and the [`EFFECT_BLOCKS`]
// reverse-map share one definition. Ported from the `bool` parameters in
// `remedy/config/profiles/katana.toml`.
/// BOOST block on/off.
pub const ADDR_BOOST_SW: [u8; 4] = [0x60, 0x00, 0x00, 0x30];
/// MOD block on/off.
pub const ADDR_MOD_SW: [u8; 4] = [0x60, 0x00, 0x01, 0x40];
/// DELAY block on/off.
pub const ADDR_DELAY_SW: [u8; 4] = [0x60, 0x00, 0x05, 0x60];
/// REVERB block on/off.
pub const ADDR_REVERB_SW: [u8; 4] = [0x60, 0x00, 0x06, 0x10];
/// FX-LOOP block on/off.
pub const ADDR_FX_LOOP_SW: [u8; 4] = [0x00, 0x00, 0x04, 0x00];

/// Boot device-state query sweep: `(address, request-length)` pairs the
/// firmware reads back from a Katana so the board can reflect the amp's current
/// state. Scoped to the categories the app mirrors onto radio groups — **amp
/// type** (1 byte) and **preset** (2 bytes, matching the CP profile's
/// `query_length`). Mirrors `remedy/main.py::_query_device_state` (which swept
/// each configured bool effect; the baked Katana page here uses amp-type /
/// preset radios, so those are what we read). The effect-switch blocks are swept
/// separately (via [`EFFECT_BLOCKS`]). The DT1 replies are decoded by
/// [`parse_dt1`] and reflected by `app::Router::on_sysex_rx`.
pub const DEVICE_QUERY_SWEEP: [([u8; 4], u8); 2] = [(ADDR_AMP_TYPE, 1), (ADDR_RECALL_PRESET, 2)];

/// BOSS Katana effect-switch blocks that mirror onto a CC toggle, as
/// `(block address, cc_alias)`. The `cc_alias` is the GA-FC CC number the amp
/// uses for that on/off block — ported from the `cc_alias` fields on the `bool`
/// parameters in `remedy/config/profiles/katana.toml`. One source of truth, two
/// uses: the boot sweep RQ1-reads each address (so the toggles reflect the amp's
/// state on connect), and an inbound block DT1 is reverse-mapped to its CC by
/// [`effect_block_cc`] so `app::Router` can reflect the amp's real on/off onto
/// the matching CC-toggle button (the CP `_sync_cc_to_toggle` path).
pub const EFFECT_BLOCKS: [([u8; 4], u8); 5] = [
    (ADDR_BOOST_SW, 16),
    (ADDR_MOD_SW, 17),
    (ADDR_DELAY_SW, 19),
    (ADDR_REVERB_SW, 20),
    (ADDR_FX_LOOP_SW, 21),
];

/// Reverse-map an inbound DT1 block address to its `cc_alias`, or [`None`] if the
/// address is not a mirrored effect switch. The inverse of the [`EFFECT_BLOCKS`]
/// table — used by `app::Router::on_sysex_rx` to bridge a Katana block DT1 to
/// the CC toggle that drives it.
pub fn effect_block_cc(address: &[u8; 4]) -> Option<u8> {
    EFFECT_BLOCKS
        .iter()
        .find(|&&(addr, _)| addr == *address)
        .map(|&(_, cc)| cc)
}

/// `true` if `cc` is the `cc_alias` of a mirrored effect-switch block (the
/// forward of [`effect_block_cc`] over just the CC). `app::Router` uses it to
/// decide whether a *local* toggle press is device-backed and so should be
/// cached as amp state: the Katana accepts the GA-FC CC but does **not** echo it
/// back over DIN, so without an optimistic cache the pressed state would be lost
/// on the next page change.
pub fn is_effect_cc(cc: u8) -> bool {
    EFFECT_BLOCKS.iter().any(|&(_, alias)| alias == cc)
}

/// Roland 7-bit checksum over `data`:
/// `accum = sum(data) & 0x7F; (128 - accum) & 0x7F`.
///
/// Direct port of `roland_checksum` in `remedy/lib/midi.py`.
pub fn roland_checksum(data: &[u8]) -> u8 {
    let sum: u32 = data.iter().map(|&b| b as u32).sum();
    let accum = (sum & 0x7F) as u8;
    (128 - accum) & 0x7F
}

/// Encode a value (0..=2000) using Roland's 11-bit split into two 7-bit
/// bytes `[high, low]`. Used for delay time and similar large values.
///
/// Port of `encode_roland_11bit`.
pub fn encode_11bit(value: u16) -> [u8; 2] {
    let v = value.min(2000);
    [((v >> 7) & 0x0F) as u8, (v & 0x7F) as u8]
}

/// Decode Roland's 11-bit `[high, low]` pair back to an integer.
///
/// Port of `decode_roland_11bit`.
pub fn decode_11bit(high: u8, low: u8) -> u16 {
    (((high & 0x0F) as u16) << 7) | ((low & 0x7F) as u16)
}

/// Build a complete Roland SysEx message into `out`:
/// `F0 41 <dev> <model..> <op> <addr..> <data..> <cksum> F7`.
///
/// The checksum is taken over `address ++ data` only. Mirrors
/// `build_roland_sysex` (plus the `0xF0`/`0xF7` that `adafruit_midi` adds
/// downstream of it). Returns [`SysExError::Overflow`] if `out` is too
/// small for the whole message.
pub fn build_into<const N: usize>(
    out: &mut Vec<u8, N>,
    model_id: &[u8],
    operation: u8,
    address: &[u8; 4],
    data: &[u8],
    device_id: u8,
) -> Result<(), SysExError> {
    out.clear();
    out.push(SYSEX_START).map_err(|_| SysExError::Overflow)?;
    out.push(ROLAND_ID).map_err(|_| SysExError::Overflow)?;
    out.push(device_id).map_err(|_| SysExError::Overflow)?;
    out.extend_from_slice(model_id).map_err(|_| SysExError::Overflow)?;
    out.push(operation).map_err(|_| SysExError::Overflow)?;

    // The checksum region (address + data) is contiguous in the buffer
    // once both are pushed; compute it there so there is one definition of
    // the algorithm (`roland_checksum`).
    let payload_start = out.len();
    out.extend_from_slice(address).map_err(|_| SysExError::Overflow)?;
    out.extend_from_slice(data).map_err(|_| SysExError::Overflow)?;
    let checksum = roland_checksum(&out[payload_start..]);

    out.push(checksum).map_err(|_| SysExError::Overflow)?;
    out.push(SYSEX_END).map_err(|_| SysExError::Overflow)?;
    Ok(())
}

/// Build a DT1 (write) message for the given model into a fresh [`SysEx`]
/// buffer.
pub fn dt1(model_id: &[u8], address: &[u8; 4], data: &[u8]) -> Result<SysEx, SysExError> {
    let mut msg = SysEx::new();
    build_into(&mut msg, model_id, OP_DT1, address, data, DEVICE_ID)?;
    Ok(msg)
}

/// Build an RQ1 (read) message requesting `length` bytes at `address`.
///
/// The length is encoded as the 4-byte size field `[0, 0, 0, length]`,
/// matching `query_sysex_param` in the CP firmware (which supports a
/// single-byte length in the low position — sufficient for the per-
/// parameter reads the firmware issues).
pub fn rq1(model_id: &[u8], address: &[u8; 4], length: u8) -> Result<SysEx, SysExError> {
    let size = [0, 0, 0, length];
    let mut msg = SysEx::new();
    build_into(&mut msg, model_id, OP_RQ1, address, &size, DEVICE_ID)?;
    Ok(msg)
}

// ───────────────────────────────────────────────────────────────────────
// Katana-specific convenience builders (all DT1 on `KATANA_MODEL_ID`).
// Addresses ported verbatim from `remedy/lib/midi.py`.
// ───────────────────────────────────────────────────────────────────────

/// Enter the Katana BTS editor mode (`0x7F 00 00 01` ← `0x01`).
pub fn enter_editor_mode() -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &[0x7F, 0x00, 0x00, 0x01], &[0x01])
}

/// Exit the Katana BTS editor mode (`0x7F 00 00 01` ← `0x00`).
pub fn exit_editor_mode() -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &[0x7F, 0x00, 0x00, 0x01], &[0x00])
}

/// Recall a preset: `0` = Panel, `1..=4` = CH1..CH4.
pub fn recall_preset(preset: u8) -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &ADDR_RECALL_PRESET, &[0x00, preset])
}

/// Set amp type: `0` = Acoustic, `1` = Clean, `2` = Crunch, `3` = Lead,
/// `4` = Brown.
pub fn set_amp_type(amp_type: u8) -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &ADDR_AMP_TYPE, &[amp_type])
}

/// Set gain (0..=100).
pub fn set_gain(value: u8) -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &ADDR_GAIN, &[value])
}

/// Set master volume (0..=100).
pub fn set_volume(value: u8) -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &ADDR_VOLUME, &[value])
}

/// Set the pedal/wah position (0..=100), addr `60 00 01 5D`. Used by a
/// continuous expression-pedal binding ([`crate::config::ContinuousSysex::Wah`]).
/// The caller scales its `0..=127` control value to `0..=100` first.
pub fn set_wah_position(value: u8) -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &[0x60, 0x00, 0x01, 0x5D], &[value])
}

/// Toggle the BOOST block on/off.
pub fn set_boost(on: bool) -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &ADDR_BOOST_SW, &[on as u8])
}

/// Toggle the MOD block on/off.
pub fn set_mod(on: bool) -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &ADDR_MOD_SW, &[on as u8])
}

/// Toggle the DELAY block on/off.
pub fn set_delay(on: bool) -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &ADDR_DELAY_SW, &[on as u8])
}

/// Toggle the REVERB block on/off.
pub fn set_reverb(on: bool) -> Result<SysEx, SysExError> {
    dt1(&KATANA_MODEL_ID, &ADDR_REVERB_SW, &[on as u8])
}

/// Set delay time in milliseconds (1..=2000), Roland 11-bit encoded.
pub fn set_delay_time(ms: u16) -> Result<SysEx, SysExError> {
    let encoded = encode_11bit(ms);
    dt1(&KATANA_MODEL_ID, &[0x60, 0x00, 0x05, 0x62], &encoded)
}

// ───────────────────────────────────────────────────────────────────────
// Inbound parsing — the receive half of device sync.
// ───────────────────────────────────────────────────────────────────────

/// A parsed inbound Roland **DT1** (*Data Set 1*) message: the 4-byte
/// parameter address and its data payload (borrowed from the source buffer).
/// Produced by [`parse_dt1`].
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct Dt1<'a> {
    /// The 4-byte Roland parameter address (e.g. [`ADDR_AMP_TYPE`]).
    pub address: [u8; 4],
    /// The parameter data bytes (may be empty).
    pub data: &'a [u8],
}

/// Parse a complete inbound Roland DT1 message of the form
/// `F0 41 <dev> <model[4]> 12 <addr[4]> <data..> <cksum> F7`.
///
/// Returns the address + data iff the frame is a well-formed Roland **DT1**
/// for `model_id` with a valid checksum (verified over `address ++ data`,
/// the same region [`build_into`] signs). This is the exact inverse of the
/// builders above, so anything they emit round-trips.
///
/// The device ID byte (`msg[2]`) is **not** matched: the amp answers with its
/// own unit id, so any value is accepted. Anything that fails a structural or
/// checksum check returns [`None`] (a foreign manufacturer's SysEx, an RQ1
/// echo, a truncated frame, a corrupt payload) rather than mis-parsing.
pub fn parse_dt1<'a>(msg: &'a [u8], model_id: &[u8; 4]) -> Option<Dt1<'a>> {
    // Smallest valid frame (zero data bytes):
    //   F0 41 dev m0 m1 m2 m3 12 a0 a1 a2 a3 cksum F7 = 14 bytes.
    if msg.len() < 14 {
        return None;
    }
    if msg[0] != SYSEX_START || msg[msg.len() - 1] != SYSEX_END {
        return None;
    }
    if msg[1] != ROLAND_ID {
        return None;
    }
    if msg[3..7] != *model_id {
        return None;
    }
    if msg[7] != OP_DT1 {
        return None;
    }
    // `body` = address(4) ++ data(n); the checksum sits just before `F7`.
    let body = &msg[8..msg.len() - 2];
    let checksum = msg[msg.len() - 2];
    if roland_checksum(body) != checksum {
        return None;
    }
    let mut address = [0u8; 4];
    address.copy_from_slice(&body[..4]);
    Some(Dt1 { address, data: &body[4..] })
}
