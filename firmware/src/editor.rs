//! On-device config editor (a display mode), mirroring [`crate::menu`].
//!
//! Encoder-driven, list-based. The router owns an [`Editor`] and routes inputs
//! to it while in [`crate::app`]'s edit mode; the editor mutates the live
//! [`RuntimeConfig`]'s current page (and shared cycle pool) and returns an
//! [`EditOutcome`] the router acts on (repaint, go back a level, or save + exit).
//! It performs no I/O and holds no config of its own — just the cursor.
//!
//! ## Levels
//!
//! The editor is a small drill-down with three levels; row 0 of every level is a
//! `< Back` item that pops one level (and from the top level returns to the
//! settings menu), so there is always a reliable way back without holding:
//!
//! 1. **Pick** — land here; tap a footswitch to choose the switch to edit.
//! 2. **Fields** — that switch's short-press action TYPE (None / CC toggle /
//!    momentary / trigger / fixed / Program Change / Tap Tempo / Cycle), its
//!    parameter (CC#, program, or cycle index), a fixed value (trigger/fixed),
//!    and the LED colour. When the action is a Cycle, an `Edit Steps >` row
//!    drills into level 3.
//! 3. **Cycle (keytimes)** — the referenced [`crate::config::CycleDef`]'s shared
//!    CC#, step count, each step's value (the "keytimes"), and the long-press
//!    behaviour. On-device steps are CC keytimes (one shared CC stepped through
//!    several values); heterogeneous-CC / Program-Change / SysEx steps stay in
//!    the webapp editor and are shown read-only here.
//!
//! ## Gestures
//!
//! - **Tap a footswitch** — (re)pick the switch to edit (jumps to Fields).
//! - **Encoder turn** — move the cursor, or change the highlighted value while
//!   editing.
//! - **Encoder short-press** — on `< Back`, pop a level; on `Edit Steps >`, drill
//!   in; otherwise toggle editing the highlighted field.
//! - **Encoder hold** — save (persist + hot-reload) and return to performance
//!   from anywhere (HOME).
//!
//! Labels and long-press actions stay in the webapp editor for now — this is the
//! "no computer handy" convenience, not a replacement.

use core::fmt::Write as _;

use crate::config::{
    self, Action, CcValue, CycleDef, CycleLong, OwnedButton, RuntimeConfig, StepAction,
    MAX_CYCLES, MAX_STEPS, PAGE_BUTTONS,
};
use crate::events::{DisplayCmd, LedColor, ListLine, LIST_MAX_ROWS};

/// Editable action "kinds" the Type field cycles through (index = position).
const KIND_COUNT: usize = 8;
const KIND_NAMES: [&str; KIND_COUNT] = [
    "None", "CC Toggle", "CC Moment", "CC Trig", "CC Fixed", "Prog Chg", "Tap Tempo", "Cycle",
];

/// Fields-level row indices (row 0 is always `< Back`).
const FROW_BACK: usize = 0;
const FROW_TYPE: usize = 1;
const FROW_PARAM: usize = 2;
const FROW_VALUE: usize = 3;
const FROW_COLOR: usize = 4;
/// The `Edit Steps >` drill-in row — only present when the action is a Cycle.
const FROW_STEPS: usize = 5;
/// Fields-level row count for a non-cycle action (Back + Type/Param/Value/Color).
const FIELDS_BASE_ROWS: usize = 5;

/// Cycle-level row index of the shared CC# (row 0 is `< Back`).
const CROW_CC: usize = 1;
/// Cycle-level row index of the step count.
const CROW_STEPS: usize = 2;
/// First step row (each subsequent step follows). Long-press behaviour is the
/// final row after the steps.
const CROW_STEP0: usize = 3;
/// Cycle-level fixed row count (Back + CC# + Steps + Long), excluding step rows.
const CYCLE_FIXED_ROWS: usize = 4;

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

/// Long-press behaviour names (index = [`CycleLong`] position).
const LONG_NAMES: [&str; 3] = ["None", "Reset", "Reverse"];

/// Which drill-down level the editor cursor is on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    /// Choose the switch to edit (the landing screen).
    Pick,
    /// Edit the selected switch's action + colour fields.
    Fields,
    /// Edit the selected switch's cycle steps (keytimes).
    Cycle,
}

