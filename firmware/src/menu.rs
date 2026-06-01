//! Encoder-driven on-device settings menu (a display mode).
//!
//! Single-item-at-a-time view, ported in spirit from `remedy/lib/menu.py`.
//! The router owns a [`Menu`] and routes inputs to it while in menu mode;
//! the menu edits a borrowed [`Settings`] and returns a [`MenuOutcome`] the
//! router acts on (persist, apply live calibration, leave the menu). The
//! menu itself performs no I/O — it's a pure state machine plus reads of the
//! sampler's published raw ADC value.
//!
//! Items: MIDI channel, LED brightness, calibrate pedal 1, calibrate pedal
//! 2, exit. (Display-brightness — needing a PWM backlight — is a follow-up.)

use crate::events::{CalStep, DisplayCmd, MenuKind};
use crate::hal::expression::{self, CalPhase, Calibration, CalibrationWizard};
use crate::storage::{PedalCal, Settings};

const ITEM_MIDI: usize = 0;
const ITEM_LED: usize = 1;
const ITEM_CAL1: usize = 2;
const ITEM_CAL2: usize = 3;
const ITEM_EDIT: usize = 4;
const ITEM_EXIT: usize = 5;
const ITEM_COUNT: usize = 6;

const LABELS: [&str; ITEM_COUNT] = [
    "MIDI Channel",
    "LED Bright",
    "Cal Pedal 1",
    "Cal Pedal 2",
    "Edit Page",
    "Exit",
];

/// What the router should do after a menu interaction.
pub enum MenuOutcome {
    /// Repaint the menu (the router sends [`Menu::display_cmd`]).
    Redraw,
    /// Leave the menu (the router persists settings + restores the page).
    Exit,
    /// A calibration completed: apply it live to the sampler and persist.
    CalSaved([Calibration; expression::PEDAL_COUNT]),
    /// Enter the on-device config editor for the current page.
    EnterEdit,
}

/// Settings-menu state machine.
pub struct Menu {
    selected: usize,
    editing: bool,
    wizard: CalibrationWizard,
}

impl Menu {
    pub const fn new() -> Self {
        Self {
            selected: 0,
            editing: false,
            wizard: CalibrationWizard::new(),
        }
    }

    /// Reset to the top of the menu when (re-)entering.
    pub fn enter(&mut self) {
        self.selected = 0;
        self.editing = false;
        self.wizard.cancel();
    }

    fn calibrating(&self) -> bool {
        !matches!(self.wizard.phase(), CalPhase::Idle)
    }

    /// Encoder rotation: navigate items, or edit the selected value.
    pub fn turn(&mut self, delta: i8, s: &mut Settings) -> MenuOutcome {
        if self.calibrating() {
            return MenuOutcome::Redraw; // rotation ignored mid-calibration
        }
        if self.editing {
            let d = delta as i32;
            match self.selected {
                ITEM_MIDI => s.midi_channel = (s.midi_channel as i32 + d).clamp(1, 16) as u8,
                ITEM_LED => s.led_brightness = (s.led_brightness as i32 + d * 5).clamp(0, 100) as u8,
                _ => {}
            }
        } else {
            self.selected = (self.selected as i32 + delta as i32).clamp(0, ITEM_COUNT as i32 - 1) as usize;
        }
        MenuOutcome::Redraw
    }

    /// Encoder short press: toggle editing on value items, or run actions.
    pub fn press(&mut self) -> MenuOutcome {
        if self.calibrating() {
            return MenuOutcome::Redraw; // footswitch captures; encoder press is a no-op
        }
        match self.selected {
            ITEM_MIDI | ITEM_LED => {
                self.editing = !self.editing;
                MenuOutcome::Redraw
            }
            ITEM_CAL1 => {
                self.wizard.start(0);
                MenuOutcome::Redraw
            }
            ITEM_CAL2 => {
                self.wizard.start(1);
                MenuOutcome::Redraw
            }
            ITEM_EDIT => MenuOutcome::EnterEdit,
            ITEM_EXIT => MenuOutcome::Exit,
            _ => MenuOutcome::Redraw,
        }
    }

    /// Footswitch press: captures the current raw reading into the wizard
    /// while calibrating; ignored otherwise. On the final capture it writes
    /// the new endpoints into `s.pedal_cal` and returns [`MenuOutcome::CalSaved`].
    pub fn footswitch(&mut self, s: &mut Settings) -> MenuOutcome {
        if !self.calibrating() {
            return MenuOutcome::Redraw;
        }
        let pedal = self.wizard.pedal();
        let raw = current_raw(pedal);
        if matches!(self.wizard.advance(raw), CalPhase::Complete) {
            let result = self.wizard.result();
            self.wizard.cancel();
            if let Some((p, min, max)) = result {
                s.pedal_cal[p] = PedalCal { min, max };
                return MenuOutcome::CalSaved(calibrations(s));
            }
        }
        MenuOutcome::Redraw
    }

    /// The display command for the current menu / calibration state.
    pub fn display_cmd(&self, s: &Settings) -> DisplayCmd {
        if self.calibrating() {
            let pedal = self.wizard.pedal();
            let step = match self.wizard.phase() {
                CalPhase::AwaitMin => CalStep::Min,
                CalPhase::AwaitMax => CalStep::Max,
                _ => CalStep::Done,
            };
            return DisplayCmd::Cal {
                pedal: pedal as u8,
                step,
                raw: current_raw(pedal),
            };
        }
        let (value, kind) = match self.selected {
            ITEM_MIDI => (s.midi_channel as u16, MenuKind::Int),
            ITEM_LED => (s.led_brightness as u16, MenuKind::Percent),
            _ => (0, MenuKind::Action),
        };
        DisplayCmd::Menu {
            title: LABELS[self.selected],
            value,
            kind,
            editing: self.editing,
        }
    }
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

/// Current raw ADC reading for `pedal`, published by the expression sampler.
fn current_raw(pedal: usize) -> u16 {
    expression::LATEST_RAW
        .lock(|c| c.get())
        .get(pedal)
        .copied()
        .unwrap_or(0)
}

/// Build the `[Calibration; 2]` the sampler wants from the stored settings.
fn calibrations(s: &Settings) -> [Calibration; expression::PEDAL_COUNT] {
    core::array::from_fn(|i| Calibration {
        min: s.pedal_cal[i].min,
        max: s.pedal_cal[i].max,
    })
}
