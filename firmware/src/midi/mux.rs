//! MIDI mux: merge USB-MIDI and the DIN UART into one normalised stream.
//!
//! This module is the single owner of both MIDI transports (per the
//! "one owner per peripheral" rule in `ARCHITECTURE.md`). It:
//!
//! - **decodes** inbound USB-MIDI event packets and the raw DIN byte
//!   stream into [`MidiRx`] (channel-voice) on [`MidiRxChannel`], and
//!   reassembled SysEx on a [`SysExChannel`];
//! - **encodes** outbound [`MidiCmd`] from [`MidiCmdChannel`] and SysEx
//!   from a second [`SysExChannel`], sending each to **both** transports.
//!
//! The pure codec functions ([`decode_voice`], [`encode_usb`],
//! [`encode_din`], [`DinParser`]) carry no transport types and are unit-
//! testable; the `*_loop` async functions wire them to the embassy
//! transports.
//!
//! ## Channel numbering
//!
//! [`MidiRx`]/[`MidiCmd`] carry the raw 4-bit wire channel (`0..=15`),
//! taken straight from the status byte's low nibble and masked back in on
//! encode. Callers that think in 1-based MIDI channels convert at their
//! edge — the mux stays a pure wire codec.

use core::cell::Cell;

use embassy_futures::select::{select3, Either3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex as BlockingMutex;
use embassy_sync::channel::Channel;
use embassy_usb::class::midi::{Receiver, Sender};
use embassy_usb::driver::{Driver, EndpointError};
use embedded_io_async::{Read, Write};
use heapless::Vec;

use super::sysex::{self, SysEx, SysExBuf, MAX_SYSEX};
use crate::config;
use crate::events::{MidiCmd, MidiRx};

/// Depth of the inbound channel-voice channel. Drop-newest on overflow.
pub const RX_DEPTH: usize = 16;
/// Depth of the outbound command channel. Producers block on send.
pub const CMD_DEPTH: usize = 16;
/// Depth of a SysEx channel (in or out). SysEx is bursty but low-rate.
pub const SYSEX_DEPTH: usize = 4;

/// Channel carrying normalised inbound channel-voice MIDI to the router.
pub type MidiRxChannel = Channel<CriticalSectionRawMutex, MidiRx, RX_DEPTH>;
/// Channel carrying outbound channel-voice commands to the mux.
pub type MidiCmdChannel = Channel<CriticalSectionRawMutex, MidiCmd, CMD_DEPTH>;
/// Channel carrying owned, reassembled/outbound SysEx messages.
pub type SysExChannel = Channel<CriticalSectionRawMutex, SysEx, SYSEX_DEPTH>;

// Endpoint aliases the app wires the router to. (`Sender`/`Receiver` are
// fully-pathed here because this module already imports the USB-MIDI
// `Sender`/`Receiver` under those names.)
/// Receiver the router reads normalised inbound MIDI from.
pub type MidiRxReceiver = embassy_sync::channel::Receiver<'static, CriticalSectionRawMutex, MidiRx, RX_DEPTH>;
/// Sender the router pushes outbound channel-voice commands to.
pub type MidiCmdSender = embassy_sync::channel::Sender<'static, CriticalSectionRawMutex, MidiCmd, CMD_DEPTH>;
/// Sender the router pushes outbound SysEx to (drained by [`out_loop`]).
pub type SysExSender = embassy_sync::channel::Sender<'static, CriticalSectionRawMutex, SysEx, SYSEX_DEPTH>;

// ── MIDI-thru routing ────────────────────────────────────────────────────
// The thru matrix forwards inbound channel-voice messages to a *specific*
// outbound transport, gated by [`config::ThruRoutes`]. The input loops read the
// live routes from a shared `static` (published by the router on config load /
// apply, mirroring `hal::expression::LIVE_CAL`) and, when a route is enabled,
// hand the message to [`out_loop`] tagged with its destination — the only path
// that writes one transport instead of fanning to both.
//
// Scope: channel-voice messages (Note / CC / PC / Pitch Bend — everything
// [`MidiRx`] represents). SysEx pass-through is not forwarded yet.

/// Which transport a thru-forwarded message goes out on.
#[derive(Clone, Copy)]
pub enum ThruDest {
    Usb,
    Din,
}

/// A thru-forwarded inbound message plus its destination transport.
#[derive(Clone, Copy)]
pub struct ThruMsg {
    pub dest: ThruDest,
    pub msg: MidiRx,
}

/// Depth of the thru channel (input loops → [`out_loop`]). Drop-newest on overflow.
pub const THRU_DEPTH: usize = 16;
/// Channel carrying thru-forwarded messages to [`out_loop`].
pub type ThruChannel = Channel<CriticalSectionRawMutex, ThruMsg, THRU_DEPTH>;
/// Sender half — held by the input loops.
pub type ThruSender = embassy_sync::channel::Sender<'static, CriticalSectionRawMutex, ThruMsg, THRU_DEPTH>;
/// Receiver half — held by [`out_loop`].
pub type ThruReceiver = embassy_sync::channel::Receiver<'static, CriticalSectionRawMutex, ThruMsg, THRU_DEPTH>;

/// Live MIDI-thru routes, shared between the router (writer, via [`set_thru`])
/// and the input loops (readers, via [`thru_routes`]).
static THRU: BlockingMutex<CriticalSectionRawMutex, Cell<config::ThruRoutes>> =
    BlockingMutex::new(Cell::new(config::ThruRoutes::NONE));

/// Publish the active thru routes — called by the router on config load/apply.
pub fn set_thru(routes: config::ThruRoutes) {
    THRU.lock(|c| c.set(routes));
}

/// Read the active thru routes.
fn thru_routes() -> config::ThruRoutes {
    THRU.lock(|c| c.get())
}

/// Forward an inbound message per the live routes. `from_usb` picks which route
/// pair applies (USB-in vs DIN-in). A message can fan to both transports (e.g.
/// DIN-in with both `din_to_usb` and `din_to_din` set).
fn forward_thru(m: MidiRx, from_usb: bool, thru: &ThruSender) {
    let r = thru_routes();
    let (to_usb, to_din) = if from_usb {
        (r.usb_to_usb, r.usb_to_din)
    } else {
        (r.din_to_usb, r.din_to_din)
    };
    if to_usb {
        let _ = thru.try_send(ThruMsg { dest: ThruDest::Usb, msg: m });
    }
    if to_din {
        let _ = thru.try_send(ThruMsg { dest: ThruDest::Din, msg: m });
    }
}

/// Worst-case USB packet count for one [`MAX_SYSEX`]-byte SysEx:
/// `ceil(MAX_SYSEX / 3)` (three SysEx bytes per packet).
const USB_PACKETS_MAX: usize = MAX_SYSEX / 3 + 1;

// ───────────────────────────────────────────────────────────────────────
// Pure codec
// ───────────────────────────────────────────────────────────────────────

/// Decode a channel-voice message from a status byte plus its (up to two)
/// data bytes. Returns `None` for message types absent from [`MidiRx`]
/// (poly/channel aftertouch, pitch-bend) or for a non-status `status`.
pub fn decode_voice(status: u8, d0: u8, d1: u8) -> Option<MidiRx> {
    let channel = status & 0x0F;
    match status & 0xF0 {
        0x80 => Some(MidiRx::Note {
            channel,
            note: d0,
            velocity: d1,
            on: false,
        }),
        // NoteOn with velocity 0 is the conventional NoteOff.
        0x90 => Some(MidiRx::Note {
            channel,
            note: d0,
            velocity: d1,
            on: d1 != 0,
        }),
        0xB0 => Some(MidiRx::ControlChange {
            channel,
            cc: d0,
            value: d1,
        }),
        0xC0 => Some(MidiRx::ProgramChange {
            channel,
            program: d0,
        }),
        // Pitch bend: LSB first (`d0`), then MSB (`d1`), each 7-bit → 14-bit.
        0xE0 => Some(MidiRx::PitchBend {
            channel,
            value: ((d1 as u16) << 7) | d0 as u16,
        }),
        _ => None,
    }
}

/// Decode a channel-voice USB-MIDI event packet into a [`MidiRx`].
/// Returns `None` for SysEx/system/realtime CINs (route SysEx through a
/// [`SysExBuf`]) and for unrepresented message types.
pub fn decode_usb_channel(packet: &[u8; 4]) -> Option<MidiRx> {
    match packet[0] & 0x0F {
        0x8..=0xE => decode_voice(packet[1], packet[2], packet[3]),
        _ => None,
    }
}

/// Encode a [`MidiCmd`] as a single USB-MIDI event packet on `cable`.
pub fn encode_usb(cmd: &MidiCmd, cable: u8) -> [u8; 4] {
    let cab = (cable & 0x0F) << 4;
    match *cmd {
        MidiCmd::ControlChange { channel, cc, value } => {
            [cab | 0x0B, 0xB0 | (channel & 0x0F), cc & 0x7F, value & 0x7F]
        }
        MidiCmd::ProgramChange { channel, program } => {
            [cab | 0x0C, 0xC0 | (channel & 0x0F), program & 0x7F, 0]
        }
        MidiCmd::Note {
            channel,
            note,
            velocity,
            on,
        } => {
            let (status_hi, cin) = if on { (0x90, 0x09) } else { (0x80, 0x08) };
            [
                cab | cin,
                status_hi | (channel & 0x0F),
                note & 0x7F,
                velocity & 0x7F,
            ]
        }
    }
}

/// Encode a [`MidiCmd`] as raw DIN MIDI bytes (no running-status
/// compression — each message is emitted in full).
pub fn encode_din(cmd: &MidiCmd) -> Vec<u8, 3> {
    let mut v = Vec::new();
    match *cmd {
        MidiCmd::ControlChange { channel, cc, value } => {
            let _ = v.push(0xB0 | (channel & 0x0F));
            let _ = v.push(cc & 0x7F);
            let _ = v.push(value & 0x7F);
        }
        MidiCmd::ProgramChange { channel, program } => {
            let _ = v.push(0xC0 | (channel & 0x0F));
            let _ = v.push(program & 0x7F);
        }
        MidiCmd::Note {
            channel,
            note,
            velocity,
            on,
        } => {
            let status = (if on { 0x90u8 } else { 0x80u8 }) | (channel & 0x0F);
            let _ = v.push(status);
            let _ = v.push(note & 0x7F);
            let _ = v.push(velocity & 0x7F);
        }
    }
    v
}

/// Encode a [`MidiRx`] as a single USB-MIDI event packet on `cable` — the
/// thru-forwarding counterpart to [`encode_usb`], which additionally covers
/// pitch bend (present in [`MidiRx`] but not [`MidiCmd`]).
fn encode_usb_rx(m: &MidiRx, cable: u8) -> [u8; 4] {
    let cab = (cable & 0x0F) << 4;
    match *m {
        MidiRx::ControlChange { channel, cc, value } => {
            [cab | 0x0B, 0xB0 | (channel & 0x0F), cc & 0x7F, value & 0x7F]
        }
        MidiRx::ProgramChange { channel, program } => {
            [cab | 0x0C, 0xC0 | (channel & 0x0F), program & 0x7F, 0]
        }
        MidiRx::Note { channel, note, velocity, on } => {
            let (status_hi, cin) = if on { (0x90, 0x09) } else { (0x80, 0x08) };
            [cab | cin, status_hi | (channel & 0x0F), note & 0x7F, velocity & 0x7F]
        }
        MidiRx::PitchBend { channel, value } => [
            cab | 0x0E,
            0xE0 | (channel & 0x0F),
            (value & 0x7F) as u8,
            ((value >> 7) & 0x7F) as u8,
        ],
    }
}

/// Encode a [`MidiRx`] as raw DIN MIDI bytes — the thru-forwarding counterpart
/// to [`encode_din`].
fn encode_din_rx(m: &MidiRx) -> Vec<u8, 3> {
    let mut v = Vec::new();
    match *m {
        MidiRx::ControlChange { channel, cc, value } => {
            let _ = v.push(0xB0 | (channel & 0x0F));
            let _ = v.push(cc & 0x7F);
            let _ = v.push(value & 0x7F);
        }
        MidiRx::ProgramChange { channel, program } => {
            let _ = v.push(0xC0 | (channel & 0x0F));
            let _ = v.push(program & 0x7F);
        }
        MidiRx::Note { channel, note, velocity, on } => {
            let status = (if on { 0x90u8 } else { 0x80u8 }) | (channel & 0x0F);
            let _ = v.push(status);
            let _ = v.push(note & 0x7F);
            let _ = v.push(velocity & 0x7F);
        }
        MidiRx::PitchBend { channel, value } => {
            let _ = v.push(0xE0 | (channel & 0x0F));
            let _ = v.push((value & 0x7F) as u8);
            let _ = v.push(((value >> 7) & 0x7F) as u8);
        }
    }
    v
}

// ───────────────────────────────────────────────────────────────────────
// DIN byte-stream parser
// ───────────────────────────────────────────────────────────────────────

/// Output of [`DinParser::feed`].
pub enum DinOut<'a> {
    /// A decoded channel-voice message.
    Rx(MidiRx),
    /// A complete SysEx (`0xF0..0xF7`), borrowing the parser's buffer.
    SysEx(&'a [u8]),
}

/// Incremental parser for the raw DIN MIDI byte stream.
///
/// Handles MIDI running status, channel-voice messages, system-common
/// (consumed but not surfaced), system-realtime (transparent — may be
/// interleaved mid-message), and SysEx (reassembled via an embedded
/// [`SysExBuf`]). Feed one byte at a time.
pub struct DinParser {
    sysex: SysExBuf,
    /// Running status byte (`0` = none).
    status: u8,
    data: [u8; 2],
    data_idx: usize,
    /// Data bytes the current status expects (1 or 2).
    expected: usize,
}

impl Default for DinParser {
    fn default() -> Self {
        Self::new()
    }
}

impl DinParser {
    /// Create an idle parser. `const` so it can back a `static`.
    pub const fn new() -> Self {
        Self {
            sysex: SysExBuf::new(),
            status: 0,
            data: [0; 2],
            data_idx: 0,
            expected: 0,
        }
    }

    /// Feed one byte. Returns `Some(DinOut)` when a full channel-voice
    /// message or SysEx has been parsed.
    pub fn feed(&mut self, b: u8) -> Option<DinOut<'_>> {
        // System-realtime: single byte, interleavable anywhere; no state
        // change and not surfaced (no MidiRx variant for it).
        if (0xF8..=0xFF).contains(&b) {
            return None;
        }

        if b == sysex::SYSEX_START {
            self.status = 0; // system messages cancel running status
            let _ = self.sysex.push_byte(sysex::SYSEX_START);
            return None;
        }
        if b == sysex::SYSEX_END {
            return self.sysex.push_byte(sysex::SYSEX_END).map(DinOut::SysEx);
        }

        if self.sysex.is_active() {
            if b < 0x80 {
                let _ = self.sysex.push_byte(b);
                return None;
            }
            // A non-F7 status byte mid-SysEx aborts it, then is handled as
            // a fresh message by the logic below.
            let _ = self.sysex.push_byte(b);
        }

        if b >= 0x80 {
            if b < 0xF0 {
                // Channel-voice status.
                self.status = b;
                self.expected = voice_data_len(b);
                self.data_idx = 0;
            } else {
                // System-common (0xF1..=0xF6): cancels running status and
                // is not surfaced. Any trailing data bytes fall through as
                // orphans (status == 0) and are dropped.
                self.status = 0;
                self.data_idx = 0;
            }
            return None;
        }

        // Data byte under running status.
        if self.status == 0 {
            return None;
        }
        self.data[self.data_idx] = b;
        self.data_idx += 1;
        if self.data_idx >= self.expected {
            self.data_idx = 0; // running status stays armed for repeats
            return decode_voice(self.status, self.data[0], self.data[1]).map(DinOut::Rx);
        }
        None
    }
}

