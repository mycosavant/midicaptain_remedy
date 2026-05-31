//! Configuration model — what each button *does*, per page.
//!
//! This is the router's brain in data form. A [`Config`] is a list of
//! [`Page`]s; each page binds the ten footswitches to [`Action`]s for
//! short- and long-press. The router (`bin/midicaptain.rs`) walks the
//! active page, dispatches actions, tracks toggle state, and renders LED /
//! display feedback from this data.
//!
//! v1 is **baked in** as Rust consts ([`DEFAULT_CONFIG`]) — a faithful
//! port of `remedy/config/pages/*.toml` + the action vocabulary in
//! `remedy/lib/events.py`. Loading TOML from flash is a later step
//! (`no_std` serde-TOML is a research item; the storage region already
//! exists). Until then, editing the firmware *is* editing the config.
//!
//! ## Button ordering
//!
//! Button slots are indexed by [`crate::events::ButtonEvent::index`] — the
//! GPIO **scan** order the buttons task emits (`SW1..SW4, A..D, UP, DOWN`).
//! That is **not** the WS2812 **chain** order ([`crate::pins::Switch::ALL`]
//! = `S1..S4, Up, A..D, Down`). [`SWITCH_FOR_BUTTON`] maps scan-index →
//! chain position so the router can build an [`crate::events::LedFrame`]
//! from per-button colours.

use crate::events::{HidReport, LedColor};
use crate::pins::Switch;

/// Number of footswitch slots on a page (matches the buttons task).
pub const PAGE_BUTTONS: usize = 10;

/// Max number of mutual-exclusion (radio/select) groups per page. A button's
/// [`ButtonConfig::group`] is `0` for ungrouped or `1..=MAX_GROUPS` to join a
/// group; pressing a grouped button selects it and deselects the rest of its
/// group. Group ids outside this range are treated as ungrouped (the router
/// ignores them defensively, so a malformed config can't panic). Eight is
/// ample for ten switches.
pub const MAX_GROUPS: usize = 8;

/// Max number of multi-state cycle definitions ([`CycleDef`]) a config can hold.
/// Cycles live in a shared per-config pool ([`RuntimeConfig::cycles`]); buttons
/// reference one by index via [`Action::Cycle`]. Pooling (vs storing steps in
/// each button) keeps the per-button size — and thus the worst-case blob —
/// small, and lets several buttons share a cycle.
pub const MAX_CYCLES: usize = 8;

/// Max states (steps) in one [`CycleDef`]. A button bound to that cycle advances
/// through these on each short press, wrapping at the end.
pub const MAX_STEPS: usize = 8;

/// Map a `ButtonEvent.index` (GPIO scan order, `SW1..SW4,A..D,UP,DOWN`) to
/// its WS2812 chain position ([`Switch`]). The two orders differ only in
/// where UP/DOWN sit, but that difference is real — get it wrong and the
/// lit pixel is under the wrong switch.
pub const SWITCH_FOR_BUTTON: [Switch; PAGE_BUTTONS] = [
    Switch::S1, // 0  SW1
    Switch::S2, // 1  SW2
    Switch::S3, // 2  SW3
    Switch::S4, // 3  SW4
    Switch::A,  // 4  A
    Switch::B,  // 5  B
    Switch::C,  // 6  C
    Switch::D,  // 7  D
    Switch::Up, // 8  UP
    Switch::Down, // 9 DOWN
];

/// CC value mode for [`Action::MidiCc`]. Mirrors the `value = <n> | "toggle"`
/// form in the page TOML (`remedy/lib/events.py::MidiCCAction`).
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format, serde::Serialize, serde::Deserialize)]
pub enum CcValue {
    /// Send this fixed value (`0..=127`).
    Fixed(u8),
    /// Flip between `0` and `127`, tracking the router's per-CC toggle state.
    Toggle,
    /// Non-latching: send `127` on the press edge and `0` on release. Because it
    /// acts on both edges it ignores the button's long-press action.
    ///
    /// NOTE: serde keys enum variants by position — only ever *append* variants
    /// here (a reorder would silently re-interpret every stored/pushed config).
    Momentary,
}

