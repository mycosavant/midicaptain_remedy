//! Chromatic tuner state — cooperative with the connected amp.
//!
//! Ported in spirit from `remedy/lib/tuner.py::TunerState`. The MIDI Captain
//! has no audio input, so it does **no** pitch detection: it asks the amp to
//! enter tuner mode (CC#25 = 127), the amp detects pitch from the guitar and
//! streams the result back as **Note On** (which note) + **Pitch Bend** (how
//! many cents off). This struct holds that readout; the router feeds it from
//! [`crate::events::MidiRx`] and the [`crate::ui::TunerView`] widget paints it.
//!
//! It is pure state — no I/O. The note→name mapping and colour thresholds
//! live in the view, where presentation belongs; here we keep only the raw
//! MIDI note number and the mapped cents.

use crate::events::DisplayCmd;

/// MIDI pitch-bend centre (14-bit, `0..=16383`). 8192 = no deviation.
const PITCH_BEND_CENTRE: i32 = 8192;

/// Cents of deviation at full pitch-bend deflection. Matches the CP default
/// (`DEFAULT_PITCH_BEND_CENTS`): the amp maps its tuner's ±range onto the
/// full 14-bit bend, and ±200 cents (a whole tone) is the convention.
const PITCH_BEND_RANGE_CENTS: i32 = 200;

/// Current tuner readout: the detected note and its deviation in cents.
pub struct TunerState {
    /// MIDI note number of the detected pitch, or `None` when nothing is
    /// sounding (no note yet, or a Note Off cleared it).
    note: Option<u8>,
    /// Deviation in cents (negative = flat, positive = sharp).
    cents: i16,
}

impl TunerState {
    pub const fn new() -> Self {
        Self {
            note: None,
            cents: 0,
        }
    }

    /// Reset to "no note" — used when entering or leaving tuner mode so a
    /// stale reading from a previous session never lingers.
    pub fn reset(&mut self) {
        self.note = None;
        self.cents = 0;
    }

    /// A Note On named the detected pitch.
    pub fn update_note(&mut self, note: u8) {
        self.note = Some(note);
    }

    /// A Note Off (or zero-velocity Note On) — the pitch stopped sounding.
    /// Clears the note *and* the cents so the bar recentres at `--`.
    pub fn clear_note(&mut self) {
        self.note = None;
        self.cents = 0;
    }

    /// Map a 14-bit pitch-bend value to cents of deviation. Centre (8192)
    /// is 0 cents; the full range spans ±[`PITCH_BEND_RANGE_CENTS`].
    pub fn update_pitch_bend(&mut self, value: u16) {
        let offset = value as i32 - PITCH_BEND_CENTRE; // -8192..=8191
        self.cents = ((offset * PITCH_BEND_RANGE_CENTS) / PITCH_BEND_CENTRE) as i16;
    }

    /// The render request for the current readout.
    pub fn display_cmd(&self) -> DisplayCmd {
        DisplayCmd::Tuner {
            note: self.note,
            cents: self.cents,
        }
    }
}

impl Default for TunerState {
    fn default() -> Self {
        Self::new()
    }
}
