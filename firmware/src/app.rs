//! Application event router + state machine.
//!
//! Extracted from `bin/midicaptain.rs` once enough subsystems were wired
//! that the bin was mostly router logic. The bin is now thin wiring
//! (construct peripherals, spawn tasks); this module owns *what the device
//! does* with the events those tasks produce.
//!
//! [`Router`] holds the application state — active display [`Mode`], page,
//! per-CC toggle state, the live [`Settings`] (and the [`Storage`] to persist
//! them), and the settings [`Menu`]. In [`Mode::Performance`] it dispatches
//! the active [`crate::config`] page's actions (CC / PC / SysEx / page-nav)
//! with LED + display feedback and bidirectional CC sync; in [`Mode::Menu`]
//! it routes the encoder + footswitches to the settings menu.
//!
//! It is the **single owner** of that state; the rest of the system
//! influences it only by sending events to [`router_task`].

use defmt::warn;
use embassy_futures::select::{select, select4, Either, Either4};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Instant, Timer};

use crate::config::{self, Action, CcValue, CycleLong, RuntimeConfig, StepAction, SysexCmd};
use crate::events::{
    ButtonEvent, Cell, CellState, DisplayCmd, EncoderEvent, ExprEvent, LedColor, LedFrame, MidiCmd,
    MidiRx,
};
use crate::editor::{EditOutcome, Editor};
use crate::hal::{buttons, encoder, expression, hid, leds};
use crate::menu::{Menu, MenuOutcome};
use crate::midi::{katana, mux, sysex};
use crate::pins;
use crate::proto;
use crate::storage::{Settings, Storage};
use crate::tuner::TunerState;

/// Press duration at/above which a release counts as a long-press. Used both
/// for footswitches (short vs long action) and the encoder push (enter/exit
/// the settings menu).
const LONG_PRESS: Duration = Duration::from_millis(500);

/// CC that toggles the connected device's tuner. CC#25 = 127 enters, 0 exits
/// (matches `remedy/lib/tuner.py`'s default `toggle_cc`). The amp then streams
/// Note On + Pitch Bend back, which drives the [`TunerState`].
const TUNER_CC: u8 = 25;

// ── Display channel (router → display task) ────────────────────────────
// Owned here because the router is the producer; the bin's display task
// (sole owner of the ST7789) holds the receiver.

/// Depth of the [`DisplayCmd`] channel. The router coalesces renders, so 8
/// is ample.
pub const DISPLAY_QUEUE_DEPTH: usize = 8;
/// Bounded channel carrying [`DisplayCmd`]s to the display task.
pub type DisplayChannel = Channel<CriticalSectionRawMutex, DisplayCmd, DISPLAY_QUEUE_DEPTH>;
/// Sender half — held by the [`Router`].
pub type DisplaySender = Sender<'static, CriticalSectionRawMutex, DisplayCmd, DISPLAY_QUEUE_DEPTH>;
/// Receiver half — held by the display task.
pub type DisplayReceiver = Receiver<'static, CriticalSectionRawMutex, DisplayCmd, DISPLAY_QUEUE_DEPTH>;

// ── Config-sync channels (cdc_task ↔ router) ───────────────────────────
// The CDC task (in the bin) owns the wire codec; the router owns the live
// config + flash store. A GET/SET frame becomes a [`ConfigReq`] sent to the
// router, which answers with a [`ConfigResp`]. It's a strict request/response
// from a single client, so depth 1 suffices and no reply can be misattributed.

/// A config-sync request from the CDC task to the router.
//
// `Set` carries a whole `RuntimeConfig` (~2.4 KB) while `Get` is empty — a
// large variant spread. With no allocator (`no_std`) there's nothing to box
// into, and the config must move by value through the channel, so the spread is
// intrinsic; the channel is depth 1, so only one lives in `.bss` at a time.
#[allow(clippy::large_enum_variant)]
pub enum ConfigReq {
    /// Read the live config — the router answers [`ConfigResp::Config`].
    Get,
    /// Replace the live config with a validated one (persist + hot-reload).
    /// The CDC task has already deserialized and checked it is non-empty.
    Set(RuntimeConfig),
}

/// The router's answer to a [`ConfigReq`].
#[allow(clippy::large_enum_variant)] // see [`ConfigReq`]: no allocator to box into
pub enum ConfigResp {
    /// The live config, answering [`ConfigReq::Get`].
    Config(RuntimeConfig),
    /// A [`ConfigReq::Set`] succeeded (persisted + applied live).
    Ok,
    /// A [`ConfigReq::Set`] failed; carries the wire error code to relay.
    Err(proto::ProtoError),
}

/// Depth of the config-sync channels. Request/response with one client → 1.
pub const CONFIG_QUEUE_DEPTH: usize = 1;
/// Channel carrying a [`ConfigReq`] from the CDC task to the router.
pub type ConfigReqChannel = Channel<CriticalSectionRawMutex, ConfigReq, CONFIG_QUEUE_DEPTH>;
/// Sender half — held by the CDC task.
pub type ConfigReqSender = Sender<'static, CriticalSectionRawMutex, ConfigReq, CONFIG_QUEUE_DEPTH>;
/// Receiver half — held by the router.
pub type ConfigReqReceiver =
    Receiver<'static, CriticalSectionRawMutex, ConfigReq, CONFIG_QUEUE_DEPTH>;
/// Channel carrying a [`ConfigResp`] from the router back to the CDC task.
pub type ConfigRespChannel = Channel<CriticalSectionRawMutex, ConfigResp, CONFIG_QUEUE_DEPTH>;
/// Sender half — held by the router.
pub type ConfigRespSender =
    Sender<'static, CriticalSectionRawMutex, ConfigResp, CONFIG_QUEUE_DEPTH>;
/// Receiver half — held by the CDC task.
pub type ConfigRespReceiver =
    Receiver<'static, CriticalSectionRawMutex, ConfigResp, CONFIG_QUEUE_DEPTH>;

/// Active display mode — what the screen shows and how inputs route.
enum Mode {
    /// Live performance: footswitches dispatch page actions; encoder/expr → CC.
    Performance,
    /// Settings menu: encoder navigates/edits; footswitches drive calibration.
    Menu,
    /// Chromatic tuner: the screen shows the amp's pitch readout; inbound
    /// Note/Pitch-Bend update it. Any footswitch release — or an encoder
    /// hold — leaves back to performance.
    Tuner,
    /// On-device config editor: a footswitch picks the switch to edit; the
    /// encoder navigates/changes its fields; an encoder hold saves + exits.
    Edit,
}