/// A named BOSS Katana SysEx command. The config stays free of raw Roland
/// addresses — the router builds the on-wire message via
/// [`crate::midi::katana`], which owns those.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format, serde::Serialize, serde::Deserialize)]
pub enum SysexCmd {
    /// Recall a preset: `0` = Panel, `1..=4` = CH1..CH4.
    RecallPreset(u8),
    /// Set amp type (`0..=4`: Acoustic/Clean/Crunch/Lead/Brown).
    AmpType(u8),
    /// Set gain (`0..=100`).
    Gain(u8),
    /// Set master volume (`0..=100`).
    Volume(u8),
}

/// One state of a multi-state cycle ([`CycleDef`]) — what gets emitted when a
/// cycle button lands on this step. Deliberately a **flat** action subset (no
/// nesting, no page-nav/tuner/toggle/momentary): each step sets a concrete
/// value, which is exactly the "keytimes" idiom and keeps the type bounded for
/// the worst-case size guarantee.
///
/// NOTE: serde keys variants by position — append only.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format, serde::Serialize, serde::Deserialize)]
pub enum StepAction {
    /// Send `cc = value` (a fixed CC value — the cycling-CC / keytimes case).
    MidiCc { cc: u8, value: u8 },
    /// Send a Program Change (cycle through presets).
    ProgramChange { program: u8 },
    /// Send a Katana SysEx command (cycle amp types, etc.).
    Sysex(SysexCmd),
}

/// How a cycle button's *long* press behaves. Stored per [`CycleDef`] so the
/// choice is configurable per cycle.
///
/// NOTE: serde keys variants by position — append only.
#[derive(Clone, Copy, PartialEq, Eq, Default, defmt::Format, serde::Serialize, serde::Deserialize)]
pub enum CycleLong {
    /// Long press is *not* claimed by the cycle — the button's normal
    /// [`ButtonConfig::on_long_press`] action runs (page nav, tuner, …).
    #[default]
    None,
    /// Long press resets the cycle to its first state (and emits it).
    Reset,
    /// Long press steps the cycle *backward* (and emits that state).
    Reverse,
}

/// What a button does when triggered. Port of the action types in
/// `remedy/lib/events.py::Action.from_config`.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format, serde::Serialize, serde::Deserialize)]
pub enum Action {
    /// Do nothing (unbound slot).
    None,
    /// Send a Control Change on the global MIDI channel.
    MidiCc { cc: u8, value: CcValue },
    /// Send a Program Change on the global MIDI channel.
    ProgramChange { program: u8 },
    /// Send a Katana SysEx command.
    Sysex(SysexCmd),
    /// Jump to a specific page (0-based; clamped by the router).
    PageChange(u8),
    /// Advance to the next page (wraps).
    PageNext,
    /// Return to the previous page (wraps).
    PagePrev,
    /// Enter the chromatic tuner display mode (sends CC#25 = 127 to start the
    /// amp's tuner). Leaving the tuner is a mode-level gesture, not an action —
    /// see `app::Router`.
    TunerToggle,
    /// Step the program number by a signed delta and send the resulting Program
    /// Change (preset/bank up or down). The router tracks the current program
    /// (also set by [`Action::ProgramChange`]) and clamps the result to `0..=127`.
    ///
    /// NOTE: append-only — see [`CcValue::Momentary`].
    ProgramChangeStep(i8),
    /// Multi-state cycle: a short press advances through the referenced
    /// [`CycleDef`]'s states (by index into [`RuntimeConfig::cycles`]), emitting
    /// each in turn; the long press follows the cycle's [`CycleLong`]. Only
    /// meaningful as a button's `on_press` — the router resolves it there (where
    /// it has the button index for per-button position); in any other slot it is
    /// inert. An out-of-range index is a no-op.
    ///
    /// NOTE: append-only — see [`CcValue::Momentary`].
    Cycle(u8),
    /// Emit a USB-HID report on the host — a keyboard keystroke (with optional
    /// modifiers) or a consumer-control / media key. Fire-and-forget: a press
    /// sends the "tap" (press then release) via the HID task; there is no toggle
    /// or selection state, so the LED just shows the button's base colour. The
    /// HID interface is always present on the composite device, so this works
    /// regardless of which page or config is loaded.
    ///
    /// NOTE: append-only — see [`CcValue::Momentary`]. [`HidReport`] is itself
    /// append-only (see its docs).
    Hid(HidReport),
}

