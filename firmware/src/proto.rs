//! Config-sync wire protocol — the device side of the webapp ↔ device link
//! over USB-CDC serial (Web Serial).
//!
//! This module is the **codec only**: framing + integrity, no I/O and no
//! config semantics. The CDC task layers GET/SET-config handling on top (it
//! decodes a frame, acts on `cmd`, and encodes a reply). Keeping the codec
//! pure makes it directly testable (`examples/proto_selftest.rs`).
//!
//! ## Framing
//!
//! Each frame on the wire is:
//!
//! ```text
//!   COBS(body) || 0x00
//!   body = cmd(1) | seq(1) | payload(N) | crc16(2, big-endian)
//!   crc16 covers cmd|seq|payload
//! ```
//!
//! **COBS** ([Consistent Overhead Byte Stuffing]) removes every `0x00` from the
//! encoded body, so a single `0x00` delimiter unambiguously ends a frame and
//! the receiver can resync after garbage or a USB suspend/wake glitch by
//! scanning to the next `0x00`. **CRC-16/CCITT-FALSE** catches the corruption
//! COBS can't (CDC is reliable, but not perfectly). Overhead is ~1 byte per
//! 254 payload bytes plus the 2-byte CRC and the 1-byte delimiter.
//!
//! [Consistent Overhead Byte Stuffing]: https://en.wikipedia.org/wiki/Consistent_Overhead_Byte_Stuffing

/// Protocol version, sent in `HELLO`. Bump on any breaking wire change.
///
/// v2: `config::RuntimeConfig` gained the `midi_thru` field (appended after
/// `pages`), so a v1 config blob no longer round-trips — a breaking change to
/// the GET/SET payload format.
pub const PROTO_VERSION: u8 = 2;

/// Largest config payload we carry (the postcard `RuntimeConfig` blob ceiling;
/// see `config::MAX_PAGES`). Buffers are sized from this.
pub const MAX_PAYLOAD: usize = 2048;
/// Largest `body` (= payload + cmd + seq + crc).
pub const MAX_BODY: usize = MAX_PAYLOAD + 4;
/// Largest encoded frame incl. COBS overhead and the `0x00` delimiter. RX/TX
/// buffers in the CDC task are sized to this.
pub const MAX_FRAME_LEN: usize = MAX_BODY + MAX_BODY / 254 + 2;

/// Command opcodes. Requests carry the opcode in `cmd`; responses echo it,
/// except failures which use [`cmd::ERROR`].
pub mod cmd {
    /// Handshake. Req payload `[client_proto]`; resp payload `[PROTO_VERSION]`.
    pub const HELLO: u8 = 0x01;
    /// Read the device's current config. Req empty; resp payload = config blob.
    pub const GET_CONFIG: u8 = 0x02;
    /// Replace the device's config. Req payload = config blob; resp empty on OK.
    /// The device validates, persists, and hot-reloads before replying.
    pub const SET_CONFIG: u8 = 0x03;
    /// Reboot into the firmware (e.g. after a config that needs a clean start).
    pub const REBOOT: u8 = 0x04;
    /// Failure reply. Payload `[ProtoError as u8]`.
    pub const ERROR: u8 = 0xFF;
}

/// Application-level error codes carried in an [`cmd::ERROR`] reply.
#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format)]
#[repr(u8)]
pub enum ProtoError {
    /// Unknown / unsupported opcode.
    BadCommand = 1,
    /// Payload was not a valid config.
    BadPayload = 2,
    /// Persisting the config to flash failed.
    StoreFailed = 3,
}

/// Codec (framing) errors — distinct from [`ProtoError`] (which is an
/// application reply). These mean the bytes on the wire were malformed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format)]
pub enum FrameError {
    /// Decoded body shorter than the minimum (cmd+seq+crc).
    TooShort,
    /// COBS structure was invalid (a stray `0x00`, or a truncated run).
    BadCobs,
    /// CRC mismatch — the body was corrupted in transit.
    BadCrc,
    /// An output/scratch buffer was too small.
    Overflow,
}

/// A decoded frame: opcode, sequence id (echoed in the reply for matching),
/// and the payload — borrowing the caller's decode buffer.
pub struct Frame<'a> {
    pub cmd: u8,
    pub seq: u8,
    pub payload: &'a [u8],
}