/// Direction a cycle button's press moves through its states.
enum CycleDir {
    /// Short press: next state (wraps).
    Advance,
    /// Long press with [`CycleLong::Reverse`]: previous state (wraps).
    Reverse,
    /// Long press with [`CycleLong::Reset`]: back to the first state.
    Reset,
}

/// The event router + application state.
pub struct Router {
    config: RuntimeConfig,
    page: usize,
    /// Live, editable settings (the menu mutates these; persisted on exit).
    settings: Settings,
    /// Flash store — owned here so the menu can persist.
    storage: Storage,
    /// Scratch for persisting a pushed config (`Storage::store_config` needs
    /// `≥ CONFIG_SCRATCH_LEN`). Reuses the buffer the bin allocates for the
    /// boot-time config load, rather than carrying a second large array.
    config_scratch: &'static mut [u8],
    mode: Mode,
    menu: Menu,
    /// On-device config editor cursor (active in [`Mode::Edit`]).
    editor: Editor,
    /// Tuner readout (driven by inbound MIDI while in [`Mode::Tuner`]).
    tuner: TunerState,
    /// Accumulated encoder-driven value (`0..=127`), emitted per the active
    /// page's [`config::ContinuousBinding`] encoder binding. A single relative
    /// accumulator shared across pages (not reset on page change).
    enc_value: u8,
    /// Latest level (`0..=127`) of each continuous control for the on-screen
    /// meters: `[EXP1, EXP2, encoder]`. Pushed to the display on change (see
    /// [`Self::send_meters`]); `[2]` mirrors [`Self::enc_value`].
    meter_values: [u8; 3],
    /// Current program number, for [`Action::ProgramChangeStep`] (inc/dec).
    /// Set by any absolute [`Action::ProgramChange`]; stepped by inc/dec.
    current_program: u8,
    /// Per-CC toggle state (on/off), indexed by CC number. Cleared on page
    /// change; synced from incoming MIDI CC.
    toggles: [bool; 128],
    /// Per-page radio/select group selection: for each group (`index =
    /// group_id - 1`), the scan-index of the currently-selected member, or
    /// `None`. Mutated by a short press on a grouped button; cleared on page
    /// change + config apply (like [`Self::toggles`]). Local-only — not driven
    /// by inbound MIDI.
    group_sel: [Option<u8>; config::MAX_GROUPS],
    /// Device-confirmed effect-switch state, keyed by CC number: the on/off the
    /// connected Katana last reported for a block (an inbound block DT1 bridged
    /// to its `cc_alias` — see [`Self::on_sysex_rx`]). Unlike the per-page
    /// [`Self::toggles`] (local press intent, cleared on page change), this
    /// persists across page changes and is re-applied to `toggles` on page entry
    /// ([`Self::reapply_device_state`]) so a device-backed toggle keeps showing
    /// the amp's real state. `None` = the amp hasn't reported that CC. A bare CC
    /// echo (`sync_cc`) is *not* cached here — only amp-confirmed block state is.
    dev_toggles: [Option<bool>; 128],
    /// Last amp type / preset the device reported (the radio-group params swept
    /// on boot + broadcast in editor mode). Cached like [`Self::dev_toggles`] so
    /// the amp-type / channel radios survive a page change — re-applied via
    /// [`Self::reflect_sysex`] on page entry. `None` = not yet reported.
    dev_amp_type: Option<u8>,
    dev_preset: Option<u8>,
    /// Per-page cycle position, indexed by button scan-index: the state a
    /// [`Action::Cycle`] button last landed on, or `None` (not yet pressed).
    /// Cleared on page change + config apply, like [`Self::toggles`]. Local —
    /// not driven by inbound MIDI.
    cycle_active: [Option<u8>; config::PAGE_BUTTONS],
    /// Press timestamps for footswitch long-press detection, per switch index.
    press_at: [Option<Instant>; buttons::COUNT],
    /// For [`CcValue::Momentary`]: the CC a held switch is driving (sent `127`
    /// on press), so its release can send `0`. `None` = not momentary-held.
    momentary_active: [Option<u8>; buttons::COUNT],
    /// Encoder push timestamp, for its long-press (enter/exit the menu).
    enc_press_at: Option<Instant>,
    /// Timestamp of the previous tap-tempo tap, for measuring the interval.
    /// `None` until the first tap (or after a too-long gap resets the count).
    tap_last: Option<Instant>,
    display: DisplaySender,
    leds: leds::LedSender,
    midi_cmd: mux::MidiCmdSender,
    sysex_out: mux::SysExSender,
    /// Outbound USB-HID reports (keyboard / consumer control) → the HID task.
    hid: hid::HidSender,
    /// Reply channel for config-sync requests, back to the CDC task. Owned here
    /// (like the other output senders) so [`Self::handle_config_req`] answers in
    /// place rather than the task loop juggling the response.
    config_resp: ConfigRespSender,
}

impl Router {
    /// Build a router bound to its output channels, the loaded settings, and
    /// the flash store (for menu persistence).
    #[allow(clippy::too_many_arguments)] // wiring constructor: state + four output channels
    pub fn new(
        config: RuntimeConfig,
        settings: Settings,
        storage: Storage,
        config_scratch: &'static mut [u8],
        display: DisplaySender,
        leds: leds::LedSender,
        midi_cmd: mux::MidiCmdSender,
        sysex_out: mux::SysExSender,
        hid: hid::HidSender,
        config_resp: ConfigRespSender,
    ) -> Self {
        Self {
            config,
            page: 0,
            settings,
            storage,
            config_scratch,
            mode: Mode::Performance,
            menu: Menu::new(),
            editor: Editor::new(),
            tuner: TunerState::new(),
            enc_value: 0,
            meter_values: [0; 3],
            current_program: 0,
            toggles: [false; 128],
            group_sel: [None; config::MAX_GROUPS],
            dev_toggles: [None; 128],
            dev_amp_type: None,
            dev_preset: None,
            cycle_active: [None; config::PAGE_BUTTONS],
            press_at: [None; buttons::COUNT],
            momentary_active: [None; buttons::COUNT],
            enc_press_at: None,
            tap_last: None,
            display,
            leds,
            midi_cmd,
            sysex_out,
            hid,
            config_resp,
        }
    }

    /// 0-based wire channel from the 1-based MIDI-channel setting.
    fn wire_channel(&self) -> u8 {
        self.settings.midi_channel.saturating_sub(1) & 0x0F
    }

    fn current_page(&self) -> &config::OwnedPage {
        self.config.page(self.page)
    }