/// Channel-voice data-byte count for a status byte (`0x80..=0xEF`).
fn voice_data_len(status: u8) -> usize {
    match status & 0xF0 {
        0xC0 | 0xD0 => 1, // Program Change, Channel Pressure
        _ => 2,           // Note On/Off, Poly AT, Control Change, Pitch Bend
    }
}

// ───────────────────────────────────────────────────────────────────────
// Transport loops
// ───────────────────────────────────────────────────────────────────────

/// Copy a reassembled SysEx into an owned buffer and try to enqueue it.
/// Drop-newest on a full channel (better than back-pressuring RX).
fn emit_sysex(ch: &'static SysExChannel, msg: &[u8]) {
    let mut owned = SysEx::new();
    if owned.extend_from_slice(msg).is_ok() {
        let _ = ch.try_send(owned);
    }
}

/// USB-MIDI input loop: channel-voice → `rx_ch`, reassembled SysEx →
/// `sysex_in`, and (per the live thru routes) forwarded to `thru`. Owns the
/// USB-MIDI receiver; never returns.
pub async fn usb_in_loop<'d, D: Driver<'d>>(
    mut usb_rx: Receiver<'d, D>,
    rx_ch: &'static MidiRxChannel,
    sysex_in: &'static SysExChannel,
    thru: ThruSender,
) -> ! {
    let mut buf = [0u8; 64];
    let mut reasm = SysExBuf::new();
    loop {
        usb_rx.wait_connection().await;
        defmt::info!("midi mux: USB-MIDI connected");
        loop {
            match usb_rx.read_packet(&mut buf).await {
                Ok(n) => {
                    for chunk in buf[..n].chunks_exact(4) {
                        let packet = [chunk[0], chunk[1], chunk[2], chunk[3]];
                        match packet[0] & 0x0F {
                            0x4..=0x7 => {
                                if let Some(msg) = reasm.push_packet(&packet) {
                                    emit_sysex(sysex_in, msg);
                                }
                            }
                            0x8..=0xE => {
                                if let Some(rx) = decode_usb_channel(&packet) {
                                    let _ = rx_ch.try_send(rx);
                                    forward_thru(rx, true, &thru);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Err(EndpointError::Disabled) => {
                    defmt::info!("midi mux: USB-MIDI disconnected");
                    break;
                }
                Err(_) => {}
            }
        }
    }
}

/// DIN input loop: parse the raw UART byte stream into `rx_ch`, `sysex_in`, and
/// (per the live thru routes) `thru`. Owns the UART receiver; never returns.
pub async fn din_in_loop<R: Read>(
    mut rx: R,
    rx_ch: &'static MidiRxChannel,
    sysex_in: &'static SysExChannel,
    thru: ThruSender,
) -> ! {
    let mut parser = DinParser::new();
    let mut buf = [0u8; 32];
    loop {
        // A UART read error (framing/overrun) just means "try again";
        // there is nothing actionable for a MIDI stream but to resync.
        if let Ok(n) = rx.read(&mut buf).await {
            for &b in &buf[..n] {
                match parser.feed(b) {
                    Some(DinOut::Rx(m)) => {
                        let _ = rx_ch.try_send(m);
                        forward_thru(m, false, &thru);
                    }
                    Some(DinOut::SysEx(msg)) => emit_sysex(sysex_in, msg),
                    None => {}
                }
            }
        }
    }
}

/// Output loop. Drains three sources: `cmd_ch` (channel-voice) and `sysex_out`
/// — both the router's own output, fanned to **both** transports — and `thru`,
/// MIDI-thru forwards that go to a **single** transport (USB *or* DIN) per
/// [`ThruMsg::dest`]. Owns the USB-MIDI sender and the UART transmitter; never
/// returns.
pub async fn out_loop<'d, D: Driver<'d>, W: Write>(
    mut usb_tx: Sender<'d, D>,
    mut din_tx: W,
    cmd_ch: &'static MidiCmdChannel,
    sysex_out: &'static SysExChannel,
    thru: ThruReceiver,
) -> ! {
    loop {
        match select3(cmd_ch.receive(), sysex_out.receive(), thru.receive()).await {
            Either3::First(cmd) => {
                let pkt = encode_usb(&cmd, 0);
                let _ = usb_tx.write_packet(&pkt).await;
                let din = encode_din(&cmd);
                let _ = din_tx.write_all(din.as_slice()).await;
            }
            Either3::Second(sx) => {
                let mut packets: Vec<[u8; 4], USB_PACKETS_MAX> = Vec::new();
                if sysex::to_usb_packets(&sx, 0, &mut packets).is_ok() {
                    for p in &packets {
                        let _ = usb_tx.write_packet(p).await;
                    }
                }
                let _ = din_tx.write_all(sx.as_slice()).await;
            }
            // A thru forward targets exactly one transport (the directional
            // route), unlike the router's output which fans to both.
            Either3::Third(t) => match t.dest {
                ThruDest::Usb => {
                    let _ = usb_tx.write_packet(&encode_usb_rx(&t.msg, 0)).await;
                }
                ThruDest::Din => {
                    let _ = din_tx.write_all(encode_din_rx(&t.msg).as_slice()).await;
                }
            },
        }
    }
}
