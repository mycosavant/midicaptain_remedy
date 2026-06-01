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

/// A USB-HID report to emit on the host. Carried to the HID task
/// (`hal::hid::hid_loop`), which writes a press-then-release ("tap") report on
/// the interrupt endpoint. Also embedded directly in
/// [`crate::config::Action::Hid`]: unlike CC (where the router resolves the
/// channel and toggle state), a HID action is already fully concrete, so the
/// config value and the wire message are one and the same type.
///
/// NOTE: serde keys enum variants by position — only ever *append* variants here
/// (a reorder would silently re-interpret every stored/pushed config). It is
/// `serde` + `Copy` for exactly that embedding in the config model (cf.
/// [`LedColor`]).
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format, serde::Serialize, serde::Deserialize)]
pub enum HidReport {
    /// A keyboard keystroke: a USB Usage-Page-0x07 `keycode` plus a `modifiers`
    /// bitmask (bit0 LeftCtrl, bit1 LeftShift, bit2 LeftAlt, bit3 LeftGUI; bits
    /// 4–7 the right-hand modifiers). The HID task emits a press then an
    /// all-keys-up release, so it behaves as a momentary tap with the held
    /// modifiers — e.g. Ctrl+Z.
    Key { keycode: u8, modifiers: u8 },
    /// A Consumer-Control (media / transport) `usage` on USB Usage Page 0x0C —
    /// e.g. `0x00CD` Play/Pause, `0x00E9`/`0x00EA` Volume Up/Down, `0x00B5`/
    /// `0x00B6` Scan Next/Prev. These are *not* keyboard keys; the host routes
    /// them through a separate report. Emitted as a press then a zero-usage
    /// release. 16-bit because consumer usages exceed 255.
    Consumer { usage: u16 },
}

/// A full LED frame: one colour per footswitch, in `pins::Switch::ALL`
/// order (10 entries). The LEDs task is the only owner of the WS2812
/// chain; it maps this to the 30-pixel buffer.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct LedFrame {
    pub switches: [LedColor; 10],
}

/// Per-cell presentation state for the performance [`crate::ui::PageGrid`].
/// Derived by the router (`app::Router::cell_state`) from the live toggle /
/// group / cycle / momentary state, mirroring the LED feedback precedence
/// (cycle > group > toggle). Purely presentational — carried in
/// [`DisplayCmd::Page`].
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum CellState {
    /// Unbound slot — drawn blank.
    Empty,
    /// Bound, no latching state (Program Change / SysEx / HID / page nav) —
    /// drawn lit, no state glyph. A transient press flashes the cell (see
    /// [`DisplayCmd::Flash`]).
    Plain,
    /// CC toggle: on (lit) / off (dim).
    Toggle(bool),
    /// Radio/select-group member: selected (lit) / unselected (dim).
    Radio(bool),
    /// Multi-state cycle position. `pos` is 1-based (`0` = not yet pressed);
    /// `len` is the cycle length. Drawn lit when `pos > 1` (off the base state).
    Cycle { pos: u8, len: u8 },
    /// Momentary CC: held (lit) / idle (dim).
    Momentary(bool),
}

/// One footswitch's cell in the performance page-grid snapshot: its label and
/// colour (from the active page) plus the live [`CellState`]. The label is an
/// owned [`crate::config::Label`] (runtime config), so this is `Clone`, not
/// `Copy` — like [`DisplayCmd`] itself.
#[derive(Clone, PartialEq, Eq)]
pub struct Cell {
    pub label: crate::config::Label,
    pub color: LedColor,
    pub state: CellState,
}