    /// Build the LED frame for the active page + toggle state and send it.
    /// Toggle buttons: full colour when on, `idle_dim` when off. Non-toggle
    /// bound buttons: full colour. Unbound slots: off. Everything is then
    /// scaled by the LED-brightness setting.
    fn refresh_leds(&self) {
        let page = self.current_page();
        let bright = self.settings.led_brightness;
        let mut frame = LedFrame {
            switches: [config::color::OFF; pins::Switch::COUNT],
        };
        for (bi, btn) in page.buttons.iter().enumerate() {
            let base = if let Action::Cycle(_) = btn.on_press {
                // Cycle button: full on any non-base state, dim on the base state
                // (index 0) or before the first press. Highest LED precedence.
                if matches!(self.cycle_active[bi], Some(p) if p != 0) {
                    btn.color
                } else {
                    leds::idle_dim(btn.color)
                }
            } else if let Some(slot) = group_slot(btn.group) {
                // Radio/select group: the selected member is full, the rest dim.
                // Takes precedence over toggle feedback.
                if self.group_sel[slot] == Some(bi as u8) {
                    btn.color
                } else {
                    leds::idle_dim(btn.color)
                }
            } else {
                match btn.on_press.toggle_cc() {
                    Some(cc) => {
                        if self.toggles[cc as usize] {
                            btn.color
                        } else {
                            leds::idle_dim(btn.color)
                        }
                    }
                    None if matches!(btn.on_press, Action::None) => config::color::OFF,
                    None => btn.color,
                }
            };
            // Scan-index → WS2812 chain position.
            frame.switches[config::SWITCH_FOR_BUTTON[bi] as usize] = scale(base, bright);
        }
        let _ = self.leds.try_send(frame);
    }

    /// Presentation state for one button cell, mirroring the LED feedback
    /// precedence in [`Self::refresh_leds`] (cycle > group > toggle). Display-
    /// only: the `PageGrid` widget renders it. Kept as a separate read (rather
    /// than refactoring `refresh_leds` to share it) so the validated LED
    /// behaviour is untouched — the two are deliberately kept in lockstep here.
    fn cell_state(&self, bi: usize) -> CellState {
        let btn = &self.current_page().buttons[bi];
        // Held momentary takes visual priority (it's the live edge).
        if self.momentary_active[bi].is_some() {
            return CellState::Momentary(true);
        }
        if let Action::Cycle(ci) = btn.on_press {
            // A valid, non-empty cycle shows "pos/len" (pos 1-based; 0 = unset).
            if let Some(len) = self.config.cycle(ci).map(|c| c.steps.len()).filter(|&l| l > 0) {
                let pos = self.cycle_active[bi].map_or(0, |p| p + 1);
                return CellState::Cycle { pos, len: len as u8 };
            }
            // Bad/empty cycle index → inert, drawn as a plain bound button.
            return CellState::Plain;
        }
        if let Some(slot) = group_slot(btn.group) {
            return CellState::Radio(self.group_sel[slot] == Some(bi as u8));
        }
        if let Some(cc) = btn.on_press.toggle_cc() {
            return CellState::Toggle(self.toggles[cc as usize]);
        }
        if matches!(btn.on_press, Action::MidiCc { value: CcValue::Momentary, .. }) {
            return CellState::Momentary(false);
        }
        if matches!(btn.on_press, Action::None) && matches!(btn.on_long_press, Action::None) {
            return CellState::Empty;
        }
        CellState::Plain
    }

    /// Send the performance-screen snapshot (one [`Cell`] per footswitch) to the
    /// display. No-op outside [`Mode::Performance`] so a background CC-sync /
    /// LED refresh can't clobber the menu or tuner screen.
    fn refresh_view(&self) {
        if !matches!(self.mode, Mode::Performance) {
            return;
        }
        let page = self.current_page();
        let cells: [Cell; config::PAGE_BUTTONS] = core::array::from_fn(|bi| Cell {
            label: page.buttons[bi].label.clone(),
            color: page.buttons[bi].color,
            state: self.cell_state(bi),
        });
        let _ = self.display.try_send(DisplayCmd::Page {
            name: page.name.clone(),
            index: self.page as u8 + 1,
            total: self.config.page_count() as u8,
            program: self.current_program,
            cells,
        });
    }

    /// Repaint everything the live state drives: the LED frame and (in
    /// performance mode) the page-grid snapshot. The single call to make after
    /// any state change.
    fn refresh(&self) {
        self.refresh_leds();
        self.refresh_view();
    }

    /// Paint the active page (grid + LEDs). Used on every transition into
    /// performance mode and on the initial boot paint.
    fn refresh_page(&self) {
        self.refresh();
    }

    /// Switch pages, clearing per-page toggle + group state (CLAUDE.md) and
    /// repainting.
    fn change_page(&mut self, page: usize) {
        self.page = page.min(self.config.page_count().saturating_sub(1));
        self.toggles = [false; 128];
        self.group_sel = [None; config::MAX_GROUPS];
        self.cycle_active = [None; config::PAGE_BUTTONS];
        self.reapply_device_state();
        self.refresh_page();
    }

    /// Re-apply the cached device-confirmed state after the per-page toggle /
    /// group clear (in [`Self::change_page`] / a config apply), so device-backed
    /// buttons — Katana effect switches, amp type, preset — keep showing the
    /// amp's real state on the new page instead of resetting to off. The cache is
    /// fed by inbound device feedback (the boot RQ1 sweep + live editor-mode
    /// broadcasts) in [`Self::on_sysex_rx`]. Purely-local CC toggles aren't
    /// cached, so they still clear on a page change (their cross-page meaning is
    /// undefined — the reason the clear exists).
    fn reapply_device_state(&mut self) {
        for cc in 0..self.dev_toggles.len() {
            if let Some(on) = self.dev_toggles[cc] {
                self.toggles[cc] = on;
            }
        }
        if let Some(t) = self.dev_amp_type {
            self.reflect_sysex(SysexCmd::AmpType(t));
        }
        if let Some(p) = self.dev_preset {
            self.reflect_sysex(SysexCmd::RecallPreset(p));
        }
    }

    // ── input handlers (mode-routed) ───────────────────────────────────

    async fn on_button(&mut self, ev: ButtonEvent) {
        match self.mode {
            Mode::Performance => self.on_button_perf(ev),
            // In the menu a footswitch press captures a calibration endpoint.
            Mode::Menu => {
                if ev.pressed {
                    let outcome = self.menu.footswitch(&mut self.settings);
                    self.apply_menu_outcome(outcome).await;
                }
            }
            // In the tuner any footswitch leaves. Act on release (not press)
            // so the same switch that *entered* via long-press can exit on its
            // next tap without that tap's release also firing a page action.
            Mode::Tuner => {
                if !ev.pressed {
                    self.exit_tuner();
                }
            }
            // In the editor a footswitch press picks the switch being edited.
            Mode::Edit => {
                if ev.pressed {
                    let outcome = self.editor.footswitch(ev.index as usize);
                    self.apply_edit_outcome(outcome).await;
                }
            }
        }
    }

