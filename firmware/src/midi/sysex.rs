//! Streaming SysEx (System Exclusive) reassembly and packetisation.
//!
//! USB-MIDI carries data as 4-byte *USB-MIDI Event Packets*: a header
//! byte `(cable << 4) | CIN` followed by up to three MIDI bytes. SysEx,
//! being arbitrarily long, is fragmented across packets distinguished by
//! their Code Index Number (CIN):
//!
//! | CIN | meaning                                          | sig. bytes |
//! |-----|--------------------------------------------------|-----------|
//! | 0x4 | SysEx starts or continues                        | 3         |
//! | 0x5 | SysEx ends with one byte (or 1-byte sys-common)  | 1         |
//! | 0x6 | SysEx ends with two bytes                         | 2         |
//! | 0x7 | SysEx ends with three bytes                       | 3         |
//!
//! DIN MIDI is a raw byte stream with no packet framing, so the same
//! logical reassembly applies one byte at a time. The core of this module
//! is therefore byte-oriented ([`SysExBuf::push_byte`]); the USB-packet
//! adapter [`SysExBuf::push_packet`] is a thin wrapper that feeds the
//! significant bytes of a packet through it. The DIN parser in
//! [`super::mux`] reuses the same byte path.
//!
//! Reassembled messages include both the leading `0xF0` and the trailing
//! `0xF7`, so the output is the exact on-wire SysEx — directly comparable
//! to what [`super::katana`] builds.

use heapless::Vec;

/// SysEx start-of-exclusive status byte.
pub const SYSEX_START: u8 = 0xF0;
/// SysEx end-of-exclusive status byte.
pub const SYSEX_END: u8 = 0xF7;

/// Maximum reassembled SysEx length, *including* the `0xF0`/`0xF7`
/// delimiters. BOSS Katana parameter set/read messages are under 20
/// bytes; larger reads are chunked by the device. 256 covers both with
/// generous margin while costing only a quarter-KB of the RP2040's 264 KB.
pub const MAX_SYSEX: usize = 256;

/// An owned, fully reassembled SysEx message including its `0xF0`/`0xF7`
/// delimiters. Deliberately **not** `Copy`: it is moved through channels
/// rather than copied on every send, per the `events.rs` contract.
pub type SysEx = Vec<u8, MAX_SYSEX>;

/// Error returned by the SysEx builders/packetisers.
#[derive(Copy, Clone, PartialEq, Eq, Debug, defmt::Format)]
pub enum SysExError {
    /// The destination buffer ran out of capacity.
    Overflow,
}

/// Incremental SysEx reassembler.
///
/// Feed it USB-MIDI packets ([`push_packet`](Self::push_packet)) or raw
/// DIN bytes ([`push_byte`](Self::push_byte)); it yields the complete
/// message exactly once, when it sees the closing `0xF7`.
///
/// Robustness rules (matching the MIDI spec):
/// - A fresh `0xF0` abandons any partial message and starts over.
/// - Any other status byte (`0x80..=0xF6`) arriving mid-message aborts
///   it — a SysEx interrupted by a channel message is malformed and
///   dropped rather than silently corrupted.
/// - System-realtime bytes (`0xF8..=0xFF`) are transparent: they may be
///   interleaved inside a SysEx stream and never belong to it.
/// - Overflow past [`MAX_SYSEX`] drops the current message (the closing
///   `0xF7` returns `None`); the next `0xF0` recovers cleanly.
#[derive(Debug)]
pub struct SysExBuf {
    buf: Vec<u8, MAX_SYSEX>,
    /// True between a `0xF0` and its terminating `0xF7`.
    active: bool,
    /// True once the active message exceeded [`MAX_SYSEX`]; cleared when
    /// the next message starts.
    overflowed: bool,
}

impl Default for SysExBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl SysExBuf {
    /// Create an empty reassembler. `const` so it can back a `static`.
    pub const fn new() -> Self {
        Self {
            buf: Vec::new(),
            active: false,
            overflowed: false,
        }
    }