impl Action {
    /// The CC a [`CcValue::Toggle`] action drives, if any. The router keys
    /// toggle state (and LED on/off feedback) by this.
    pub fn toggle_cc(self) -> Option<u8> {
        match self {
            Action::MidiCc {
                cc,
                value: CcValue::Toggle,
            } => Some(cc),
            _ => None,
        }
    }
}

/// One footswitch's binding on a page.
#[derive(Clone, Copy)]
pub struct ButtonConfig {
    /// Short label shown on the display.
    pub label: &'static str,
    /// Base LED colour (full brightness; the router dims idle/off states).
    pub color: LedColor,
    /// Action on a short press / release.
    pub on_press: Action,
    /// Action on a long press (`Action::None` = none).
    pub on_long_press: Action,
    /// Mutual-exclusion group (`0` = ungrouped, `1..=`[`MAX_GROUPS`]). Buttons
    /// that share a non-zero group are radio-style: a short press selects this
    /// one (full-brightness LED) and deselects the others in the group
    /// (dimmed). Selection is per-page and local — cleared on page change /
    /// config apply, and not driven by inbound MIDI. Takes LED precedence over
    /// toggle feedback if a button is somehow both.
    pub group: u8,
}

impl ButtonConfig {
    /// An unbound slot: dark and inert.
    pub const EMPTY: Self = Self {
        label: "",
        color: color::OFF,
        on_press: Action::None,
        on_long_press: Action::None,
        group: 0,
    };
}

/// A page: a name plus the ten button bindings (scan-index order).
#[derive(Clone, Copy)]
pub struct Page {
    pub name: &'static str,
    pub buttons: [ButtonConfig; PAGE_BUTTONS],
}

/// A multi-state cycle definition — baked (`'static`) form of [`CycleDef`].
pub struct StaticCycleDef {
    /// The states emitted in turn (short press advances, wrapping).
    pub steps: &'static [StepAction],
    /// How the long press behaves.
    pub long: CycleLong,
}

/// The whole configuration: an ordered, non-empty list of pages plus the shared
/// pool of cycle definitions buttons reference by index.
#[derive(Clone, Copy)]
pub struct Config {
    pub pages: &'static [Page],
    /// Shared cycle pool, indexed by [`Action::Cycle`].
    pub cycles: &'static [StaticCycleDef],
}

impl Config {
    /// The active page, clamped so an out-of-range index can't panic.
    pub fn page(&self, index: usize) -> &'static Page {
        let pages = self.pages;
        &pages[index.min(pages.len() - 1)]
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// Named LED colours at a current-safe "full" level. Idle/off variants are
/// derived by the LED helper (`hal::leds::idle_dim`). Channel level `L` is
/// deliberately modest: 30 pixels at full white would exceed the USB
/// budget, so "full" here is ~⅕ scale.
pub mod color {
    use crate::events::LedColor;

    /// Per-channel "full on" level. Conservative on purpose — see module note.
    const L: u8 = 0x30; // 48