    /// Performance-mode footswitch handling: long/short-press action dispatch.
    fn on_button_perf(&mut self, ev: ButtonEvent) {
        let idx = ev.index as usize;
        if idx >= buttons::COUNT {
            return; // defensive: ignore out-of-range indices
        }
        if ev.pressed {
            // A momentary CC fires on the press edge (127); its release (below)
            // sends 0. Everything else waits for release to tell long from short.
            if let Action::MidiCc { cc, value: CcValue::Momentary } =
                self.current_page().buttons[idx].on_press
            {
                // Mark held *before* refreshing so the cell shows `Momentary(true)`.
                self.momentary_active[idx] = Some(cc);
                self.send_cc(cc, 127);
                self.refresh();
            }
            self.press_at[idx] = Some(Instant::now());
            return;
        }
        // Released. A held momentary completes here (send 0) and short-circuits
        // the long/short dispatch entirely.
        if let Some(cc) = self.momentary_active[idx].take() {
            self.press_at[idx] = None;
            self.send_cc(cc, 0);
            self.refresh(); // cell reverts to idle now that it's released
            return;
        }
        // Otherwise: pick long- vs short-press by held duration. Dispatching
        // on release is what lets a single switch carry both actions.
        let long = self.press_at[idx]
            .take()
            .map(|t| Instant::now().saturating_duration_since(t) >= LONG_PRESS)
            .unwrap_or(false);
        // Read the button's bindings + clone its label out of the
        // (immutably-borrowed) config *before* any `&mut self` call — the owned
        // label means no config borrow is held across the mutable dispatch.
        let (on_press, on_long_press, label, group) = {
            let btn = &self.current_page().buttons[idx];
            (btn.on_press, btn.on_long_press, btn.label.clone(), btn.group)
        };

        // Cycle buttons are special: the short press advances the cycle; the
        // long press follows the cycle's `CycleLong` (Reset/Reverse), or — when
        // the cycle doesn't claim it (`None`, or a bad index) — falls through to
        // the button's own `on_long_press`.
        if let Action::Cycle(ci) = on_press {
            if !long {
                self.cycle_step(idx, ci, CycleDir::Advance);
            } else {
                match self.config.cycle(ci).map(|c| c.long) {
                    Some(CycleLong::Reset) => self.cycle_step(idx, ci, CycleDir::Reset),
                    Some(CycleLong::Reverse) => self.cycle_step(idx, ci, CycleDir::Reverse),
                    _ if on_long_press != Action::None => self.act(idx, on_long_press, label),
                    _ => {}
                }
            }
            return;
        }

        // Non-cycle: pick long- vs short-press action (dispatching on release is
        // what lets a single switch carry both).
        let action = if long && on_long_press != Action::None {
            on_long_press
        } else {
            on_press
        };
        // A short press on a grouped button makes it that group's selection
        // (radio behaviour), deselecting the previous member. Recorded before
        // dispatch so the repaint (in `act`) reflects the new selection no matter
        // what the action does. Long-presses don't latch — they fire
        // `on_long_press` (e.g. tuner / page nav) without changing the group.
        if !long {
            if let Some(slot) = group_slot(group) {
                self.group_sel[slot] = Some(idx as u8);
            }
        }
        self.act(idx, action, label);
    }

    /// Dispatch a button action, then give feedback: always repaint (LEDs +
    /// grid header/state), and for a non-latching action (Program Change /
    /// SysEx / HID / PC-step / fixed CC) additionally flash the pressed cell —
    /// it has no persistent [`CellState`] of its own. `idx` is a validated
    /// switch index.
    fn act(&mut self, idx: usize, action: Action, label: config::Label) {
        let transient = self.dispatch(action, label);
        self.refresh();
        if transient {
            let _ = self.display.try_send(DisplayCmd::Flash { index: idx as u8 });
        }
    }

    /// Advance / reverse / reset a cycle button's state and emit the landed
    /// step. `idx` is a validated switch index; `ci` the cycle index. An empty
    /// or out-of-range cycle is a no-op.
    fn cycle_step(&mut self, idx: usize, ci: u8, dir: CycleDir) {
        let (next, step) = {
            let cyc = match self.config.cycle(ci) {
                Some(c) if !c.steps.is_empty() => c,
                _ => return,
            };
            let len = cyc.steps.len();
            let next = match dir {
                CycleDir::Advance => self.cycle_active[idx].map_or(0, |p| ((p as usize + 1) % len) as u8),
                CycleDir::Reverse => {
                    self.cycle_active[idx].map_or(0, |p| ((p as usize + len - 1) % len) as u8)
                }
                CycleDir::Reset => 0,
            };
            (next, cyc.steps[next as usize])
        };
        self.cycle_active[idx] = Some(next);
        self.dispatch_step(step);
        // A cycle is latched: the cell shows the new "pos/len", so no flash.
        self.refresh();
    }

    /// Emit one cycle step — the concrete MIDI it represents. Feedback (the
    /// cell's `pos/len`) is repainted by the caller via [`Self::refresh`].
    fn dispatch_step(&mut self, step: StepAction) {
        let channel = self.wire_channel();
        match step {
            StepAction::MidiCc { cc, value } => {
                let _ = self.midi_cmd.try_send(MidiCmd::ControlChange { channel, cc, value });
            }
            StepAction::ProgramChange { program } => {
                self.current_program = program;
                let _ = self
                    .midi_cmd
                    .try_send(MidiCmd::ProgramChange { channel, program });
            }
            StepAction::Sysex(cmd) => {
                if let Ok(sx) = build_sysex(cmd) {
                    let _ = self.sysex_out.try_send(sx);
                }
            }
        }
    }

