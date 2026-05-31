//! MIDI engine: transport mux, streaming-SysEx (de)framing, and
//! Roland/BOSS-Katana message builders.
//!
//! - [`mux`] owns both MIDI transports (USB-MIDI + DIN UART0) and turns
//!   them into normalised [`crate::events::MidiRx`] / [`crate::events::MidiCmd`]
//!   plus reassembled SysEx, exchanged over `embassy_sync` channels.
//! - [`sysex`] reassembles SysEx fragmented across USB-MIDI 4-byte
//!   packets (the hard part: CIN `0x4`/`0x5`/`0x6`/`0x7`) and packetises
//!   outbound SysEx — byte-exact with the DIN byte stream.
//! - [`katana`] ports the Roland checksum and RQ1/DT1 builders from
//!   `remedy/lib/midi.py`.
//!
//! See `ARCHITECTURE.md` for where the mux sits on the task graph and the
//! channel rules it follows (bounded queues, drop-newest for time-
//! sensitive RX, block-the-producer for commands).

pub mod katana;
pub mod mux;
pub mod sysex;

pub use sysex::{SysEx, SysExBuf, SysExError, MAX_SYSEX};

pub use mux::{
    decode_usb_channel, decode_voice, encode_din, encode_usb, DinOut, DinParser, MidiCmdChannel,
    MidiRxChannel, SysExChannel,
};