    pub const OFF: LedColor = LedColor { r: 0, g: 0, b: 0 };
    pub const RED: LedColor = LedColor { r: L, g: 0, b: 0 };
    pub const GREEN: LedColor = LedColor { r: 0, g: L, b: 0 };
    pub const BLUE: LedColor = LedColor { r: 0, g: 0, b: L };
    pub const CYAN: LedColor = LedColor { r: 0, g: L, b: L };
    pub const AMBER: LedColor = LedColor { r: L, g: L / 2, b: 0 };
    pub const PURPLE: LedColor = LedColor { r: L / 2, g: 0, b: L };
    pub const WHITE: LedColor = LedColor { r: L, g: L, b: L };
}

/// USB-HID constants used by the baked demo page ([`PAGE_HID`]).
///
/// On-device we keep keycodes / modifiers / consumer usages as **raw integers**
/// (an [`HidReport`] carries `u8`/`u16`), exactly as CC numbers are raw — the
/// friendly-name table (e.g. "Space", "Ctrl+Z", "Play/Pause") is the webapp's
/// job, not the firmware's. These named consts exist only to make the demo page
/// legible; they are a tiny subset of the USB HID Usage Tables.
pub mod hid {
    /// Keyboard modifier bitmask bits (USB HID, left-hand modifiers). OR them
    /// together for combos (e.g. `CTRL | SHIFT`).
    pub mod mods {
        pub const NONE: u8 = 0x00;
        /// Left Ctrl.
        pub const CTRL: u8 = 0x01;
        /// Left Shift.
        pub const SHIFT: u8 = 0x02;
    }
    /// Keyboard usage IDs (USB HID Usage Page 0x07, "Keyboard/Keypad").
    pub mod key {
        pub const ENTER: u8 = 0x28;
        pub const SPACE: u8 = 0x2C;
        /// Letter `Z` — paired with [`super::mods::CTRL`] for Undo.
        pub const Z: u8 = 0x1D;
    }
    /// Consumer-control usage IDs (USB HID Usage Page 0x0C, "Consumer").
    pub mod consumer {
        pub const PLAY_PAUSE: u16 = 0x00CD;
        pub const MUTE: u16 = 0x00E2;
        pub const VOLUME_UP: u16 = 0x00E9;
        pub const VOLUME_DOWN: u16 = 0x00EA;
    }
}

// ── Baked-in default configuration ─────────────────────────────────────
// Two pages. Page 0 ports remedy/config/pages/default.toml (CC toggles +
// PC presets + nav). Page 1 demonstrates the SysEx path (amp types + preset
// recall). Together they exercise every Action variant.

/// Page 0 — generic MIDI controller (port of `default.toml`).
const PAGE_DEFAULT: Page = Page {
    name: "Default",
    buttons: [
        // SW1..SW4 → Program Change 0..3 (presets), a radio group (group 1):
        // the active preset stays lit, the others dim.
        radio("PRE1", color::WHITE, Action::ProgramChange { program: 0 }, 1),
        radio("PRE2", color::WHITE, Action::ProgramChange { program: 1 }, 1),
        radio("PRE3", color::WHITE, Action::ProgramChange { program: 2 }, 1),
        radio("PRE4", color::WHITE, Action::ProgramChange { program: 3 }, 1),
        // A..D → CC toggles (FX on/off), with LED on/off feedback. D also
        // long-presses into the tuner.
        button("FX1", color::GREEN, toggle(80)),
        button("FX2", color::BLUE, toggle(81)),
        // C → a 3-state cycle (CC 82 = 0/64/127), demoing "keytimes". Long-press
        // resets it to the first state (CycleLong::Reset). References cycle 0.
        cycle_button("LVL", color::AMBER, 0),
        ButtonConfig {
            label: "FX4",
            color: color::PURPLE,
            on_press: toggle(83),
            on_long_press: Action::TunerToggle,
            group: 0,
        },
        // UP/DOWN → bank step (program ±1) on a short press, page nav on a long
        // press. Same `nav` pair on every page so navigation is uniform.
        nav("BANK+", 1, Action::PageNext),
        nav("BANK-", -1, Action::PagePrev),
    ],
};

/// Page 1 — Katana SysEx (amp types + channel recall).
const PAGE_KATANA: Page = Page {
    name: "Katana",
    buttons: [
        // Amp types (group 1) and channel recall (group 2) are two independent
        // radio groups — the active amp and the active channel each stay lit.
        radio("CLEAN", color::GREEN, Action::Sysex(SysexCmd::AmpType(1)), 1),
        radio("CRUNCH", color::AMBER, Action::Sysex(SysexCmd::AmpType(2)), 1),
        radio("LEAD", color::RED, Action::Sysex(SysexCmd::AmpType(3)), 1),
        // BROWN amp on press (group 1); long-press into the tuner (consistent
        // with D on the default page). Group selection latches on the short
        // press only — a long-press enters the tuner without changing the amp.
        ButtonConfig {
            label: "BROWN",
            color: color::PURPLE,
            on_press: Action::Sysex(SysexCmd::AmpType(4)),
            on_long_press: Action::TunerToggle,
            group: 1,
        },
        radio("CH1", color::BLUE, Action::Sysex(SysexCmd::RecallPreset(1)), 2),
        radio("CH2", color::BLUE, Action::Sysex(SysexCmd::RecallPreset(2)), 2),
        radio("CH3", color::BLUE, Action::Sysex(SysexCmd::RecallPreset(3)), 2),
        radio("CH4", color::BLUE, Action::Sysex(SysexCmd::RecallPreset(4)), 2),
        // Same nav pair as every page: short = bank step, long = page change.
        nav("BANK+", 1, Action::PageNext),
        nav("BANK-", -1, Action::PagePrev),
    ],
};

/// Page 2 — USB-HID demo. Footswitches type on / send media keys to the host
/// (the device enumerates a keyboard + consumer-control HID interface alongside
/// MIDI + CDC). Demonstrates plain keys, a modifier combo (Ctrl+Z), and
/// consumer/media usages. UP/DOWN are the standard nav pair (long-press to
/// change page) so you can leave the page without the encoder.
const PAGE_HID: Page = Page {
    name: "HID",
    buttons: [
        // Keyboard keys (Usage Page 0x07): plain, then modifier combos.
        hid_key("SPACE", color::WHITE, hid::key::SPACE, hid::mods::NONE),
        hid_key("ENTER", color::WHITE, hid::key::ENTER, hid::mods::NONE),
        hid_key("UNDO", color::CYAN, hid::key::Z, hid::mods::CTRL),
        hid_key("REDO", color::CYAN, hid::key::Z, hid::mods::CTRL | hid::mods::SHIFT),
        // Consumer-control / media keys (Usage Page 0x0C).
        hid_consumer("PLAY", color::GREEN, hid::consumer::PLAY_PAUSE),
        hid_consumer("VOL+", color::BLUE, hid::consumer::VOLUME_UP),
        hid_consumer("VOL-", color::BLUE, hid::consumer::VOLUME_DOWN),
        hid_consumer("MUTE", color::AMBER, hid::consumer::MUTE),
        // Same nav pair as every page: short = bank step, long = page change.
        nav("BANK+", 1, Action::PageNext),
        nav("BANK-", -1, Action::PagePrev),
    ],
};

const PAGES: [Page; 3] = [PAGE_DEFAULT, PAGE_KATANA, PAGE_HID];

/// Shared cycle pool. Cycle 0 (referenced by page-0 "LVL") steps CC 82 through
/// three levels, with the long press resetting to the first.
const CYCLES: [StaticCycleDef; 1] = [StaticCycleDef {
    steps: &[
        StepAction::MidiCc { cc: 82, value: 0 },
        StepAction::MidiCc { cc: 82, value: 64 },
        StepAction::MidiCc { cc: 82, value: 127 },
    ],
    long: CycleLong::Reset,
}];

/// The baked-in default configuration the firmware boots with.
pub const DEFAULT_CONFIG: Config = Config {
    pages: &PAGES,
    cycles: &CYCLES,
};

/// Helper: a button with a single short-press action and no long-press.
const fn button(label: &'static str, color: LedColor, on_press: Action) -> ButtonConfig {
    ButtonConfig {
        label,
        color,
        on_press,
        on_long_press: Action::None,
        group: 0,
    }
}

/// Helper: the UP/DOWN navigation pair. A short press steps the current program
/// by `step` (bank up / down); a long press runs `page` (page nav). Used
/// identically on every page so navigation is always a long-press — a quick tap
/// never changes the page out from under you, and a short tap nudges the bank.
const fn nav(label: &'static str, step: i8, page: Action) -> ButtonConfig {
    ButtonConfig {
        label,
        color: color::CYAN,
        on_press: Action::ProgramChangeStep(step),
        on_long_press: page,
        group: 0,
    }
}

/// Helper: a radio-group button — single short-press action, no long-press,
/// member of mutual-exclusion `group` (`1..=`[`MAX_GROUPS`]).
const fn radio(
    label: &'static str,
    color: LedColor,
    on_press: Action,
    group: u8,
) -> ButtonConfig {
    ButtonConfig {
        label,
        color,
        on_press,
        on_long_press: Action::None,
        group,
    }
}

/// Helper: a CC toggle action on `cc`.
const fn toggle(cc: u8) -> Action {
    Action::MidiCc {
        cc,
        value: CcValue::Toggle,
    }
}

/// Helper: a button that sends a keyboard keystroke (Usage Page 0x07 `keycode`
/// with a `modifiers` bitmask — see [`hid::mods`]). No long-press, ungrouped.
const fn hid_key(
    label: &'static str,
    color: LedColor,
    keycode: u8,
    modifiers: u8,
) -> ButtonConfig {
    button(label, color, Action::Hid(HidReport::Key { keycode, modifiers }))
}

/// Helper: a button that sends a consumer-control / media usage (Usage Page
/// 0x0C — see [`hid::consumer`]). No long-press, ungrouped.
const fn hid_consumer(label: &'static str, color: LedColor, usage: u16) -> ButtonConfig {
    button(label, color, Action::Hid(HidReport::Consumer { usage }))
}

/// Helper: a button bound to cycle `index` (in the config's cycle pool), no
/// long-press of its own, ungrouped.
const fn cycle_button(label: &'static str, color: LedColor, index: u8) -> ButtonConfig {
    ButtonConfig {
        label,
        color,
        on_press: Action::Cycle(index),
        on_long_press: Action::None,
        group: 0,
    }
}

// ── Runtime (user-editable) configuration ──────────────────────────────
//
// The baked [`DEFAULT_CONFIG`] above is the firmware's built-in fallback. A
// *user* config — authored in the webapp, pushed over USB, persisted in flash
// — can't use `&'static str`, so the runtime model owns its strings (fixed-cap
// `heapless::String`) and pages (`heapless::Vec`). It serde-derives so it can
// round-trip through a compact `postcard` blob (see [`serialize`] /
// [`deserialize`] and `storage::Storage::{load,store}_config`).
//
// Phase A (this slice): the model + its (de)serialization + flash persistence,
// proven by `examples/config_selftest.rs`. The router still runs the baked
// `DEFAULT_CONFIG`; swapping it to `RuntimeConfig` (and the display string-
// lifetime change that implies) is the next slice.

/// Max pages a user config can hold. Bounds the model's RAM and the serialized
/// blob size. (The OEM SuperMode allowed 99; 8 is ample for v1 and keeps the
/// blob well within the scratch buffer.)
pub const MAX_PAGES: usize = 8;
/// Max bytes of a button label.
pub const LABEL_CAP: usize = 12;
/// Max bytes of a page name.
pub const NAME_CAP: usize = 16;

/// A guaranteed upper bound (bytes) on the postcard-serialized size of any
/// [`RuntimeConfig`] with at most [`MAX_PAGES`] pages — i.e. on the length of
/// the slice [`serialize`] can ever return.
///
/// Derived from the model's fixed caps so it tracks the model automatically:
/// grow a cap and this grows with it, which in turn forces the flash store's
/// scratch buffer to keep up (see the `const` assertion in `storage.rs` — the
/// settings store must size its scan buffer against this, since the config blob
/// is the largest item in its key-value map).
///
/// Worst-case postcard layout (every cap full, largest enum variants):
/// - `RuntimeConfig` → `Vec<OwnedPage, MAX_PAGES>` + `ThruRoutes` + `Vec<CycleDef, MAX_CYCLES>`
/// - `Vec<OwnedPage, MAX_PAGES>` → 1-byte length varint (`MAX_PAGES ≤ 127`) + pages
/// - `OwnedPage`  → `PageName` (1-byte len + `NAME_CAP`) + `[OwnedButton; PAGE_BUTTONS]`
/// - `OwnedButton`→ `Label` (1-byte len + `LABEL_CAP`) + `LedColor` (3) + 2 × `Action` + `group` (1)
/// - `Action`     → 1-byte discriminant + ≤4-byte payload (`Hid(Consumer{ usage: u16 })`)
/// - `ThruRoutes` → 4 × `bool` (1 byte each)
/// - `Vec<CycleDef, MAX_CYCLES>` → 1-byte length varint + cycles
/// - `CycleDef`   → `Vec<StepAction, MAX_STEPS>` (1-byte len + steps) + `CycleLong` (1)
/// - `StepAction` → 1-byte discriminant + ≤2-byte payload (`MidiCc{ cc, value }`)
pub const MAX_SERIALIZED_LEN: usize = {
    // 1-byte enum discriminant + largest variant payload.
    // `Hid(HidReport::Consumer { usage: u16 })` = HidReport disc(1) + a u16, which
    // postcard varint-encodes in up to 3 bytes = 4 — the largest payload.
    // (`MidiCc { cc, CcValue::Fixed }` = 3; `Cycle(u8)` = 1; both smaller.)
    const ACTION_MAX: usize = 1 + 4;
    // String<N> postcard-encodes as a 1-byte length (N ≤ 127) followed by N bytes.
    const BUTTON_MAX: usize =
        (1 + LABEL_CAP) + 3 /* LedColor r,g,b */ + 2 * ACTION_MAX + 1 /* group u8 */;
    const PAGE_MAX: usize = (1 + NAME_CAP) + PAGE_BUTTONS * BUTTON_MAX;
    const THRU_MAX: usize = 4; // ThruRoutes: 4 bools
    // `StepAction` disc(1) + largest payload (`MidiCc` cc(1)+value(1) = 2).
    const STEP_MAX: usize = 1 + 2;
    const CYCLE_MAX: usize = (1 /* steps len */ + MAX_STEPS * STEP_MAX) + 1 /* CycleLong */;
    const CYCLES_MAX: usize = 1 /* pool len */ + MAX_CYCLES * CYCLE_MAX;
    1 /* pages Vec length varint */ + MAX_PAGES * PAGE_MAX + THRU_MAX + CYCLES_MAX
};

/// An owned button label.
pub type Label = heapless::String<LABEL_CAP>;
/// An owned page name.
pub type PageName = heapless::String<NAME_CAP>;

/// One footswitch binding — owned (runtime/user) form of [`ButtonConfig`].
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnedButton {
    pub label: Label,
    pub color: LedColor,
    pub on_press: Action,
    pub on_long_press: Action,
    /// Mutual-exclusion group (`0` = ungrouped). See [`ButtonConfig::group`].
    ///
    /// NOTE: serde/postcard serializes struct fields in declaration order — this
    /// is **appended** after `on_long_press`. Adding it is a breaking wire change
    /// (an older blob lacks the trailing byte per button and fails to
    /// deserialize, falling back to the default on upgrade), so
    /// [`crate::proto::PROTO_VERSION`] is bumped alongside it. Only append; never
    /// reorder.
    pub group: u8,
}

/// One page — owned (runtime/user) form of [`Page`].
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnedPage {
    pub name: PageName,
    pub buttons: [OwnedButton; PAGE_BUTTONS],
}

/// MIDI-thru routing matrix — which inbound port is forwarded to which outbound
/// port. Each route is independent and defaults off (no forwarding). It is a
/// device-global, applied in [`crate::midi::mux`] (the transport layer reads it
/// live), not per page.
#[derive(Clone, Copy, PartialEq, Eq, Default, defmt::Format, serde::Serialize, serde::Deserialize)]
pub struct ThruRoutes {
    /// Forward USB-MIDI input to the DIN output.
    pub usb_to_din: bool,
    /// Forward DIN input to the USB-MIDI output.
    pub din_to_usb: bool,
    /// Forward DIN input back to the DIN output (5-pin daisy-chain passthrough).
    pub din_to_din: bool,
    /// Forward USB-MIDI input back to the USB-MIDI output (host loopback; rare).
    pub usb_to_usb: bool,
}

impl ThruRoutes {
    /// No routing — the default and the `static` initialiser in the mux.
    pub const NONE: Self = Self {
        usb_to_din: false,
        din_to_usb: false,
        din_to_din: false,
        usb_to_usb: false,
    };
}

/// A multi-state cycle — owned (runtime/user) form of [`StaticCycleDef`]. Lives
/// in [`RuntimeConfig::cycles`]; buttons reference it by index via
/// [`Action::Cycle`]. A short press advances through `steps` (wrapping); the
/// long press follows `long`.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CycleDef {
    pub steps: heapless::Vec<StepAction, MAX_STEPS>,
    pub long: CycleLong,
}