/// What the router should do after an editor interaction.
pub enum EditOutcome {
    /// Repaint the editor view (the router sends [`Editor::display_cmd`]).
    Redraw,
    /// Pop out of the editor back to the settings menu (persisting first if a
    /// change was made). The router owns the menu + storage, so it does the work.
    Back,
    /// Save: persist the live config to flash + hot-reload, then leave to
    /// performance (HOME). The router owns the storage + config, so it does the work.
    Save,
}

/// Config-editor cursor state machine.
pub struct Editor {
    /// Current drill-down level.
    level: Level,
    /// Selected switch (scan index); meaningful at the Fields / Cycle levels.
    sw: usize,
    /// Selected row at the current level (row 0 is always `< Back`).
    cursor: usize,
    /// Whether the encoder is changing the field value (vs. moving the cursor).
    editing: bool,
    /// Whether any edit was made (so the router only writes flash if needed).
    dirty: bool,
}

impl Editor {
    pub const fn new() -> Self {
        Self {
            level: Level::Pick,
            sw: 0,
            cursor: 0,
            editing: false,
            dirty: false,
        }
    }

    /// Reset the cursor when (re-)entering the editor.
    pub fn enter(&mut self) {
        self.level = Level::Pick;
        self.sw = 0;
        self.cursor = 0;
        self.editing = false;
        self.dirty = false;
    }

    /// Whether any change was made this session (the router persists only then).
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Footswitch press: (re)pick the switch to edit, jumping to the Fields level.
    pub fn footswitch(&mut self, idx: usize) -> EditOutcome {
        if idx < PAGE_BUTTONS {
            self.sw = idx;
            self.level = Level::Fields;
            self.cursor = FROW_TYPE;
            self.editing = false;
        }
        EditOutcome::Redraw
    }

    /// Encoder rotation: move the cursor, or change the highlighted field value.
    pub fn turn(&mut self, delta: i8, cfg: &mut RuntimeConfig, page: usize) -> EditOutcome {
        match self.level {
            Level::Pick => {
                self.cursor = clamp(self.cursor, delta, PICK_ROWS);
            }
            Level::Fields => {
                if self.editing {
                    self.edit_fields_value(delta, cfg, page);
                } else {
                    self.cursor = clamp(self.cursor, delta, self.fields_row_count(cfg, page));
                }
            }
            Level::Cycle => {
                if self.editing {
                    self.edit_cycle_value(delta, cfg, page);
                } else {
                    self.cursor = clamp(self.cursor, delta, self.cycle_row_count(cfg, page));
                }
            }
        }
        EditOutcome::Redraw
    }

    /// Encoder short press: pop on `< Back`, drill in on `Edit Steps >`, else
    /// toggle editing the highlighted field.
    pub fn press(&mut self, cfg: &mut RuntimeConfig, page: usize) -> EditOutcome {
        match self.level {
            Level::Pick => {
                if self.cursor == FROW_BACK {
                    return EditOutcome::Back; // leave the editor → settings menu
                }
                EditOutcome::Redraw // the info row isn't actionable; tap a switch
            }
            Level::Fields => {
                if self.cursor == FROW_BACK {
                    self.level = Level::Pick;
                    self.cursor = FROW_BACK;
                    self.editing = false;
                    return EditOutcome::Redraw;
                }
                let is_cycle = matches!(self.button(cfg, page).on_press, Action::Cycle(_));
                if is_cycle && self.cursor == FROW_STEPS {
                    self.enter_cycle(cfg, page);
                    return EditOutcome::Redraw;
                }
                self.editing = !self.editing;
                EditOutcome::Redraw
            }
            Level::Cycle => {
                if self.cursor == FROW_BACK {
                    self.level = Level::Fields;
                    // Land back on the drill-in row we came from.
                    self.cursor = FROW_STEPS;
                    self.editing = false;
                    return EditOutcome::Redraw;
                }
                self.editing = !self.editing;
                EditOutcome::Redraw
            }
        }
    }