/// A render request to the display task. It is an **in-process** channel
/// message (router → display task) — `Clone` because the [`Self::Page`] snapshot
/// owns its strings (runtime [`crate::config::RuntimeConfig`] data, not
/// `'static`). Unlike the wire/flash config it is never serialized, so the only
/// contract is the exhaustive `match` in `bin/midicaptain.rs::display_task`;
/// change it on both ends (no `proto::PROTO_VERSION` bump). The hand-written
/// [`defmt::Format`] (below) keeps it loggable without a heapless `defmt`
/// feature.
//
// `Page` carries the whole page snapshot (~263 B: ten owned [`Cell`]s) while the
// other variants are tiny — a large variant spread. With no allocator (`no_std`)
// there is nothing to box into, and the snapshot must move by value through the
// display channel (depth-bounded, so only a handful live in `.bss` at once), so
// the spread is intrinsic — exactly like [`crate::app::ConfigReq`].
#[allow(clippy::large_enum_variant)]
#[derive(Clone, PartialEq, Eq)]
pub enum DisplayCmd {
    /// Full performance-screen snapshot — sent on the initial paint, on every
    /// page change, and after any state change (toggle / group / cycle /
    /// momentary / inbound-CC sync). Carries the page `name`, 1-based position
    /// (`index` of `total`), the current program (for the header), and one
    /// [`Cell`] per footswitch. The [`crate::ui::PageGrid`] widget diffs this
    /// against what it last drew and repaints only the cells that changed.
    Page {
        name: crate::config::PageName,
        index: u8,
        total: u8,
        program: u8,
        cells: [Cell; crate::config::PAGE_BUTTONS],
    },
    /// Briefly highlight one cell (scan `index`) — transient feedback for a
    /// non-latching press (Program Change / SysEx / HID / page-step) that has no
    /// persistent [`CellState`] to show. The display task draws the pressed
    /// style, then restores the cell after a short timeout.
    Flash {
        index: u8,
    },
    /// Scrolling list view — the settings menu and the config editor. Carries a
    /// `title`, the pre-formatted `rows` (built by the menu / editor), the
    /// `selected` cursor row, and whether the cursor is being edited (the
    /// [`crate::ui::ListView`] brightens it). Replaces the old one-item-at-a-time
    /// `Menu`/`Edit` views with a denser multi-row display.
    List {
        title: ListLine,
        rows: heapless::Vec<ListLine, LIST_MAX_ROWS>,
        selected: u8,
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
    /// Live continuous-control levels (`0..=127`) for the on-screen meters: the
    /// two expression pedals and the encoder. **Screen-neutral** — it overlays
    /// the performance grid and never switches the active screen; the display
    /// task only applies it while the grid is showing. Sent by the router on
    /// each expr/encoder change in performance mode. The `PageGrid` widget draws
    /// these in reserved edge/footer lanes (see `ui::page_grid`).
    Meters {
        exp1: u8,
        exp2: u8,
        encoder: u8,
    },
}

/// A single pre-formatted line (title or row) in a [`DisplayCmd::List`] view.
pub type ListLine = heapless::String<24>;
/// Max rows a [`DisplayCmd::List`] snapshot carries (the [`crate::ui::ListView`]
/// scrolls when the list is longer than it can show at once). Lives here, not in
/// `ui`, because `events` is the lower layer (`ui` depends on it, not vice-versa).
pub const LIST_MAX_ROWS: usize = 12;

// Hand-written so the owned-string variants can format without depending on a
// heapless `defmt` feature (and its defmt-version coupling). The `&str` fields
// print via `{=str}`; the small enums via their own derived `Format`.
impl defmt::Format for DisplayCmd {
    fn format(&self, f: defmt::Formatter) {
        match self {
            DisplayCmd::Page { name, index, total, program, .. } => {
                defmt::write!(f, "Page({=str} {}/{} pc={})", name.as_str(), index, total, program)
            }
            DisplayCmd::Flash { index } => defmt::write!(f, "Flash(#{})", index),
            DisplayCmd::List {
                title,
                rows,
                selected,
                editing,
            } => defmt::write!(
                f,
                "List({=str} [{}] sel={} editing={})",
                title.as_str(),
                rows.len(),
                selected,
                editing
            ),
            DisplayCmd::Cal { pedal, step, raw } => {
                defmt::write!(f, "Cal(p{} {} raw={})", pedal, step, raw)
            }
            DisplayCmd::Tuner { note, cents } => {
                defmt::write!(f, "Tuner(note={} cents={})", note, cents)
            }
            DisplayCmd::Meters { exp1, exp2, encoder } => {
                defmt::write!(f, "Meters(e1={} e2={} enc={})", exp1, exp2, encoder)
            }
        }
    }
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
