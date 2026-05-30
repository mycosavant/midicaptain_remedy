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

use crate::events::LedColor;
use crate::pins::Switch;

/// Number of footswitch slots on a page (matches the buttons task).
pub const PAGE_BUTTONS: usize = 10;

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
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum CcValue {
    /// Send this fixed value (`0..=127`).
    Fixed(u8),
    /// Flip between `0` and `127`, tracking the router's per-CC toggle state.
    Toggle,
}

/// A named BOSS Katana SysEx command. The config stays free of raw Roland
/// addresses — the router builds the on-wire message via
/// [`crate::midi::katana`], which owns those.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
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

/// What a button does when triggered. Port of the action types in
/// `remedy/lib/events.py::Action.from_config`.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
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
    /// Toggle the tuner display mode (Wave 3 — no-op until the tuner lands).
    TunerToggle,
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
}

impl ButtonConfig {
    /// An unbound slot: dark and inert.
    pub const EMPTY: Self = Self {
        label: "",
        color: color::OFF,
        on_press: Action::None,
        on_long_press: Action::None,
    };
}

/// A page: a name plus the ten button bindings (scan-index order).
#[derive(Clone, Copy)]
pub struct Page {
    pub name: &'static str,
    pub buttons: [ButtonConfig; PAGE_BUTTONS],
}

/// The whole configuration: an ordered, non-empty list of pages.
#[derive(Clone, Copy)]
pub struct Config {
    pub pages: &'static [Page],
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

// ── Baked-in default configuration ─────────────────────────────────────
// Two pages. Page 0 ports remedy/config/pages/default.toml (CC toggles +
// PC presets + nav). Page 1 demonstrates the SysEx path (amp types + preset
// recall). Together they exercise every Action variant.

/// Page 0 — generic MIDI controller (port of `default.toml`).
const PAGE_DEFAULT: Page = Page {
    name: "Default",
    buttons: [
        // SW1..SW4 → Program Change 0..3 (presets).
        button("PRE1", color::WHITE, Action::ProgramChange { program: 0 }),
        button("PRE2", color::WHITE, Action::ProgramChange { program: 1 }),
        button("PRE3", color::WHITE, Action::ProgramChange { program: 2 }),
        button("PRE4", color::WHITE, Action::ProgramChange { program: 3 }),
        // A..D → CC toggles (FX on/off), with LED on/off feedback.
        button("FX1", color::GREEN, toggle(80)),
        button("FX2", color::BLUE, toggle(81)),
        button("FX3", color::AMBER, toggle(82)),
        button("FX4", color::PURPLE, toggle(83)),
        // UP/DOWN → PC on press, page nav on long-press.
        ButtonConfig {
            label: "BANK+",
            color: color::CYAN,
            on_press: Action::ProgramChange { program: 4 },
            on_long_press: Action::PageNext,
        },
        ButtonConfig {
            label: "BANK-",
            color: color::CYAN,
            on_press: Action::ProgramChange { program: 5 },
            on_long_press: Action::PagePrev,
        },
    ],
};

/// Page 1 — Katana SysEx (amp types + channel recall).
const PAGE_KATANA: Page = Page {
    name: "Katana",
    buttons: [
        button("CLEAN", color::GREEN, Action::Sysex(SysexCmd::AmpType(1))),
        button("CRUNCH", color::AMBER, Action::Sysex(SysexCmd::AmpType(2))),
        button("LEAD", color::RED, Action::Sysex(SysexCmd::AmpType(3))),
        button("BROWN", color::PURPLE, Action::Sysex(SysexCmd::AmpType(4))),
        button("CH1", color::BLUE, Action::Sysex(SysexCmd::RecallPreset(1))),
        button("CH2", color::BLUE, Action::Sysex(SysexCmd::RecallPreset(2))),
        button("CH3", color::BLUE, Action::Sysex(SysexCmd::RecallPreset(3))),
        button("CH4", color::BLUE, Action::Sysex(SysexCmd::RecallPreset(4))),
        button("PAGE+", color::CYAN, Action::PageNext),
        button("PAGE-", color::CYAN, Action::PagePrev),
    ],
};

const PAGES: [Page; 2] = [PAGE_DEFAULT, PAGE_KATANA];

/// The baked-in default configuration the firmware boots with.
pub const DEFAULT_CONFIG: Config = Config { pages: &PAGES };

/// Helper: a button with a single short-press action and no long-press.
const fn button(label: &'static str, color: LedColor, on_press: Action) -> ButtonConfig {
    ButtonConfig {
        label,
        color,
        on_press,
        on_long_press: Action::None,
    }
}

/// Helper: a CC toggle action on `cc`.
const fn toggle(cc: u8) -> Action {
    Action::MidiCc {
        cc,
        value: CcValue::Toggle,
    }
}