/// A complete user configuration: an ordered list of owned pages, the
/// device-global MIDI-thru routing, and the shared cycle pool.
///
/// NOTE: serde/postcard serializes fields in declaration order. New fields are
/// **appended** (`midi_thru` after `pages`, then `cycles`). Adding a field is a
/// breaking wire change (an older blob lacks the trailing bytes and fails to
/// deserialize, falling back to the default on upgrade), so
/// [`crate::proto::PROTO_VERSION`] is bumped alongside it. Only append; never
/// reorder.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RuntimeConfig {
    pub pages: heapless::Vec<OwnedPage, MAX_PAGES>,
    /// Device-global MIDI-thru routing, applied by [`crate::midi::mux`].
    pub midi_thru: ThruRoutes,
    /// Shared cycle pool, referenced by [`Action::Cycle`] (index).
    pub cycles: heapless::Vec<CycleDef, MAX_CYCLES>,
}

/// Copy `s` into a fixed-cap string, truncating at a char boundary if it
/// overflows `N` (well-formed for non-ASCII input).
fn copy_str<const N: usize>(s: &str) -> heapless::String<N> {
    let mut out = heapless::String::new();
    for ch in s.chars() {
        if out.push(ch).is_err() {
            break;
        }
    }
    out
}

impl OwnedButton {
    fn from_static(b: &ButtonConfig) -> Self {
        Self {
            label: copy_str(b.label),
            color: b.color,
            on_press: b.on_press,
            on_long_press: b.on_long_press,
            group: b.group,
        }
    }
}