    /// Perform one action's side effects (MIDI + state mutation). Returns
    /// `true` when the action is **transient** — it has no persistent
    /// [`CellState`], so the caller ([`Self::act`]) flashes the pressed cell.
    /// Display/LED repainting is the caller's job (`act` → `refresh`); this
    /// method only mutates state and sends MIDI. `_label` is retained for a
    /// future banner but unused now that feedback is the grid cell.
    fn dispatch(&mut self, action: Action, _label: config::Label) -> bool {
        let channel = self.wire_channel();
        match action {
            Action::None => false,
            Action::MidiCc { cc, value } => match value {
                CcValue::Fixed(v) => {
                    let _ = self.midi_cmd.try_send(MidiCmd::ControlChange { channel, cc, value: v });
                    true
                }
                CcValue::Toggle => {
                    let on = !self.toggles[cc as usize];
                    self.toggles[cc as usize] = on;
                    let _ = self
                        .midi_cmd
                        .try_send(MidiCmd::ControlChange { channel, cc, value: if on { 127 } else { 0 } });
                    false // latched: the cell shows on/off
                }
                // Self-toggling device: send `v` (e.g. 127) on EVERY press — the
                // device flips its own state — and track only a local on/off so
                // the cell + LED still read as a toggle. Never sends `0`, which is
                // what desynced `Toggle` on NeuralDSP-style plugins.
                CcValue::Trigger(v) => {
                    self.toggles[cc as usize] = !self.toggles[cc as usize];
                    let _ = self.midi_cmd.try_send(MidiCmd::ControlChange { channel, cc, value: v });
                    false // latched: the cell shows on/off
                }
                // Momentary is edge-driven in `on_button_perf` (press=127,
                // release=0). Reaching dispatch means it was mapped without a
                // release edge (e.g. a long-press) — do nothing rather than
                // leave the CC stuck high.
                CcValue::Momentary => false,
            },
            Action::ProgramChange { program } => {
                self.current_program = program;
                let _ = self
                    .midi_cmd
                    .try_send(MidiCmd::ProgramChange { channel, program });
                true
            }
            Action::Sysex(cmd) => {
                if let Ok(sx) = build_sysex(cmd) {
                    let _ = self.sysex_out.try_send(sx);
                }
                true
            }
            Action::PageNext => {
                let n = self.config.page_count();
                self.change_page((self.page + 1) % n);
                false
            }
            Action::PagePrev => {
                let n = self.config.page_count();
                self.change_page((self.page + n - 1) % n);
                false
            }
            Action::PageChange(p) => {
                self.change_page(p as usize);
                false
            }
            // `dispatch` only runs in performance mode, so this always *enters*
            // the tuner; leaving is handled by the footswitch/encoder in
            // `Mode::Tuner` (see `on_button` / `on_encoder`).
            Action::TunerToggle => {
                self.enter_tuner();
                false
            }
            Action::ProgramChangeStep(step) => {
                let program = (self.current_program as i16 + step as i16).clamp(0, 127) as u8;
                self.current_program = program;
                let _ = self
                    .midi_cmd
                    .try_send(MidiCmd::ProgramChange { channel, program });
                true
            }
            // Cycles need the button index for per-button position, so they are
            // handled in `on_button_perf` (the only place an `Action::Cycle` is
            // meaningful). In any other slot it is inert.
            Action::Cycle(_) => false,
            // Hand the HID report to the HID task, which writes the press +
            // release tap. Fire-and-forget (no toggle/selection state); a full
            // queue drops the tap rather than blocking the router.
            Action::Hid(report) => {
                let _ = self.hid.try_send(report);
                true
            }
            // Tap tempo: mark a beat, set the delay time from the interval. The
            // cell flashes per tap (transient — no persistent state).
            Action::TapTempo => {
                self.tap_tempo();
                true
            }
        }
    }

    /// One tap-tempo beat. Measures the interval since the previous tap and, if
    /// it falls in a musical window (≈30–600 BPM), sets the Katana delay time to
    /// that interval (SysEx). A first tap — or one after a > 2 s gap — just
    /// (re)starts the count; a < 100 ms re-trigger is ignored as a bounce.
    fn tap_tempo(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.tap_last {
            let dt = now.saturating_duration_since(last).as_millis() as u32;
            if dt < 100 {
                return; // debounce — keep the previous tap as the reference
            }
            if dt <= 2000 {
                if let Ok(sx) = katana::set_delay_time(dt as u16) {
                    let _ = self.sysex_out.try_send(sx);
                }
                defmt::info!("tap tempo: {=u32} ms ({=u32} bpm)", dt, 60_000 / dt);
            }
            // dt > 2000 ms → too slow; fall through to restart the count.
        }
        self.tap_last = Some(now);
    }

    /// Send a Control Change on the global MIDI channel.
    fn send_cc(&self, cc: u8, value: u8) {
        let channel = self.wire_channel();
        let _ = self.midi_cmd.try_send(MidiCmd::ControlChange { channel, cc, value });
    }

    /// Push the live meter levels to the display as a screen-neutral overlay.
    /// `try_send` (latest-wins; a full queue just drops the intermediate frame,
    /// the next change re-sends). Only called in performance mode, and the
    /// display task additionally ignores it unless the grid is showing.
    fn send_meters(&self) {
        let _ = self.display.try_send(DisplayCmd::Meters {
            exp1: self.meter_values[0],
            exp2: self.meter_values[1],
            encoder: self.meter_values[2],
        });
    }

    /// Emit a continuous-control value (`0..=127` — from the encoder or an
    /// expression pedal) per the active page's [`config::ContinuousBinding`]:
    /// a CC carrying the value verbatim, or a Katana SysEx with the value scaled
    /// to the parameter's `0..=100` range. `None` → silent. Sends only; no state.
    fn emit_continuous(&self, binding: config::ContinuousBinding, value: u8) {
        match binding {
            config::ContinuousBinding::None => {}
            config::ContinuousBinding::MidiCc(cc) => {
                let channel = self.wire_channel();
                let _ = self
                    .midi_cmd
                    .try_send(MidiCmd::ControlChange { channel, cc, value });
            }
            config::ContinuousBinding::Sysex(param) => {
                let scaled = ((value as u16 * 100) / 127) as u8; // 0..127 → 0..100
                let sx = match param {
                    config::ContinuousSysex::Volume => katana::set_volume(scaled),
                    config::ContinuousSysex::Wah => katana::set_wah_position(scaled),
                };
                if let Ok(sx) = sx {
                    let _ = self.sysex_out.try_send(sx);
                }
            }
        }
    }

    async fn on_encoder(&mut self, ev: EncoderEvent) {
        match ev {
            EncoderEvent::Turn(delta) => match self.mode {
                Mode::Performance => {
                    // Copy the binding out (it's `Copy`) before mutating state.
                    let binding = self.current_page().encoder;
                    let v = (self.enc_value as i16 + delta as i16).clamp(0, 127) as u8;
                    if v != self.enc_value {
                        self.enc_value = v;
                        self.emit_continuous(binding, v);
                        self.meter_values[2] = v;
                        self.send_meters();
                    }
                }
                Mode::Menu => {
                    let outcome = self.menu.turn(delta, &mut self.settings);
                    self.apply_menu_outcome(outcome).await;
                }
                // The tuner is read-only; rotation does nothing.
                Mode::Tuner => {}
                Mode::Edit => {
                    let page = self.page;
                    let outcome = self.editor.turn(delta, &mut self.config, page);
                    self.apply_edit_outcome(outcome).await;
                }
            },
            EncoderEvent::Press => self.enc_press_at = Some(Instant::now()),
            EncoderEvent::Release => {
                let long = self
                    .enc_press_at
                    .take()
                    .map(|t| Instant::now().saturating_duration_since(t) >= LONG_PRESS)
                    .unwrap_or(false);
                if long {
                    self.on_encoder_hold().await;
                } else if matches!(self.mode, Mode::Menu) {
                    let outcome = self.menu.press();
                    self.apply_menu_outcome(outcome).await;
                } else if matches!(self.mode, Mode::Edit) {
                    let page = self.page;
                    let outcome = self.editor.press(&mut self.config, page);
                    self.apply_edit_outcome(outcome).await;
                }
            }
        }
    }

