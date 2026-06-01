//! On-device config editor (a display mode), mirroring [`crate::menu`].
//!
//! Encoder-driven, one-field-at-a-time. The router owns an [`Editor`] and routes
//! inputs to it while in [`crate::app`]'s edit mode; the editor mutates the live
//! [`RuntimeConfig`]'s current page and returns an [`EditOutcome`] the router
//! acts on (repaint, or save + exit). It performs no I/O and holds no config of
//! its own — just the cursor.
//!
//! ## UX
//!
//! - **Tap a footswitch** to pick the switch to edit (the physical switch you
//!   want to configure).
//! - **Encoder turn** moves between that switch's fields, or — while editing —
//!   changes the highlighted field's value.
//! - **Encoder short-press** toggles editing the highlighted field.
//! - **Encoder hold** saves (persists + hot-reloads) and returns to performance.
//!
//! ## Scope (v1)
//!
//! Edits the **short-press action** + **LED colour** of buttons on the *current*
//! page: the action TYPE (None / CC toggle / momentary / trigger / fixed /
//! Program Change / Tap Tempo / Cycle), its parameter (CC#, program, or cycle
//! index), a fixed value (for trigger/fixed), and the colour. Labels, long-press
//! actions, per-page bindings, and cycle *step* contents (keytimes) stay in the
//! webapp editor for now — this is the "no computer handy" convenience, not a
//! replacement. Changing the type rebuilds the action, carrying the
//! param/value across where they still apply.

use core::fmt::Write as _;

use crate::config::{self, Action, CcValue, OwnedButton, RuntimeConfig, PAGE_BUTTONS};
use crate::events::{DisplayCmd, EditLine, LedColor};

/// Editable action "kinds" the Type field cycles through (index = position).
const KIND_COUNT: usize = 8;
const KIND_NAMES: [&str; KIND_COUNT] = [
    "None", "CC Toggle", "CC Moment", "CC Trig", "CC Fixed", "Prog Chg", "Tap Tempo", "Cycle",
];

/// Per-switch fields, in cursor order.
const FIELD_TYPE: usize = 0;
const FIELD_PARAM: usize = 1;
const FIELD_VALUE: usize = 2;
const FIELD_COLOR: usize = 3;
const FIELD_COUNT: usize = 4;

/// Corner tag per scan-index (matches the page grid's `TAGS`).
const TAGS: [&str; PAGE_BUTTONS] = ["1", "2", "3", "4", "A", "B", "C", "D", "UP", "DN"];

/// Selectable LED colours (name + value), cycled by the Color field.
const PALETTE: [(&str, LedColor); 8] = [
    ("Off", config::color::OFF),
    ("Red", config::color::RED),
    ("Green", config::color::GREEN),
    ("Blue", config::color::BLUE),
    ("Cyan", config::color::CYAN),
    ("Amber", config::color::AMBER),
    ("Purple", config::color::PURPLE),
    ("White", config::color::WHITE),
];

/// What the router should do after an editor interaction.
pub enum EditOutcome {
    /// Repaint the editor view (the router sends [`Editor::display_cmd`]).
    Redraw,
    /// Save: persist the live config to flash + hot-reload, then leave to
    /// performance. The router owns the storage + config, so it does the work.
    Save,
}

/// Config-editor cursor state machine.
pub struct Editor {
    /// Selected switch (scan index), or `None` until one is tapped.
    sw: Option<usize>,
    /// Selected field (`FIELD_*`).
    field: usize,
    /// Whether the encoder is changing the field value (vs. moving the cursor).
    editing: bool,
    /// Whether any edit was made (so the router only writes flash if needed).
    dirty: bool,
}

impl Editor {
    pub const fn new() -> Self {
        Self {
            sw: None,
            field: 0,
            editing: false,
            dirty: false,
        }
    }

    /// Reset the cursor when (re-)entering the editor.
    pub fn enter(&mut self) {
        self.sw = None;
        self.field = 0;
        self.editing = false;
        self.dirty = false;
    }

    /// Whether any change was made this session (the router persists only then).
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Footswitch press: pick the switch to edit.
    pub fn footswitch(&mut self, idx: usize) -> EditOutcome {
        if idx < PAGE_BUTTONS {
            self.sw = Some(idx);
            self.field = FIELD_TYPE;
            self.editing = false;
        }
        EditOutcome::Redraw
    }

    /// Encoder rotation: move the field cursor, or change the field value.
    pub fn turn(&mut self, delta: i8, cfg: &mut RuntimeConfig, page: usize) -> EditOutcome {
        let Some(sw) = self.sw else {
            return EditOutcome::Redraw; // nothing selected yet
        };
        if self.editing {
            let last = cfg.pages.len().saturating_sub(1);
            if let Some(p) = cfg.pages.get_mut(page.min(last)) {
                if let Some(btn) = p.buttons.get_mut(sw) {
                    self.edit_field(delta, btn);
                }
            }
        } else {
            self.field =
                (self.field as i32 + delta as i32).clamp(0, FIELD_COUNT as i32 - 1) as usize;
        }
        EditOutcome::Redraw
    }

    /// Encoder short press: toggle editing the highlighted field.
    pub fn press(&mut self) -> EditOutcome {
        if self.sw.is_some() {
            self.editing = !self.editing;
        }
        EditOutcome::Redraw
    }