impl OwnedPage {
    fn from_static(p: &Page) -> Self {
        Self {
            name: copy_str(p.name),
            buttons: core::array::from_fn(|i| OwnedButton::from_static(&p.buttons[i])),
        }
    }
}

impl CycleDef {
    fn from_static(c: &StaticCycleDef) -> Self {
        let mut steps = heapless::Vec::new();
        for s in c.steps {
            if steps.push(*s).is_err() {
                break; // steps beyond MAX_STEPS are dropped
            }
        }
        Self {
            steps,
            long: c.long,
        }
    }
}

impl RuntimeConfig {
    /// Build an owned config from a baked [`Config`], cloning the `'static`
    /// strings into owned ones. Pages beyond [`MAX_PAGES`] (and cycles beyond
    /// [`MAX_CYCLES`]) are dropped.
    pub fn from_static(c: &Config) -> Self {
        let mut pages = heapless::Vec::new();
        for p in c.pages {
            if pages.push(OwnedPage::from_static(p)).is_err() {
                break;
            }
        }
        let mut cycles = heapless::Vec::new();
        for cy in c.cycles {
            if cycles.push(CycleDef::from_static(cy)).is_err() {
                break;
            }
        }
        Self {
            pages,
            midi_thru: ThruRoutes::NONE,
            cycles,
        }
    }

