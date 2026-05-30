//! Application event router + state machine.
//!
//! Extracted from `bin/midicaptain.rs` once enough subsystems were wired
//! that the bin was mostly router logic. The bin is now thin wiring
//! (construct peripherals, spawn tasks); this module owns *what the device
//! does* with the events those tasks produce.
//!
//! [`Router`] holds the application state — active page, per-CC toggle
//! state, MIDI channel — and turns input events into the actions the active
//! [`crate::config`] page defines: per-button CC / PC / SysEx / page-nav,
//! with LED on/off feedback for toggles and a page/action readout on the
//! display. Incoming MIDI CC syncs toggle state back (bidirectional).
//!
//! It is the **single owner** of that state (no shared mutables); the rest
//! of the system influences it only by sending events to [`router_task`].

use defmt::info;
use embassy_futures::select::{select4, Either4};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Instant};

use crate::config::{self, Action, CcValue, Config, SysexCmd};
use crate::events::{ButtonEvent, DisplayCmd, EncoderEvent, ExprEvent, LedFrame, MidiCmd, MidiRx};
use crate::hal::{buttons, encoder, expression, leds};
use crate::midi::{katana, mux, sysex};
use crate::pins;

/// Press duration at/above which a release counts as a long-press.
const LONG_PRESS: Duration = Duration::from_millis(500);

/// CC the encoder drives (volume), and the per-pedal expression CCs —
/// matching `remedy/config/pages/default.toml` (`encoder.fallback.cc = 7`,
/// `pedal1.cc = 1`, `pedal2.cc = 7`). A later config extension makes these
/// per-page bindings; baked in for v1.
const ENCODER_CC: u8 = 7;
const EXPR_CC: [u8; 2] = [1, 7];

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

/// The event router + application state.
pub struct Router {
    config: Config,
    page: usize,
    midi_channel: u8,
    /// Accumulated encoder-driven value for [`ENCODER_CC`] (`0..=127`).
    enc_value: u8,
    /// Per-CC toggle state (on/off), indexed by CC number. Cleared on page
    /// change; synced from incoming MIDI CC.
    toggles: [bool; 128],
    /// Press timestamps for long-press detection, per switch index.
    press_at: [Option<Instant>; buttons::COUNT],
    display: DisplaySender,
    leds: leds::LedSender,
    midi_cmd: mux::MidiCmdSender,
    sysex_out: mux::SysExSender,
}

impl Router {
    /// Build a router bound to its output channels. `midi_channel` is the
    /// 0-based wire channel (the caller converts from the 1-based setting).
    pub fn new(
        config: Config,
        midi_channel: u8,
        display: DisplaySender,
        leds: leds::LedSender,
        midi_cmd: mux::MidiCmdSender,
        sysex_out: mux::SysExSender,
    ) -> Self {
        Self {
            config,
            page: 0,
            midi_channel,
            enc_value: 0,
            toggles: [false; 128],
            press_at: [None; buttons::COUNT],
            display,
            leds,
            midi_cmd,
            sysex_out,
        }
    }

