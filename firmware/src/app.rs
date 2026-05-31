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
use embassy_time::{Duration, Instant};

use crate::config::{self, Action, CcValue, RuntimeConfig, SysexCmd};
use crate::events::{
    ButtonEvent, DisplayCmd, EncoderEvent, ExprEvent, LedColor, LedFrame, MidiCmd, MidiRx,
};
use crate::hal::{buttons, encoder, expression, leds};
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

/// CC the encoder drives (volume), and the per-pedal expression CCs —
/// matching `remedy/config/pages/default.toml` (`encoder.fallback.cc = 7`,
/// `pedal1.cc = 1`, `pedal2.cc = 7`). A later config extension makes these
/// per-page bindings; baked in for v1.
const ENCODER_CC: u8 = 7;
const EXPR_CC: [u8; 2] = [1, 7];

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
    /// Tuner readout (driven by inbound MIDI while in [`Mode::Tuner`]).
    tuner: TunerState,
    /// Accumulated encoder-driven value for [`ENCODER_CC`] (`0..=127`).
    enc_value: u8,
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
    /// Press timestamps for footswitch long-press detection, per switch index.
    press_at: [Option<Instant>; buttons::COUNT],
    /// For [`CcValue::Momentary`]: the CC a held switch is driving (sent `127`
    /// on press), so its release can send `0`. `None` = not momentary-held.
    momentary_active: [Option<u8>; buttons::COUNT],
    /// Encoder push timestamp, for its long-press (enter/exit the menu).
    enc_press_at: Option<Instant>,
    display: DisplaySender,
    leds: leds::LedSender,
    midi_cmd: mux::MidiCmdSender,
    sysex_out: mux::SysExSender,
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
    ) -> Self {
        Self {
            config,
            page: 0,
            settings,
            storage,
            config_scratch,
            mode: Mode::Performance,
            menu: Menu::new(),
            tuner: TunerState::new(),
            enc_value: 0,
            current_program: 0,
            toggles: [false; 128],
            group_sel: [None; config::MAX_GROUPS],
            press_at: [None; buttons::COUNT],
            momentary_active: [None; buttons::COUNT],
            enc_press_at: None,
            display,
            leds,
            midi_cmd,
            sysex_out,
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
            let base = if let Some(slot) = group_slot(btn.group) {
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

    /// Paint the page name/position to the display and refresh the LEDs.
    fn refresh_page(&self) {
        let page = self.current_page();
        let _ = self.display.try_send(DisplayCmd::Page {
            name: page.name.clone(),
            index: self.page as u8 + 1,
            total: self.config.page_count() as u8,
        });
        self.refresh_leds();
    }

    /// Switch pages, clearing per-page toggle + group state (CLAUDE.md) and
    /// repainting.
    fn change_page(&mut self, page: usize) {
        self.page = page.min(self.config.page_count().saturating_sub(1));
        self.toggles = [false; 128];
        self.group_sel = [None; config::MAX_GROUPS];
        self.refresh_page();
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
                self.send_momentary(idx, cc, true);
                self.momentary_active[idx] = Some(cc);
            }
            self.press_at[idx] = Some(Instant::now());
            return;
        }
        // Released. A held momentary completes here (send 0) and short-circuits
        // the long/short dispatch entirely.
        if let Some(cc) = self.momentary_active[idx].take() {
            self.press_at[idx] = None;
            self.send_momentary(idx, cc, false);
            return;
        }
        // Otherwise: pick long- vs short-press by held duration. Dispatching
        // on release is what lets a single switch carry both actions.
        let long = self.press_at[idx]
            .take()
            .map(|t| Instant::now().saturating_duration_since(t) >= LONG_PRESS)
            .unwrap_or(false);
        // Pick the action and clone the label out of the (immutably-borrowed)
        // config *before* `dispatch` takes `&mut self` — the owned label means
        // no config borrow is held across the mutable call.
        let (action, label, group) = {
            let btn = &self.current_page().buttons[idx];
            let action = if long && btn.on_long_press != Action::None {
                btn.on_long_press
            } else {
                btn.on_press
            };
            (action, btn.label.clone(), btn.group)
        };
        // A short press on a grouped button makes it that group's selection
        // (radio behaviour), deselecting the previous member. Recorded before
        // dispatch so the LED repaint reflects the new selection no matter what
        // the action does. Long-presses don't latch — they fire `on_long_press`
        // (e.g. tuner / page nav) without changing the group.
        if !long {
            if let Some(slot) = group_slot(group) {
                self.group_sel[slot] = Some(idx as u8);
                self.refresh_leds();
            }
        }
        self.dispatch(action, label);
    }

    fn dispatch(&mut self, action: Action, label: config::Label) {
        let channel = self.wire_channel();
        match action {
            Action::None => {}
            Action::MidiCc { cc, value } => {
                let (v, toggle, on) = match value {
                    CcValue::Fixed(v) => (v, false, false),
                    CcValue::Toggle => {
                        let on = !self.toggles[cc as usize];
                        self.toggles[cc as usize] = on;
                        (if on { 127 } else { 0 }, true, on)
                    }
                    // Momentary is edge-driven in `on_button_perf` (press=127,
                    // release=0). Reaching dispatch means it was mapped without a
                    // release edge (e.g. a long-press) — do nothing rather than
                    // leave the CC stuck high.
                    CcValue::Momentary => return,
                };
                let _ = self.midi_cmd.try_send(MidiCmd::ControlChange { channel, cc, value: v });
                let _ = self.display.try_send(DisplayCmd::Action { label, toggle, on });
                if toggle {
                    self.refresh_leds();
                }
            }
            Action::ProgramChange { program } => {
                self.current_program = program;
                let _ = self
                    .midi_cmd
                    .try_send(MidiCmd::ProgramChange { channel, program });
                self.announce(label);
            }
            Action::Sysex(cmd) => {
                if let Ok(sx) = build_sysex(cmd) {
                    let _ = self.sysex_out.try_send(sx);
                }
                self.announce(label);
            }
            Action::PageNext => {
                let n = self.config.page_count();
                self.change_page((self.page + 1) % n);
            }
            Action::PagePrev => {
                let n = self.config.page_count();
                self.change_page((self.page + n - 1) % n);
            }
            Action::PageChange(p) => self.change_page(p as usize),
            // `dispatch` only runs in performance mode, so this always *enters*
            // the tuner; leaving is handled by the footswitch/encoder in
            // `Mode::Tuner` (see `on_button` / `on_encoder`).
            Action::TunerToggle => self.enter_tuner(),
            Action::ProgramChangeStep(step) => {
                let program = (self.current_program as i16 + step as i16).clamp(0, 127) as u8;
                self.current_program = program;
                let _ = self
                    .midi_cmd
                    .try_send(MidiCmd::ProgramChange { channel, program });
                self.announce(label);
            }
        }
    }

    /// Send a momentary CC edge (`127` on press, `0` on release) and reflect it
    /// on the status line. `idx` is a validated switch index.
    fn send_momentary(&self, idx: usize, cc: u8, on: bool) {
        let channel = self.wire_channel();
        let _ = self.midi_cmd.try_send(MidiCmd::ControlChange {
            channel,
            cc,
            value: if on { 127 } else { 0 },
        });
        let label = self.current_page().buttons[idx].label.clone();
        let _ = self.display.try_send(DisplayCmd::Action { label, toggle: true, on });
    }

    /// Show a non-toggle action's label on the display.
    fn announce(&self, label: config::Label) {
        let _ = self.display.try_send(DisplayCmd::Action {
            label,
            toggle: false,
            on: false,
        });
    }

    async fn on_encoder(&mut self, ev: EncoderEvent) {
        match ev {
            EncoderEvent::Turn(delta) => match self.mode {
                Mode::Performance => {
                    let v = (self.enc_value as i16 + delta as i16).clamp(0, 127) as u8;
                    if v != self.enc_value {
                        self.enc_value = v;
                        let channel = self.wire_channel();
                        let _ = self.midi_cmd.try_send(MidiCmd::ControlChange {
                            channel,
                            cc: ENCODER_CC,
                            value: v,
                        });
                    }
                }
                Mode::Menu => {
                    let outcome = self.menu.turn(delta, &mut self.settings);
                    self.apply_menu_outcome(outcome).await;
                }
                // The tuner is read-only; rotation does nothing.
                Mode::Tuner => {}
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
                }
            }
        }
    }

    fn on_expr(&self, ev: ExprEvent) {
        // Pedals are silent in the menu — they're being moved for calibration.
        if !matches!(self.mode, Mode::Performance) {
            return;
        }
        let cc = EXPR_CC[(ev.pedal as usize).min(1)];
        let channel = self.wire_channel();
        let _ = self.midi_cmd.try_send(MidiCmd::ControlChange {
            channel,
            cc,
            value: ev.value,
        });
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
                self.refresh_leds();
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
        }
    }

    /// Enter the settings menu (from performance).
    fn enter_menu(&mut self) {
        self.mode = Mode::Menu;
        self.menu.enter();
        let cmd = self.menu.display_cmd(&self.settings);
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

    async fn apply_menu_outcome(&mut self, outcome: MenuOutcome) {
        match outcome {
            MenuOutcome::Redraw => {
                let cmd = self.menu.display_cmd(&self.settings);
                let _ = self.display.try_send(cmd);
            }
            MenuOutcome::Exit => self.leave_menu().await,
            MenuOutcome::CalSaved(cals) => {
                // Push live to the sampler (applies without a reboot) + persist.
                expression::LIVE_CAL.lock(|c| c.set(Some(cals)));
                self.save().await;
                let cmd = self.menu.display_cmd(&self.settings);
                let _ = self.display.try_send(cmd);
            }
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

    // ── config sync (webapp ↔ device over CDC) ─────────────────────────
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

/// Drive the router: select across every input channel, dispatch each event.
///
/// Five inputs, so the four hardware channels nest inside a [`select`] with the
/// config-sync request channel (`embassy_futures` tops out at `select4`). The
/// channel receive futures are cancellation-safe, so the branch that doesn't
/// win simply re-arms next iteration with no lost messages.
#[embassy_executor::task]
pub async fn router_task(
    mut r: Router,
    buttons: buttons::ButtonReceiver,
    encoder: encoder::EncoderReceiver,
    expr: expression::ExprReceiver,
    midi_rx: mux::MidiRxReceiver,
    config_req: ConfigReqReceiver,
    config_resp: ConfigRespSender,
) {
    r.refresh_page(); // initial paint
    r.sync_thru(); // publish the config's MIDI-thru routes to the mux
    loop {
        match select(
            select4(
                buttons.receive(),
                encoder.receive(),
                expr.receive(),
                midi_rx.receive(),
            ),
            config_req.receive(),
        )
        .await
        {
            Either::First(Either4::First(b)) => r.on_button(b).await,
            Either::First(Either4::Second(e)) => r.on_encoder(e).await,
            Either::First(Either4::Third(x)) => r.on_expr(x),
            Either::First(Either4::Fourth(m)) => r.on_midi_rx(m),
            Either::Second(req) => {
                let resp = r.on_config_req(req).await;
                config_resp.send(resp).await;
            }
        }
    }
}