    /// Drop any partial message and return to the idle state.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.active = false;
        self.overflowed = false;
    }

    /// True if a message is currently being assembled (`0xF0` seen, no
    /// `0xF7` yet).
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Feed one raw MIDI byte. Returns `Some(msg)` exactly once, when a
    /// complete `0xF0..0xF7` message has been assembled; the slice borrows
    /// the internal buffer and is valid until the next mutating call.
    pub fn push_byte(&mut self, b: u8) -> Option<&[u8]> {
        // System-realtime bytes are single-byte messages that may appear
        // anywhere, including mid-SysEx; they are not part of it.
        if (0xF8..=0xFF).contains(&b) {
            return None;
        }

        match b {
            SYSEX_START => {
                // (Re)start; any partial message is abandoned. F0 always
                // fits in a freshly-cleared buffer.
                self.buf.clear();
                self.overflowed = false;
                self.active = true;
                let _ = self.buf.push(SYSEX_START);
                None
            }
            SYSEX_END => {
                if !self.active {
                    return None;
                }
                self.active = false;
                if self.overflowed || self.buf.push(SYSEX_END).is_err() {
                    // Overflowed mid-message, or no room for the
                    // terminator: drop it.
                    self.buf.clear();
                    self.overflowed = false;
                    return None;
                }
                Some(self.buf.as_slice())
            }
            // Any other status byte aborts a SysEx in progress.
            0x80..=0xF6 => {
                self.buf.clear();
                self.active = false;
                self.overflowed = false;
                None
            }
            // Data byte (0x00..=0x7F).
            _ => {
                if self.active && !self.overflowed && self.buf.push(b).is_err() {
                    self.overflowed = true;
                }
                None
            }
        }
    }

    /// Feed one 4-byte USB-MIDI event packet. Only the SysEx CINs
    /// (`0x4`/`0x5`/`0x6`/`0x7`) carry SysEx data; any other CIN is
    /// ignored here (channel-voice decoding lives in [`super::mux`]).
    /// Returns the assembled message when this packet completes one.
    pub fn push_packet(&mut self, packet: &[u8; 4]) -> Option<&[u8]> {
        let nbytes = match packet[0] & 0x0F {
            0x4 | 0x7 => 3, // start/continue, or "ends with three"
            0x6 => 2,       // ends with two
            0x5 => 1,       // ends with one
            _ => return None,
        };

        // A completion can only occur on the last significant byte of an
        // "end" packet, but feeding strictly in order is correct for every
        // case. Track whether any byte completed a message.
        let mut completed = false;
        for &b in &packet[1..1 + nbytes] {
            if self.push_byte(b).is_some() {
                completed = true;
            }
        }
        if completed {
            Some(self.buf.as_slice())
        } else {
            None
        }
    }
}

/// Split a complete SysEx message into USB-MIDI event packets on `cable`,
/// appending each 4-byte packet to `out`.
///
/// `msg` must be a full on-wire SysEx (leading `0xF0`, trailing `0xF7`).
/// This is the inverse of [`SysExBuf::push_packet`]: feeding the produced
/// packets back through a fresh [`SysExBuf`] reconstructs `msg` byte-for-
/// byte. Returns [`SysExError::Overflow`] if `out` runs out of capacity.
pub fn to_usb_packets<const M: usize>(
    msg: &[u8],
    cable: u8,
    out: &mut Vec<[u8; 4], M>,
) -> Result<(), SysExError> {
    let cab = (cable & 0x0F) << 4;
    let n = msg.len();
    let mut i = 0;

    // Full 3-byte "start/continue" packets while more than three remain.
    while n - i > 3 {
        out.push([cab | 0x4, msg[i], msg[i + 1], msg[i + 2]])
            .map_err(|_| SysExError::Overflow)?;
        i += 3;
    }

    // Tail of 1, 2, or 3 bytes selects the matching "ends with N" CIN, so
    // the closing 0xF7 always lands in an end packet.
    match n - i {
        3 => out
            .push([cab | 0x7, msg[i], msg[i + 1], msg[i + 2]])
            .map_err(|_| SysExError::Overflow)?,
        2 => out
            .push([cab | 0x6, msg[i], msg[i + 1], 0])
            .map_err(|_| SysExError::Overflow)?,
        1 => out
            .push([cab | 0x5, msg[i], 0, 0])
            .map_err(|_| SysExError::Overflow)?,
        // 0: nothing left (also the only other reachable value, since the
        // loop above guarantees the remainder is <= 3).
        _ => {}
    }
    Ok(())
}