    fn current_page(&self) -> &'static config::Page {
        self.config.page(self.page)
    }

    /// Build the LED frame for the active page + toggle state and send it.
    /// Toggle buttons: full colour when on, `idle_dim` when off. Non-toggle
    /// bound buttons: full colour. Unbound slots: off.
    fn refresh_leds(&self) {
        let page = self.current_page();
        let mut frame = LedFrame {
            switches: [config::color::OFF; pins::Switch::COUNT],
        };
        for (bi, btn) in page.buttons.iter().enumerate() {
            let color = match btn.on_press.toggle_cc() {
                Some(cc) => {
                    if self.toggles[cc as usize] {
                        btn.color
                    } else {
                        leds::idle_dim(btn.color)
                    }
                }
                None if matches!(btn.on_press, Action::None) => config::color::OFF,
                None => btn.color,
            };
            // Scan-index → WS2812 chain position.
            frame.switches[config::SWITCH_FOR_BUTTON[bi] as usize] = color;
        }
        let _ = self.leds.try_send(frame);
    }

    /// Paint the page name/position to the display and refresh the LEDs.
    fn refresh_page(&self) {
        let page = self.current_page();
        let _ = self.display.try_send(DisplayCmd::Page {
            name: page.name,
            index: self.page as u8 + 1,
            total: self.config.page_count() as u8,
        });
        self.refresh_leds();
    }

    /// Switch pages, clearing per-page toggle state (CLAUDE.md) and repainting.
    fn change_page(&mut self, page: usize) {
        self.page = page.min(self.config.page_count() - 1);
        self.toggles = [false; 128];
        self.refresh_page();
    }

    fn on_button(&mut self, ev: ButtonEvent) {
        let idx = ev.index as usize;
        if idx >= buttons::COUNT {
            return; // defensive: ignore out-of-range indices
        }
        if ev.pressed {
            self.press_at[idx] = Some(Instant::now());
            return;
        }
        // Released: pick long- vs short-press by held duration. Dispatching
        // on release is what lets a single switch carry both actions.
        let long = self.press_at[idx]
            .take()
            .map(|t| Instant::now().saturating_duration_since(t) >= LONG_PRESS)
            .unwrap_or(false);
        let btn = self.current_page().buttons[idx];
        let action = if long && btn.on_long_press != Action::None {
            btn.on_long_press
        } else {
            btn.on_press
        };
        self.dispatch(action, btn.label);
    }

    fn dispatch(&mut self, action: Action, label: &'static str) {
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
                };
                let _ = self.midi_cmd.try_send(MidiCmd::ControlChange {
                    channel: self.midi_channel,
                    cc,
                    value: v,
                });
                let _ = self.display.try_send(DisplayCmd::Action { label, toggle, on });
                if toggle {
                    self.refresh_leds();
                }
            }
            Action::ProgramChange { program } => {
                let _ = self.midi_cmd.try_send(MidiCmd::ProgramChange {
                    channel: self.midi_channel,
                    program,
                });
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
            Action::TunerToggle => info!("router: tuner toggle (Wave 3)"),
        }
    }

    /// Show a non-toggle action's label on the display.
    fn announce(&self, label: &'static str) {
        let _ = self.display.try_send(DisplayCmd::Action {
            label,
            toggle: false,
            on: false,
        });
    }

    fn on_encoder(&mut self, ev: EncoderEvent) {
        match ev {
            EncoderEvent::Turn(delta) => {
                let v = (self.enc_value as i16 + delta as i16).clamp(0, 127) as u8;
                if v != self.enc_value {
                    self.enc_value = v;
                    let _ = self.midi_cmd.try_send(MidiCmd::ControlChange {
                        channel: self.midi_channel,
                        cc: ENCODER_CC,
                        value: v,
                    });
                }
            }
            EncoderEvent::Press => info!("router: encoder press (menu — Wave 3)"),
            EncoderEvent::Release => {}
        }
    }

    fn on_expr(&self, ev: ExprEvent) {
        let cc = EXPR_CC[(ev.pedal as usize).min(1)];
        let _ = self.midi_cmd.try_send(MidiCmd::ControlChange {
            channel: self.midi_channel,
            cc,
            value: ev.value,
        });
    }

    /// Sync toggle state (and LED feedback) from inbound device CC —
    /// bidirectional sync, so the board reflects the amp's real state.
    fn on_midi_rx(&mut self, m: MidiRx) {
        if let MidiRx::ControlChange { cc, value, .. } = m {
            let on = value > 63;
            if self.toggles[cc as usize] != on {
                self.toggles[cc as usize] = on;
                self.refresh_leds();
            }
        }
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
#[embassy_executor::task]
pub async fn router_task(
    mut r: Router,
    buttons: buttons::ButtonReceiver,
    encoder: encoder::EncoderReceiver,
    expr: expression::ExprReceiver,
    midi_rx: mux::MidiRxReceiver,
) {
    r.refresh_page(); // initial paint
    loop {
        match select4(
            buttons.receive(),
            encoder.receive(),
            expr.receive(),
            midi_rx.receive(),
        )
        .await
        {
            Either4::First(b) => r.on_button(b),
            Either4::Second(e) => r.on_encoder(e),
            Either4::Third(x) => r.on_expr(x),
            Either4::Fourth(m) => r.on_midi_rx(m),
        }
    }
}