    /// Apply a rotation to the selected field of `btn`.
    fn edit_field(&mut self, delta: i8, btn: &mut OwnedButton) {
        let d = delta as i32;
        match self.field {
            FIELD_TYPE => {
                // From "(other)" (no editable kind), the first turn enters the list.
                let base = kind_index(&btn.on_press).map(|k| k as i32).unwrap_or(-1);
                let nk = (base + d).clamp(0, KIND_COUNT as i32 - 1) as usize;
                btn.on_press = build(nk, param_of(&btn.on_press), value_of(&btn.on_press));
                self.dirty = true;
            }
            FIELD_PARAM => {
                if let Some(k) = kind_index(&btn.on_press) {
                    if matches!(k, 1..=5 | 7) {
                        let max = if k == 7 {
                            config::MAX_CYCLES as i32 - 1
                        } else {
                            127
                        };
                        let np = (param_of(&btn.on_press) as i32 + d).clamp(0, max) as u8;
                        btn.on_press = build(k, np, value_of(&btn.on_press));
                        self.dirty = true;
                    }
                }
            }
            FIELD_VALUE => {
                if let Some(k) = kind_index(&btn.on_press) {
                    if matches!(k, 3 | 4) {
                        let nv = (value_of(&btn.on_press) as i32 + d).clamp(0, 127) as u8;
                        btn.on_press = build(k, param_of(&btn.on_press), nv);
                        self.dirty = true;
                    }
                }
            }
            FIELD_COLOR => {
                let ni =
                    (color_index(btn.color) as i32 + d).rem_euclid(PALETTE.len() as i32) as usize;
                btn.color = PALETTE[ni].1;
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// The display command for the current cursor + selected switch.
    pub fn display_cmd(&self, cfg: &RuntimeConfig, page: usize) -> DisplayCmd {
        let mut title = EditLine::new();
        let mut status = EditLine::new();
        match self.sw {
            None => {
                let _ = title.push_str("EDIT PAGE");
                let _ = status.push_str("Tap a switch");
            }
            Some(sw) => {
                let _ = write!(title, "EDIT {}", TAGS.get(sw).copied().unwrap_or("?"));
                let btn = &cfg.page(page).buttons[sw.min(PAGE_BUTTONS - 1)];
                self.write_field(&mut status, &btn.on_press, btn.color);
            }
        }
        DisplayCmd::Edit { title, status }
    }

    /// Format the selected field's "name: value" into `status`.
    fn write_field(&self, status: &mut EditLine, action: &Action, color: LedColor) {
        let m = if self.editing { '*' } else { '>' };
        let kind = kind_index(action);
        match self.field {
            FIELD_TYPE => {
                let name = kind.map(|k| KIND_NAMES[k]).unwrap_or("(other)");
                let _ = write!(status, "{}Type: {}", m, name);
            }
            FIELD_PARAM => match kind {
                Some(1..=4) => {
                    let _ = write!(status, "{}CC#: {}", m, param_of(action));
                }
                Some(5) => {
                    let _ = write!(status, "{}Prog: {}", m, param_of(action));
                }
                Some(7) => {
                    let _ = write!(status, "{}Cycle#: {}", m, param_of(action));
                }
                _ => {
                    let _ = write!(status, "{}Param: -", m);
                }
            },
            FIELD_VALUE => match kind {
                Some(3) | Some(4) => {
                    let _ = write!(status, "{}Value: {}", m, value_of(action));
                }
                _ => {
                    let _ = write!(status, "{}Value: -", m);
                }
            },
            FIELD_COLOR => {
                let _ = write!(status, "{}Color: {}", m, PALETTE[color_index(color)].0);
            }
            _ => {}
        }
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

/// The editable-kind index for an action, or `None` for an action outside the
/// editor's vocabulary (SysEx / page-nav / tuner / PC-step / HID) — shown as
/// "(other)" and converted to an editable kind on the first Type turn.
fn kind_index(a: &Action) -> Option<usize> {
    match a {
        Action::None => Some(0),
        Action::MidiCc { value: CcValue::Toggle, .. } => Some(1),
        Action::MidiCc { value: CcValue::Momentary, .. } => Some(2),
        Action::MidiCc { value: CcValue::Trigger(_), .. } => Some(3),
        Action::MidiCc { value: CcValue::Fixed(_), .. } => Some(4),
        Action::ProgramChange { .. } => Some(5),
        Action::TapTempo => Some(6),
        Action::Cycle(_) => Some(7),
        _ => None,
    }
}

/// The action's parameter (CC#, program, or cycle index); `0` if it has none.
fn param_of(a: &Action) -> u8 {
    match a {
        Action::MidiCc { cc, .. } => *cc,
        Action::ProgramChange { program } => *program,
        Action::Cycle(i) => *i,
        _ => 0,
    }
}

/// The action's fixed value (trigger / fixed CC); `127` default otherwise.
fn value_of(a: &Action) -> u8 {
    match a {
        Action::MidiCc { value: CcValue::Trigger(v) | CcValue::Fixed(v), .. } => *v,
        _ => 127,
    }
}

/// Build an action from an editable kind index + parameter + value.
fn build(kind: usize, param: u8, value: u8) -> Action {
    match kind {
        1 => Action::MidiCc { cc: param, value: CcValue::Toggle },
        2 => Action::MidiCc { cc: param, value: CcValue::Momentary },
        3 => Action::MidiCc { cc: param, value: CcValue::Trigger(value) },
        4 => Action::MidiCc { cc: param, value: CcValue::Fixed(value) },
        5 => Action::ProgramChange { program: param },
        6 => Action::TapTempo,
        7 => Action::Cycle(param.min(config::MAX_CYCLES as u8 - 1)),
        _ => Action::None,
    }
}

/// Palette index of a colour, or `0` (Off) if it isn't a palette entry.
fn color_index(c: LedColor) -> usize {
    PALETTE.iter().position(|(_, p)| *p == c).unwrap_or(0)
}