    fn on_expr(&mut self, ev: ExprEvent) {
        // Pedals are silent in the menu — they're being moved for calibration.
        if !matches!(self.mode, Mode::Performance) {
            return;
        }
        let pedal = (ev.pedal as usize).min(1);
        self.emit_continuous(self.current_page().expr[pedal], ev.value);
        self.meter_values[pedal] = ev.value;
        self.send_meters();
    }

    /// Dispatch inbound device MIDI. In the tuner it drives the pitch readout;
    /// otherwise it keeps toggle state + LED feedback in sync with the amp.
    fn on_midi_rx(&mut self, m: MidiRx) {
        match self.mode {
            Mode::Tuner => self.on_midi_rx_tuner(m),
            _ => self.sync_cc(m),
        }
    }

    /// Sync toggle state (and LED feedback) from inbound device CC —
    /// bidirectional sync, so the board reflects the amp's real state.
    fn sync_cc(&mut self, m: MidiRx) {
        if let MidiRx::ControlChange { cc, value, .. } = m {
            let on = value > 63;
            if self.toggles[cc as usize] != on {
                self.toggles[cc as usize] = on;
                self.refresh(); // update LEDs + (in performance) the toggle cell
            }
        }
    }

    /// Dispatch an inbound, reassembled SysEx message from the device. In
    /// performance mode a recognised Katana DT1 reflects the amp's real state two
    /// ways: a radio param (amp type / preset) selects the matching radio button,
    /// and an effect-switch block (BOOST/MOD/…) bridges to its `cc_alias` toggle.
    /// Both are also cached so they survive a page change (see
    /// [`Self::reapply_device_state`]). Other modes and unrecognised messages are
    /// ignored (no echo, no allocation).
    fn on_sysex_rx(&mut self, msg: sysex::SysEx) {
        if !matches!(self.mode, Mode::Performance) {
            return;
        }
        let Some(dt1) = katana::parse_dt1(&msg, &katana::KATANA_MODEL_ID) else {
            return;
        };
        // Radio param (amp type / preset): cache for page-entry re-apply, then
        // reflect onto the active page's radios.
        if let Some(cmd) = sysex_cmd_from_dt1(&dt1) {
            match cmd {
                SysexCmd::AmpType(t) => self.dev_amp_type = Some(t),
                SysexCmd::RecallPreset(p) => self.dev_preset = Some(p),
                _ => {}
            }
            self.reflect_sysex(cmd);
        // Effect-switch block: bridge the block DT1 to its `cc_alias` and reflect
        // the amp's on/off on the matching CC toggle.
        } else if let Some(cc) = katana::effect_block_cc(&dt1.address) {
            if let Some(&v) = dt1.data.first() {
                self.reflect_block(cc, v != 0);
            }
        }
    }

    /// Bridge a Katana effect-switch block to its CC toggle: record the amp-
    /// confirmed on/off in both the live [`Self::toggles`] (so the current page's
    /// LED + grid cell update) and the persistent [`Self::dev_toggles`] cache (so
    /// it survives a page change — re-applied by [`Self::reapply_device_state`]),
    /// repainting only if the live state changed. The device-confirmed SysEx
    /// analogue of [`Self::sync_cc`]; unlike a bare CC echo it is cached across
    /// pages, since the amp has positively reported this block's state.
    fn reflect_block(&mut self, cc: u8, on: bool) {
        self.dev_toggles[cc as usize] = Some(on);
        if self.toggles[cc as usize] != on {
            self.toggles[cc as usize] = on;
            self.refresh();
        }
    }

    /// Reflect a device-reported [`SysexCmd`] onto the active page: select the
    /// grouped button whose press emits that same command (radio sync), then
    /// repaint. No-op if the page has no such grouped button, or it is already
    /// the group's selection.
    fn reflect_sysex(&mut self, cmd: SysexCmd) {
        let found = {
            let page = self.current_page();
            page.buttons.iter().enumerate().find_map(|(i, b)| {
                if b.on_press == Action::Sysex(cmd) {
                    group_slot(b.group).map(|slot| (i as u8, slot))
                } else {
                    None
                }
            })
        };
        if let Some((idx, slot)) = found {
            if self.group_sel[slot] != Some(idx) {
                self.group_sel[slot] = Some(idx);
                self.refresh();
            }
        }
    }

    /// Feed the tuner from the amp's Note On (which note) + Pitch Bend (cents).
    /// A Note Off — or a zero-velocity Note On — clears the readout.
    fn on_midi_rx_tuner(&mut self, m: MidiRx) {
        let changed = match m {
            MidiRx::Note { note, velocity, on, .. } => {
                if on && velocity > 0 {
                    self.tuner.update_note(note);
                } else {
                    self.tuner.clear_note();
                }
                true
            }
            MidiRx::PitchBend { value, .. } => {
                self.tuner.update_pitch_bend(value);
                true
            }
            _ => false,
        };
        if changed {
            let cmd = self.tuner.display_cmd();
            let _ = self.display.try_send(cmd);
        }
    }

    // ── mode transitions ───────────────────────────────────────────────

    /// Encoder long-press: enter the menu from performance, or leave the
    /// current mode (menu → save + performance, tuner → performance).
    async fn on_encoder_hold(&mut self) {
        match self.mode {
            Mode::Performance => self.enter_menu(),
            Mode::Menu => self.leave_menu().await,
            Mode::Tuner => self.exit_tuner(),
            Mode::Edit => self.leave_edit().await,
        }
    }

    /// Enter the settings menu (from performance).
    fn enter_menu(&mut self) {
        self.mode = Mode::Menu;
        self.menu.enter();
        let cmd = self.menu.display_cmd(&self.settings);
        let _ = self.display.try_send(cmd);
    }

    /// Enter the on-device config editor for the current page.
    fn enter_edit(&mut self) {
        self.mode = Mode::Edit;
        self.editor.enter();
        let cmd = self.editor.display_cmd(&self.config, self.page);
        let _ = self.display.try_send(cmd);
    }

