//! Channel contracts — the message types tasks exchange through
//! `embassy_sync` channels.
//!
//! Freezing these in one place lets independent subsystem tasks (LEDs,
//! MIDI, encoder, expression, …) be built in parallel against a stable
//! interface, and lets the router match on a known set. **Adding** enum
//! variants is backward-compatible as long as the router keeps a
//! catch-all arm; **changing** existing fields is breaking — coordinate
//! through the router owner.
//!
//! v0 scope: channel-voice MIDI plus per-switch / per-pedal intent.
//! Streaming SysEx is intentionally NOT a `Copy` message here — it is
//! owned and parsed inside the MIDI module (`src/midi/`), which exposes
//! its own send/receive API for it. Revisit once the MIDI workstream
//! lands and we know the buffering it needs (likely `heapless::Vec`).

/// A debounced footswitch edge. `index` is `0..10` in the order the
/// buttons task scans — see `SWITCH_NAMES` in `bin/midicaptain.rs`.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct ButtonEvent {
    pub index: u8,
    pub pressed: bool,
}

/// Rotary-encoder motion. `Turn` carries a signed detent delta
/// (`+1` = clockwise, `-1` = counter-clockwise); the decoder accumulates
/// sub-detent quadrature internally and only emits whole detents.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum EncoderEvent {
    Turn(i8),
    Press,
    Release,
}

/// A calibrated expression-pedal reading. `pedal` is `0` or `1`; `value`
/// is `0..=127` after min/max calibration is applied.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct ExprEvent {
    pub pedal: u8,
    pub value: u8,
}

/// Normalised inbound MIDI, merged from USB-MIDI and the DIN UART.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum MidiRx {
    ControlChange { channel: u8, cc: u8, value: u8 },
    ProgramChange { channel: u8, program: u8 },
    Note { channel: u8, note: u8, velocity: u8, on: bool },
    /// 14-bit pitch bend, `0..=16383`, centre `8192`. Carried inbound only
    /// (the tuner reads cents from the amp's pitch bend); there is no
    /// outbound `MidiCmd` counterpart since the firmware never sends it.
    PitchBend { channel: u8, value: u16 },
}

/// Outbound MIDI command, fanned to USB + DIN by the mux task. SysEx
/// transmission is a separate API on the MIDI module, not a `Copy` here.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum MidiCmd {
    ControlChange { channel: u8, cc: u8, value: u8 },
    ProgramChange { channel: u8, program: u8 },
    Note { channel: u8, note: u8, velocity: u8, on: bool },
}

/// Per-switch RGB intent. The LEDs task expands each entry across that
/// switch's three physical WS2812 pixels (see `pins::LED_RANGES`).
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format, serde::Serialize, serde::Deserialize)]
pub struct LedColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// A full LED frame: one colour per footswitch, in `pins::Switch::ALL`
/// order (10 entries). The LEDs task is the only owner of the WS2812
/// chain; it maps this to the 30-pixel buffer.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct LedFrame {
    pub switches: [LedColor; 10],
}

/// A render request to the display task. Grows into modes (status, tuner,
/// menu, value bar) as features land — add variants, keep the router's
/// match exhaustive. [`Self::Page`]/[`Self::Action`] labels are **owned**
/// (they come from the runtime [`crate::config::RuntimeConfig`], not `'static`
/// data), so this is `Clone`, not `Copy`; a hand-written [`defmt::Format`]
/// (below) keeps it loggable without leaning on a heapless feature.
#[derive(Clone, PartialEq, Eq)]
pub enum DisplayCmd {
    /// The active page changed (or initial paint): show its name and
    /// 1-based position (`index` of `total`).
    Page {
        name: crate::config::PageName,
        index: u8,
        total: u8,
    },
    /// A button was actuated: briefly show its label, plus on/off when the
    /// button is a toggle (`toggle = false` hides the state suffix).
    Action {
        label: crate::config::Label,
        toggle: bool,
        on: bool,
    },
    /// Settings-menu item view (single item at a time): its `title`, current
    /// `value` rendered per `kind`, and whether it's being edited.
    Menu {
        title: &'static str,
        value: u16,
        kind: MenuKind,
        editing: bool,
    },
    /// Calibration-wizard step for `pedal` (`0`/`1`): the instruction and the
    /// pedal's current raw ADC reading (so the user sees it respond).
    Cal {
        pedal: u8,
        step: CalStep,
        raw: u16,
    },
    /// Chromatic tuner readout. The board has no audio input — the amp
    /// detects pitch and streams Note On + Pitch Bend back (see `tuner.py`).
    /// `note` is the MIDI note number (`None` = nothing detected, shown as
    /// `--`); `cents` is the deviation already mapped from 14-bit pitch bend
    /// (negative = flat, positive = sharp).
    Tuner {
        note: Option<u8>,
        cents: i16,
    },
}

// Hand-written so the owned-string variants can format without depending on a
// heapless `defmt` feature (and its defmt-version coupling). The `&str` fields
// print via `{=str}`; the small enums via their own derived `Format`.
impl defmt::Format for DisplayCmd {
    fn format(&self, f: defmt::Formatter) {
        match self {
            DisplayCmd::Page { name, index, total } => {
                defmt::write!(f, "Page({=str} {}/{})", name.as_str(), index, total)
            }
            DisplayCmd::Action { label, toggle, on } => {
                defmt::write!(f, "Action({=str} toggle={} on={})", label.as_str(), toggle, on)
            }
            DisplayCmd::Menu {
                title,
                value,
                kind,
                editing,
            } => defmt::write!(f, "Menu({=str} {} {} editing={})", *title, value, kind, editing),
            DisplayCmd::Cal { pedal, step, raw } => {
                defmt::write!(f, "Cal(p{} {} raw={})", pedal, step, raw)
            }
            DisplayCmd::Tuner { note, cents } => {
                defmt::write!(f, "Tuner(note={} cents={})", note, cents)
            }
        }
    }
}

/// How a [`DisplayCmd::Menu`] value is rendered.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum MenuKind {
    /// Plain integer (e.g. MIDI channel).
    Int,
    /// Percentage (append `%`).
    Percent,
    /// No value — an action item (e.g. "Cal Pedal 1", "Exit").
    Action,
}

/// Which step of the calibration wizard a [`DisplayCmd::Cal`] is showing.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum CalStep {
    /// Capture the heel (minimum).
    Min,
    /// Capture the toe (maximum).
    Max,
    /// Captured + saved.
    Done,
}