/// CRC-16/CCITT-FALSE: poly `0x1021`, init `0xFFFF`, no reflection, no xorout.
/// (Check value for `b"123456789"` is `0x29B1`.)
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Build a frame for `(cmd, seq, payload)` into `out`, using `body` as scratch
/// to assemble `cmd|seq|payload|crc` before COBS-encoding. Returns the frame
/// length written to `out` (including the trailing `0x00`). `body` must hold
/// `payload.len()+4`; `out` must hold the COBS-encoded result + 1.
pub fn encode(
    cmd: u8,
    seq: u8,
    payload: &[u8],
    body: &mut [u8],
    out: &mut [u8],
) -> Result<usize, FrameError> {
    let body_len = payload.len() + 4;
    if body.len() < body_len {
        return Err(FrameError::Overflow);
    }
    body[0] = cmd;
    body[1] = seq;
    body[2..2 + payload.len()].copy_from_slice(payload);
    let crc = crc16(&body[..2 + payload.len()]);
    body[2 + payload.len()] = (crc >> 8) as u8;
    body[3 + payload.len()] = crc as u8;

    let n = cobs_encode(&body[..body_len], out)?;
    if n >= out.len() {
        return Err(FrameError::Overflow);
    }
    out[n] = 0; // frame delimiter
    Ok(n + 1)
}

/// Decode one frame (the bytes **before** the `0x00` delimiter — the caller
/// splits the stream on `0x00`) into `body`, verifying COBS structure and CRC.
/// Returns a [`Frame`] borrowing `body`.
pub fn decode<'a>(frame: &[u8], body: &'a mut [u8]) -> Result<Frame<'a>, FrameError> {
    let n = cobs_decode(frame, body)?;
    if n < 4 {
        return Err(FrameError::TooShort);
    }
    let crc_pos = n - 2;
    let want = ((body[crc_pos] as u16) << 8) | body[crc_pos + 1] as u16;
    if crc16(&body[..crc_pos]) != want {
        return Err(FrameError::BadCrc);
    }
    Ok(Frame {
        cmd: body[0],
        seq: body[1],
        payload: &body[2..crc_pos],
    })
}

// ── COBS ────────────────────────────────────────────────────────────────
// Hand-rolled (rather than the `cobs` crate) to keep the framing fully owned
// and unit-tested here. Standard COBS: blocks of up to 254 non-zero bytes are
// prefixed by a code = (run length + 1); a code of 0xFF means "254 bytes, no
// implicit zero follows".

/// COBS-encode `input` into `out` (no delimiter). Returns bytes written.
///
/// Round-trips every input exactly (verified across 20k+ cases incl. the
/// 254-run / trailing-zero edges). It is *not* byte-minimal in one case: a
/// block of exactly 254 non-zero bytes with no following zero emits a redundant
/// trailing `0x01` block (`FF .. 01` vs the minimal `FF ..`). Both are valid
/// COBS — [`cobs_decode`] accepts either — so this stays interop-safe with a
/// minimal encoder on the webapp side. (Chasing the minimal form there is a
/// known foot-gun: it breaks the "254 non-zero bytes then a zero" case.)
fn cobs_encode(input: &[u8], out: &mut [u8]) -> Result<usize, FrameError> {
    if out.is_empty() {
        return Err(FrameError::Overflow);
    }
    let mut write = 1; // out[0] reserved for the first block's code
    let mut code_pos = 0;
    let mut code: u8 = 1;
    for &b in input {
        if b == 0 {
            out[code_pos] = code;
            code_pos = write;
            if write >= out.len() {
                return Err(FrameError::Overflow);
            }
            write += 1; // reserve next code byte
            code = 1;
        } else {
            if write >= out.len() {
                return Err(FrameError::Overflow);
            }
            out[write] = b;
            write += 1;
            code += 1;
            if code == 0xFF {
                out[code_pos] = code;
                code_pos = write;
                if write >= out.len() {
                    return Err(FrameError::Overflow);
                }
                write += 1;
                code = 1;
            }
        }
    }
    out[code_pos] = code;
    Ok(write)
}

/// COBS-decode `input` into `out`. Returns bytes written.
fn cobs_decode(input: &[u8], out: &mut [u8]) -> Result<usize, FrameError> {
    let mut read = 0;
    let mut write = 0;
    while read < input.len() {
        let code = input[read];
        read += 1;
        if code == 0 {
            return Err(FrameError::BadCobs); // a 0 byte never appears inside a frame
        }
        for _ in 1..code {
            let b = *input.get(read).ok_or(FrameError::BadCobs)?;
            read += 1;
            if write >= out.len() {
                return Err(FrameError::Overflow);
            }
            out[write] = b;
            write += 1;
        }
        // A non-0xFF block that isn't the last one represents a trailing zero.
        if code != 0xFF && read < input.len() {
            if write >= out.len() {
                return Err(FrameError::Overflow);
            }
            out[write] = 0;
            write += 1;
        }
    }
    Ok(write)
}