    /// Enter the tuner: ask the amp to start its tuner (CC#25 = 127), switch
    /// modes, and paint the initial (empty) readout. The amp's Note/Pitch-Bend
    /// stream then drives [`Self::on_midi_rx_tuner`].
    fn enter_tuner(&mut self) {
        let channel = self.wire_channel();
        let _ = self.midi_cmd.try_send(MidiCmd::ControlChange {
            channel,
            cc: TUNER_CC,
            value: 127,
        });
        self.mode = Mode::Tuner;
        self.tuner.reset();
        let cmd = self.tuner.display_cmd();
        let _ = self.display.try_send(cmd);
    }

    /// Leave the tuner: tell the amp to stop (CC#25 = 0) and restore the
    /// performance page (which repaints the normal screen + LED brightness).
    fn exit_tuner(&mut self) {
        let channel = self.wire_channel();
        let _ = self.midi_cmd.try_send(MidiCmd::ControlChange {
            channel,
            cc: TUNER_CC,
            value: 0,
        });
        self.mode = Mode::Performance;
        self.tuner.reset();
        self.refresh_page();
    }

    /// Leave the menu: persist settings, restore the performance page (which
    /// also re-applies LED brightness).
    async fn leave_menu(&mut self) {
        self.mode = Mode::Performance;
        self.save().await;
        self.refresh_page();
    }

    /// Persist the edited config to flash, but only if a change was made (saves
    /// flash wear). The edits are already live in `self.config`. Shared by the
    /// editor's HOME exit ([`Self::leave_edit`]) and its `< Back` step-out.
    async fn persist_edits(&mut self) {
        if self.editor.dirty()
            && self
                .storage
                .store_config(&self.config, self.config_scratch)
                .await
                .is_err()
        {
            warn!("router: edit config save failed");
        }
    }

    /// Leave the editor (HOME): persist any edits, then restore the performance
    /// page. The repaint reflects the now-live config.
    async fn leave_edit(&mut self) {
        self.persist_edits().await;
        self.mode = Mode::Performance;
        self.refresh_page();
    }

    /// Step out of the editor back to the settings menu (`< Back` at the top
    /// level). Persists edits first so they survive even if the user then exits
    /// the menu (whose own exit only saves settings, not config).
    async fn back_to_menu(&mut self) {
        self.persist_edits().await;
        self.mode = Mode::Menu;
        self.menu.enter();
        let cmd = self.menu.display_cmd(&self.settings);
        let _ = self.display.try_send(cmd);
    }

    async fn apply_menu_outcome(&mut self, outcome: MenuOutcome) {
        match outcome {
            MenuOutcome::Redraw => {
                let cmd = self.menu.display_cmd(&self.settings);
                let _ = self.display.try_send(cmd);
                // Keep the backlight live with the (possibly just-edited) setting
                // so "Disp Bright" dims the screen as the user turns the encoder.
                self.sync_backlight();
            }
            MenuOutcome::Exit => self.leave_menu().await,
            MenuOutcome::CalSaved(cals) => {
                // Push live to the sampler (applies without a reboot) + persist.
                expression::LIVE_CAL.lock(|c| c.set(Some(cals)));
                self.save().await;
                let cmd = self.menu.display_cmd(&self.settings);
                let _ = self.display.try_send(cmd);
            }
            MenuOutcome::EnterEdit => {
                // Persist any settings the user changed first, then open the
                // editor on the current page.
                self.save().await;
                self.enter_edit();
            }
        }
    }

    /// Service an editor interaction: repaint the editor view, or save + leave.
    async fn apply_edit_outcome(&mut self, outcome: EditOutcome) {
        match outcome {
            EditOutcome::Redraw => {
                let cmd = self.editor.display_cmd(&self.config, self.page);
                let _ = self.display.try_send(cmd);
            }
            EditOutcome::Back => self.back_to_menu().await,
            EditOutcome::Save => self.leave_edit().await,
        }
    }

    /// Persist the current settings to flash.
    async fn save(&mut self) {
        if self.storage.store(&self.settings).await.is_err() {
            warn!("router: settings save failed");
        }
    }

    /// Publish the config's MIDI-thru routes to the transport layer (`mux`),
    /// which forwards inbound MIDI per them. Called at startup and on every
    /// config apply.
    fn sync_thru(&self) {
        mux::set_thru(self.config.midi_thru);
    }

    /// Push the persisted display-brightness setting to the display task, which
    /// applies it to the PWM backlight. Called at startup and whenever the
    /// settings menu changes it (so the screen dims live as the user adjusts).
    fn sync_backlight(&self) {
        let _ = self.display.try_send(DisplayCmd::Backlight(self.settings.display_brightness));
    }

    // ── config sync (webapp ↔ device over CDC) ─────────────────────────
    /// Handle a config-sync request end-to-end: compute the response and send
    /// it back over the owned [`Self::config_resp`] channel. The pure decision
    /// logic stays in [`Self::on_config_req`].
    async fn handle_config_req(&mut self, req: ConfigReq) {
        let resp = self.on_config_req(req).await;
        self.config_resp.send(resp).await;
    }

    /// Service a config-sync request from the CDC task.
    ///
    /// `Get` returns a clone of the live config for the CDC task to serialize.
    /// `Set` (already validated by the CDC task) is persisted to flash and then
    /// hot-reloaded: adopt the new config and repaint from a clean slate —
    /// performance mode, page 0, toggles cleared — exactly as a fresh boot into
    /// it would. Replies `Ok`, or an error code the CDC task relays as `ERROR`.
    async fn on_config_req(&mut self, req: ConfigReq) -> ConfigResp {
        match req {
            ConfigReq::Get => ConfigResp::Config(self.config.clone()),
            ConfigReq::Set(cfg) => {
                // Defensive: an empty page list would panic `page()`. The CDC
                // task already rejects this, so it's belt-and-suspenders.
                if cfg.page_count() == 0 {
                    return ConfigResp::Err(proto::ProtoError::BadPayload);
                }
                if self
                    .storage
                    .store_config(&cfg, self.config_scratch)
                    .await
                    .is_err()
                {
                    warn!("router: config store failed");
                    return ConfigResp::Err(proto::ProtoError::StoreFailed);
                }
                self.config = cfg;
                self.sync_thru();
                self.mode = Mode::Performance;
                self.page = 0;
                self.toggles = [false; 128];
                self.group_sel = [None; config::MAX_GROUPS];
                self.cycle_active = [None; config::PAGE_BUTTONS];
                // Keep the device-confirmed cache and re-apply it: the amp's real
                // state still holds across a config swap, so device-backed buttons
                // the new config defines reflect it immediately (parity with a
                // page change).
                self.reapply_device_state();
                self.refresh_page();
                defmt::info!("router: config applied ({} page(s))", self.config.page_count());
                ConfigResp::Ok
            }
        }
    }
}