    /// Borrow the currently-selected button (clamped so it can't panic).
    fn button<'a>(&self, cfg: &'a RuntimeConfig, page: usize) -> &'a OwnedButton {
        &cfg.page(page).buttons[self.sw.min(PAGE_BUTTONS - 1)]
    }

    /// Number of rows at the Fields level (one extra when the action is a Cycle).
    fn fields_row_count(&self, cfg: &RuntimeConfig, page: usize) -> usize {
        FIELDS_BASE_ROWS + matches!(self.button(cfg, page).on_press, Action::Cycle(_)) as usize
    }

    /// Number of rows at the Cycle level for the referenced cycle.
    fn cycle_row_count(&self, cfg: &RuntimeConfig, page: usize) -> usize {
        match self.cycle_index(cfg, page).and_then(|i| cfg.cycles.get(i)) {
            Some(def) => CYCLE_FIXED_ROWS + def.steps.len(),
            None => 2, // Back + "(no cycle)"
        }
    }

    /// The cycle pool index the selected switch references, if it's a Cycle.
    fn cycle_index(&self, cfg: &RuntimeConfig, page: usize) -> Option<usize> {
        match self.button(cfg, page).on_press {
            Action::Cycle(i) => Some(i as usize),
            _ => None,
        }
    }

    /// Drill into the Cycle level, allocating a fresh [`CycleDef`] if the switch
    /// references the next-free pool slot (so a just-made cycle button is editable).
    fn enter_cycle(&mut self, cfg: &mut RuntimeConfig, page: usize) {
        let Some(cyc) = self.cycle_index(cfg, page) else {
            return;
        };
        if cyc == cfg.cycles.len() && cfg.cycles.len() < MAX_CYCLES {
            let _ = cfg.cycles.push(CycleDef {
                steps: default_steps(),
                long: CycleLong::None,
            });
            self.dirty = true;
        }
        if cyc < cfg.cycles.len() {
            self.level = Level::Cycle;
            self.cursor = CROW_CC;
            self.editing = false;
        }
        // A malformed (out-of-range) reference just stays on Fields — a no-op.
    }

    /// Apply a rotation to the selected Fields-level value.
    fn edit_fields_value(&mut self, delta: i8, cfg: &mut RuntimeConfig, page: usize) {
        // Cycle-aware bounds, read before the mutable button borrow.
        let cyc_max = cfg.cycles.len().min(MAX_CYCLES - 1) as u8;
        let last = cfg.pages.len().saturating_sub(1);
        let Some(p) = cfg.pages.get_mut(page.min(last)) else {
            return;
        };
        let Some(btn) = p.buttons.get_mut(self.sw.min(PAGE_BUTTONS - 1)) else {
            return;
        };
        let d = delta as i32;
        match self.cursor {
            FROW_TYPE => {
                // From "(other)" (no editable kind), the first turn enters the list.
                let was_cycle = matches!(btn.on_press, Action::Cycle(_));
                let base = kind_index(&btn.on_press).map(|k| k as i32).unwrap_or(-1);
                let nk = (base + d).clamp(0, KIND_COUNT as i32 - 1) as usize;
                let mut np = param_of(&btn.on_press);
                // Newly a cycle → point at the next-free pool slot (created on drill-in).
                if nk == 7 && !was_cycle {
                    np = cyc_max;
                }
                btn.on_press = build(nk, np, value_of(&btn.on_press));
                self.dirty = true;
            }
            FROW_PARAM => {
                if let Some(k) = kind_index(&btn.on_press) {
                    if matches!(k, 1..=5 | 7) {
                        let max = if k == 7 { cyc_max as i32 } else { 127 };
                        let np = (param_of(&btn.on_press) as i32 + d).clamp(0, max) as u8;
                        btn.on_press = build(k, np, value_of(&btn.on_press));
                        self.dirty = true;
                    }
                }
            }
            FROW_VALUE => {
                if let Some(k) = kind_index(&btn.on_press) {
                    if matches!(k, 3 | 4) {
                        let nv = (value_of(&btn.on_press) as i32 + d).clamp(0, 127) as u8;
                        btn.on_press = build(k, param_of(&btn.on_press), nv);
                        self.dirty = true;
                    }
                }
            }
            FROW_COLOR => {
                let ni =
                    (color_index(btn.color) as i32 + d).rem_euclid(PALETTE.len() as i32) as usize;
                btn.color = PALETTE[ni].1;
                self.dirty = true;
            }
            _ => {} // Back / Steps rows aren't value-editable
        }
    }

    /// Apply a rotation to the selected Cycle-level value.
    fn edit_cycle_value(&mut self, delta: i8, cfg: &mut RuntimeConfig, page: usize) {
        let Some(cyc) = self.cycle_index(cfg, page) else {
            return;
        };
        let Some(def) = cfg.cycles.get_mut(cyc) else {
            return;
        };
        let d = delta as i32;
        let n = def.steps.len();
        match self.cursor {
            CROW_CC => {
                // One shared CC for the whole cycle (the keytimes idiom).
                let ncc = (shared_cc(def) as i32 + d).clamp(0, 127) as u8;
                for s in def.steps.iter_mut() {
                    if let StepAction::MidiCc { cc, .. } = s {
                        *cc = ncc;
                    }
                }
                self.dirty = true;
            }
            CROW_STEPS => {
                let nn = (n as i32 + d).clamp(1, MAX_STEPS as i32) as usize;
                if nn > n {
                    let cc = shared_cc(def);
                    for _ in n..nn {
                        let _ = def.steps.push(StepAction::MidiCc { cc, value: 0 });
                    }
                    self.dirty = true;
                } else if nn < n {
                    def.steps.truncate(nn);
                    self.dirty = true;
                }
            }
            c if (CROW_STEP0..CROW_STEP0 + n).contains(&c) => {
                let k = c - CROW_STEP0;
                if let Some(StepAction::MidiCc { value, .. }) = def.steps.get_mut(k) {
                    *value = (*value as i32 + d).clamp(0, 127) as u8;
                    self.dirty = true;
                }
            }
            c if c == CROW_STEP0 + n => {
                def.long = step_long(def.long, d);
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// The display command for the current level + cursor: a title plus one row
    /// per item (with `< Back` as row 0), the cursor on [`Self::cursor`].
    pub fn display_cmd(&self, cfg: &RuntimeConfig, page: usize) -> DisplayCmd {
        let mut title = ListLine::new();
        let mut rows: heapless::Vec<ListLine, LIST_MAX_ROWS> = heapless::Vec::new();
        match self.level {
            Level::Pick => {
                let _ = title.push_str("EDIT PAGE");
                push_text(&mut rows, "< Back");
                push_text(&mut rows, "Tap a switch to edit");
            }
            Level::Fields => {
                let tag = TAGS.get(self.sw).copied().unwrap_or("?");
                let _ = write!(title, "EDIT {}", tag);
                let btn = self.button(cfg, page);
                push_text(&mut rows, "< Back");
                let _ = rows.push(fields_row(FROW_TYPE, &btn.on_press, btn.color));
                let _ = rows.push(fields_row(FROW_PARAM, &btn.on_press, btn.color));
                let _ = rows.push(fields_row(FROW_VALUE, &btn.on_press, btn.color));
                let _ = rows.push(fields_row(FROW_COLOR, &btn.on_press, btn.color));
                if matches!(btn.on_press, Action::Cycle(_)) {
                    push_text(&mut rows, "Edit Steps >");
                }
            }
            Level::Cycle => {
                let tag = TAGS.get(self.sw).copied().unwrap_or("?");
                let _ = write!(title, "STEPS: {}", tag);
                push_text(&mut rows, "< Back");
                match self.cycle_index(cfg, page).and_then(|i| cfg.cycles.get(i)) {
                    Some(def) => {
                        let mut r = ListLine::new();
                        let _ = write!(r, "CC#: {}", shared_cc(def));
                        let _ = rows.push(r);
                        let mut r = ListLine::new();
                        let _ = write!(r, "Steps: {}", def.steps.len());
                        let _ = rows.push(r);
                        for (k, s) in def.steps.iter().enumerate() {
                            let mut r = ListLine::new();
                            match s {
                                StepAction::MidiCc { value, .. } => {
                                    let _ = write!(r, "Step {}: {}", k + 1, value);
                                }
                                StepAction::ProgramChange { program } => {
                                    let _ = write!(r, "Step {}: PC {}", k + 1, program);
                                }
                                StepAction::Sysex(_) => {
                                    let _ = write!(r, "Step {}: sysex", k + 1);
                                }
                            }
                            let _ = rows.push(r);
                        }
                        let mut r = ListLine::new();
                        let _ = write!(r, "Long: {}", long_name(def.long));
                        let _ = rows.push(r);
                    }
                    None => push_text(&mut rows, "(no cycle)"),
                }
            }
        }
        DisplayCmd::List {
            title,
            rows,
            selected: self.cursor as u8,
            editing: self.editing,
        }
    }
}

/// Number of rows at the Pick level (`< Back` + the tap-a-switch instruction).
const PICK_ROWS: usize = 2;

/// Clamp `cursor + delta` into `0..count`.
fn clamp(cursor: usize, delta: i8, count: usize) -> usize {
    (cursor as i32 + delta as i32).clamp(0, count as i32 - 1) as usize
}

/// Push a plain-text row onto the snapshot (best-effort; over-cap rows drop).
fn push_text(rows: &mut heapless::Vec<ListLine, LIST_MAX_ROWS>, s: &str) {
    let mut row = ListLine::new();
    let _ = row.push_str(s);
    let _ = rows.push(row);
}

/// A fresh single-step cycle (CC 0 → value 0), the default for a new cycle.
fn default_steps() -> heapless::Vec<StepAction, MAX_STEPS> {
    let mut v = heapless::Vec::new();
    let _ = v.push(StepAction::MidiCc { cc: 0, value: 0 });
    v
}

/// Format Fields-level row `frow`'s "name: value" line (no cursor marker — the
/// list view highlights the selected row).
fn fields_row(frow: usize, action: &Action, color: LedColor) -> ListLine {
    let mut row = ListLine::new();
    let kind = kind_index(action);
    match frow {
        FROW_TYPE => {
            let name = kind.map(|k| KIND_NAMES[k]).unwrap_or("(other)");
            let _ = write!(row, "Type: {}", name);
        }
        FROW_PARAM => match kind {
            Some(1..=4) => {
                let _ = write!(row, "CC#: {}", param_of(action));
            }
            Some(5) => {
                let _ = write!(row, "Prog: {}", param_of(action));
            }
            Some(7) => {
                let _ = write!(row, "Cycle#: {}", param_of(action));
            }
            _ => {
                let _ = row.push_str("Param: -");
            }
        },
        FROW_VALUE => match kind {
            Some(3) | Some(4) => {
                let _ = write!(row, "Value: {}", value_of(action));
            }
            _ => {
                let _ = row.push_str("Value: -");
            }
        },
        FROW_COLOR => {
            let _ = write!(row, "Color: {}", PALETTE[color_index(color)].0);
        }
        _ => {}
    }
    row
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
        7 => Action::Cycle(param.min(MAX_CYCLES as u8 - 1)),
        _ => Action::None,
    }
}

/// Palette index of a colour, or `0` (Off) if it isn't a palette entry.
fn color_index(c: LedColor) -> usize {
    PALETTE.iter().position(|(_, p)| *p == c).unwrap_or(0)
}

/// The shared CC of a cycle (first CC step's number), or `0` if none is a CC.
fn shared_cc(def: &CycleDef) -> u8 {
    for s in &def.steps {
        if let StepAction::MidiCc { cc, .. } = s {
            return *cc;
        }
    }
    0
}

/// Step the long-press behaviour by `delta` (wrapping through the three options).
fn step_long(cur: CycleLong, delta: i32) -> CycleLong {
    let order = [CycleLong::None, CycleLong::Reset, CycleLong::Reverse];
    let i = order.iter().position(|x| *x == cur).unwrap_or(0) as i32;
    order[(i + delta).rem_euclid(order.len() as i32) as usize]
}

/// Display name for a [`CycleLong`].
fn long_name(l: CycleLong) -> &'static str {
    match l {
        CycleLong::None => LONG_NAMES[0],
        CycleLong::Reset => LONG_NAMES[1],
        CycleLong::Reverse => LONG_NAMES[2],
    }
}