    /// The firmware's built-in default, as an owned config.
    pub fn default_config() -> Self {
        Self::from_static(&DEFAULT_CONFIG)
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Borrow a page by index, clamped so an out-of-range index can't panic.
    pub fn page(&self, index: usize) -> &OwnedPage {
        let last = self.pages.len().saturating_sub(1);
        &self.pages[index.min(last)]
    }

    /// Borrow a cycle definition by index ([`Action::Cycle`]), or `None` if the
    /// index is out of range (an unbound / malformed reference — the router
    /// treats it as a no-op).
    pub fn cycle(&self, index: u8) -> Option<&CycleDef> {
        self.cycles.get(index as usize)
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

/// A config (de)serialization failure.
#[derive(Clone, Copy, PartialEq, Eq, Debug, defmt::Format)]
pub enum ConfigError {
    /// The config did not fit the provided buffer / was otherwise un-encodable.
    Serialize,
    /// The bytes were not a valid serialized config.
    Deserialize,
}

/// Serialize a config into `buf` as a compact postcard blob, returning the
/// written prefix. `buf` must be large enough (a few KB covers [`MAX_PAGES`]).
pub fn serialize<'a>(cfg: &RuntimeConfig, buf: &'a mut [u8]) -> Result<&'a [u8], ConfigError> {
    postcard::to_slice(cfg, buf)
        .map(|written| &*written)
        .map_err(|_| ConfigError::Serialize)
}

/// Deserialize a config from a postcard blob.
pub fn deserialize(bytes: &[u8]) -> Result<RuntimeConfig, ConfigError> {
    postcard::from_bytes(bytes).map_err(|_| ConfigError::Deserialize)
}