/// Map a 1-based config group id to its [`Router::group_sel`] slot, or `None`
/// if the id is `0` (ungrouped) or out of range (`> MAX_GROUPS`). Out-of-range
/// ids render and behave as ungrouped — a malformed config can't index past the
/// array.
fn group_slot(group: u8) -> Option<usize> {
    let g = group as usize;
    (1..=config::MAX_GROUPS).contains(&g).then_some(g - 1)
}

/// Scale an LED colour by a brightness percentage (`0..=100`).
fn scale(c: LedColor, pct: u8) -> LedColor {
    let p = pct.min(100) as u16;
    LedColor {
        r: (c.r as u16 * p / 100) as u8,
        g: (c.g as u16 * p / 100) as u8,
        b: (c.b as u16 * p / 100) as u8,
    }
}

/// Build the on-wire SysEx for a config [`SysexCmd`] via the Katana builders.
fn build_sysex(cmd: SysexCmd) -> Result<sysex::SysEx, sysex::SysExError> {
    match cmd {
        SysexCmd::RecallPreset(p) => katana::recall_preset(p),
        SysexCmd::AmpType(t) => katana::set_amp_type(t),
        SysexCmd::Gain(v) => katana::set_gain(v),
        SysexCmd::Volume(v) => katana::set_volume(v),
    }
}

/// Reverse-map an inbound Katana [`katana::Dt1`] to the [`SysexCmd`] a button
/// would emit to produce it — the inverse of [`build_sysex`]. Only the four
/// parameters the board can mirror as button state are decoded; any other
/// address (or a too-short payload) yields [`None`]. Reflection onto the page
/// is [`Router::reflect_sysex`]'s job — this is the pure decode.
fn sysex_cmd_from_dt1(dt1: &katana::Dt1) -> Option<SysexCmd> {
    match dt1.address {
        katana::ADDR_AMP_TYPE => dt1.data.first().map(|&v| SysexCmd::AmpType(v)),
        // `recall_preset` emits `[0x00, preset]`; the preset is the second byte.
        katana::ADDR_RECALL_PRESET => dt1.data.get(1).map(|&p| SysexCmd::RecallPreset(p)),
        katana::ADDR_GAIN => dt1.data.first().map(|&v| SysexCmd::Gain(v)),
        katana::ADDR_VOLUME => dt1.data.first().map(|&v| SysexCmd::Volume(v)),
        _ => None,
    }
}

/// One-shot boot device-state query (the *active* half of device sync). When
/// the loaded config drives a Katana (`enabled`), this waits for USB/MIDI to
/// settle, puts the amp into editor/BTS mode — so it both answers the RQ1 reads
/// and broadcasts later front-panel changes — then sweeps an RQ1 read of the
/// mirrored radio params ([`katana::DEVICE_QUERY_SWEEP`]) and the effect-switch
/// blocks ([`katana::EFFECT_BLOCKS`]). The amp's DT1 replies arrive on the
/// inbound SysEx channel and are reflected onto the LEDs by
/// [`Router::on_sysex_rx`]. Editor mode is left engaged on purpose — exiting it
/// would stop the passive sync. A no-op (returns immediately) when not enabled.
///
/// Runs once and exits; it borrows only its own `SysExSender` (a `Copy` handle
/// to the same outbound channel the router uses), so it needs no router state.
#[embassy_executor::task]
pub async fn device_query_task(sysex_out: mux::SysExSender, enabled: bool) {
    if !enabled {
        return;
    }
    // Let enumeration / the amp's MIDI input settle before querying.
    Timer::after(Duration::from_millis(1500)).await;
    if let Ok(sx) = katana::enter_editor_mode() {
        let _ = sysex_out.try_send(sx);
    }
    Timer::after(Duration::from_millis(100)).await; // CP `settle_time_ms`
    // Radio params (amp type + preset) ...
    for (addr, len) in katana::DEVICE_QUERY_SWEEP {
        if let Ok(sx) = katana::rq1(&katana::KATANA_MODEL_ID, &addr, len) {
            let _ = sysex_out.try_send(sx);
        }
        Timer::after(Duration::from_millis(20)).await; // CP inter-query delay
    }
    // ... then the effect-switch blocks (one byte each). Their DT1 replies bridge
    // to the GA-FC CC toggles via `Router::on_sysex_rx` → `reflect_block`.
    for (addr, _cc) in katana::EFFECT_BLOCKS {
        if let Ok(sx) = katana::rq1(&katana::KATANA_MODEL_ID, &addr, 1) {
            let _ = sysex_out.try_send(sx);
        }
        Timer::after(Duration::from_millis(20)).await;
    }
    defmt::info!(
        "device query: editor mode + {} RQ1 read(s) sent",
        katana::DEVICE_QUERY_SWEEP.len() + katana::EFFECT_BLOCKS.len()
    );
}

/// Drive the router: select across every input channel, dispatch each event.
///
/// Six inputs, and `embassy_futures` tops out at [`select4`], so the four
/// hardware channels nest inside an outer [`select`] whose other arm is itself
/// a `select` over the two low-rate device channels (config-sync requests and
/// inbound SysEx). All receive futures are cancellation-safe, so the branch
/// that doesn't win simply re-arms next iteration with no lost messages.
#[embassy_executor::task]
pub async fn router_task(
    mut r: Router,
    buttons: buttons::ButtonReceiver,
    encoder: encoder::EncoderReceiver,
    expr: expression::ExprReceiver,
    midi_rx: mux::MidiRxReceiver,
    config_req: ConfigReqReceiver,
    sysex_in: mux::SysExReceiver,
) {
    r.refresh_page(); // initial paint
    r.sync_thru(); // publish the config's MIDI-thru routes to the mux
    r.sync_backlight(); // apply the persisted display brightness to the PWM backlight
    loop {
        match select(
            select4(
                buttons.receive(),
                encoder.receive(),
                expr.receive(),
                midi_rx.receive(),
            ),
            select(config_req.receive(), sysex_in.receive()),
        )
        .await
        {
            Either::First(Either4::First(b)) => r.on_button(b).await,
            Either::First(Either4::Second(e)) => r.on_encoder(e).await,
            Either::First(Either4::Third(x)) => r.on_expr(x),
            Either::First(Either4::Fourth(m)) => r.on_midi_rx(m),
            Either::Second(Either::First(req)) => r.handle_config_req(req).await,
            Either::Second(Either::Second(sx)) => r.on_sysex_rx(sx),
        }
    }
}
