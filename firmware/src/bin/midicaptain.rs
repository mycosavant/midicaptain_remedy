//! `midicaptain` — the application binary.
//!
//! Wave-2 integration: every input/output subsystem from Wave 1 is now a
//! task feeding (or fed by) the event router, communicating only through
//! bounded `embassy_sync` channels — no shared mutable state, one owner per
//! peripheral.
//!
//! ```text
//!   buttons ─ButtonEvent─┐
//!   encoder ─EncoderEvent┤                ┌─DisplayCmd─▶ display task
//!   expr    ─ExprEvent───┼▶ router task ──┼─LedFrame───▶ leds task
//!   midi-in ─MidiRx──────┘  (app state +  ├─MidiCmd────▶ midi out (USB+DIN)
//!                            config/pages) └─SysEx──────▶ midi out (USB+DIN)
//! ```
//!
//! The [`Router`] owns the application state (active page, per-CC toggle
//! state, MIDI channel) and turns input events into actions defined by the
//! baked-in [`config`]: per-button CC / PC / SysEx / page-navigation, with
//! LED on/off feedback for toggles and a page/action readout on the display.
//! Incoming MIDI CC syncs toggle state back (bidirectional).
//!
//! Not yet here (Wave 3): settings menu, tuner mode, device sync-on-boot,
//! webapp sync. They slot in as display modes / features on this base.

#![no_std]
#![no_main]

use core::fmt::Write as _;

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::select::{select4, Either4};
use embassy_rp::adc::{Adc, Config as AdcConfig, InterruptHandler as AdcIrq};
use embassy_rp::bind_interrupts;
use embassy_rp::dma::InterruptHandler as DmaIrq;
use embassy_rp::gpio::{Input, Output, Pull};
use embassy_rp::peripherals::{DMA_CH0, PIO0, UART0, USB};
use embassy_rp::pio::{InterruptHandler as PioIrq, Pio};
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program};
use embassy_rp::uart::{
    BufferedInterruptHandler, BufferedUart, BufferedUartRx, BufferedUartTx, Config as UartConfig,
};
use embassy_rp::usb::{Driver, InterruptHandler as UsbIrq};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Duration, Instant, Ticker};
use embassy_usb::class::midi::{MidiClass, Receiver as UsbMidiRx, Sender as UsbMidiTx};
use embassy_usb::{Builder, Config as UsbConfig};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::prelude::*;
use heapless::String;
use static_cell::StaticCell;

use midicaptain_firmware::config::{self, Action, CcValue, Config, SysexCmd, DEFAULT_CONFIG};
use midicaptain_firmware::display::{self, DisplayPeripherals, RemedyDisplay};
use midicaptain_firmware::events::{
    ButtonEvent, DisplayCmd, EncoderEvent, ExprEvent, LedFrame, MidiCmd, MidiRx,
};
use midicaptain_firmware::hal::encoder::{self, Encoder};
use midicaptain_firmware::hal::expression::{self, ExpressionInputs};
use midicaptain_firmware::hal::leds::{self, LedDriver};
use midicaptain_firmware::midi::{katana, mux, sysex};
use midicaptain_firmware::pins;
use midicaptain_firmware::storage::Storage;
use midicaptain_firmware::ui::{Palette, TextPanel, Widget};
use {defmt_rtt as _, panic_probe as _};

/// Footswitch count — the 10 chassis switches (encoder push handled by the
/// encoder task, not here).
const SWITCH_COUNT: usize = 10;

/// Channel depths. Buttons are bursty (a stomp can bounce a few edges); 16
/// absorbs that. Display commands are coalesced by the router, so 8 is ample.
const BUTTON_Q: usize = 16;
const DISPLAY_Q: usize = 8;

/// Poll period for the debouncer. 5 ms × `SETTLE_SAMPLES` = settle time.
const POLL_MS: u64 = 5;
/// Consecutive stable samples required before a level change is accepted.
const SETTLE_SAMPLES: u8 = 3;

/// Press duration at/above which a release counts as a long-press.
const LONG_PRESS: Duration = Duration::from_millis(500);

/// CC the encoder drives (volume), and the per-pedal expression CCs —
/// matching `remedy/config/pages/default.toml` (`encoder.fallback.cc = 7`,
/// `pedal1.cc = 1`, `pedal2.cc = 7`). A later config extension makes these
/// per-page bindings; baked in for v1.
const ENCODER_CC: u8 = 7;
const EXPR_CC: [u8; 2] = [1, 7];

// ── Interrupt bindings (one struct for the whole app) ──────────────────
bind_interrupts!(struct Irqs {
    USBCTRL_IRQ  => UsbIrq<USB>;
    UART0_IRQ    => BufferedInterruptHandler<UART0>;
    ADC_IRQ_FIFO => AdcIrq;
    PIO0_IRQ_0   => PioIrq<PIO0>;
    DMA_IRQ_0    => DmaIrq<DMA_CH0>;
});

// ── Channel endpoints ───────────────────────────────────────────────────
type ButtonReceiver = Receiver<'static, CriticalSectionRawMutex, ButtonEvent, BUTTON_Q>;
type ButtonSender = Sender<'static, CriticalSectionRawMutex, ButtonEvent, BUTTON_Q>;
type DisplaySender = Sender<'static, CriticalSectionRawMutex, DisplayCmd, DISPLAY_Q>;
type DisplayReceiver = Receiver<'static, CriticalSectionRawMutex, DisplayCmd, DISPLAY_Q>;

type UsbDriver = Driver<'static, USB>;

// Static channels live in `.bss`; `Channel::new()` is `const`.
static BUTTON_CH: Channel<CriticalSectionRawMutex, ButtonEvent, BUTTON_Q> = Channel::new();
static DISPLAY_CH: Channel<CriticalSectionRawMutex, DisplayCmd, DISPLAY_Q> = Channel::new();
static ENC_CH: encoder::EncoderChannel = encoder::EncoderChannel::new();
static EXPR_CH: expression::ExprChannel = expression::ExprChannel::new();
static LED_CH: leds::LedChannel = leds::LedChannel::new();
static MIDI_RX: mux::MidiRxChannel = mux::MidiRxChannel::new();
static MIDI_CMD: mux::MidiCmdChannel = mux::MidiCmdChannel::new();
static SYSEX_IN: mux::SysExChannel = mux::SysExChannel::new();
static SYSEX_OUT: mux::SysExChannel = mux::SysExChannel::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("MIDICaptain app: boot");
    let p = embassy_rp::init(Default::default());

    // ── Display (sole owner of SPI1 + the ST7789) ──────────────────────
    let (disp, backlight) = display::init(DisplayPeripherals {
        spi: p.SPI1,
        clk: p.PIN_14,
        mosi: p.PIN_15,
        cs: p.PIN_13,
        dc: p.PIN_12,
        backlight: p.PIN_8,
    })
    .expect("display init");

    // ── Storage: load persisted settings (defaults on a blank device) ──
    let mut storage = Storage::new(p.FLASH);
    let settings = storage.load().await;
    info!("settings: {}", settings);
    // 1-based menu channel → 0-based wire channel for the codec.
    let wire_channel = settings.midi_channel.saturating_sub(1) & 0x0F;

    // ── LEDs (WS2812 on GP7 via PIO0 + DMA) ────────────────────────────
    // `common` / `ws_program` must outlive the driver; they live in `main`,
    // which never returns (heartbeat loop below).
    let Pio { mut common, sm0, .. } = Pio::new(p.PIO0, Irqs);
    let ws_program = PioWs2812Program::new(&mut common);
    let ws: LedDriver = PioWs2812::new(&mut common, sm0, p.DMA_CH0, Irqs, p.PIN_7, &ws_program);
    spawner.spawn(leds::leds_task(ws, LED_CH.receiver()).unwrap());

    // ── Encoder (GP2/GP3 quadrature, GP0 push) ─────────────────────────
    let encoder = Encoder::new(p.PIN_2, p.PIN_3, p.PIN_0);
    spawner.spawn(encoder::encoder_task(encoder, ENC_CH.sender()).unwrap());

    // ── Expression pedals (GP27/GP28 on the async ADC) ─────────────────
    let adc = Adc::new(p.ADC, Irqs, AdcConfig::default());
    let mut expr_inputs = ExpressionInputs::new(adc, p.PIN_27, p.PIN_28);
    for (i, cal) in settings.pedal_cal.iter().enumerate() {
        expr_inputs.set_calibration(i, cal.min, cal.max);
    }
    spawner.spawn(expression::expression_task(expr_inputs, EXPR_CH.sender()).unwrap());

    // ── MIDI: USB-MIDI device (composite-capable) + DIN UART0 ──────────
    let driver = Driver::new(p.USB, Irqs);
    let mut usb_config = UsbConfig::new(pins::USB_VID, pins::USB_PID);
    usb_config.manufacturer = Some(pins::USB_MANUFACTURER);
    usb_config.product = Some(pins::USB_PRODUCT);
    usb_config.serial_number = Some("RMDY-DEV-0001");
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = 64;

    let mut builder = {
        static CFG: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS: StaticCell<[u8; 256]> = StaticCell::new();
        static CTL: StaticCell<[u8; 64]> = StaticCell::new();
        Builder::new(
            driver,
            usb_config,
            CFG.init([0; 256]),
            BOS.init([0; 256]),
            &mut [],
            CTL.init([0; 64]),
        )
    };
    let midi_class = MidiClass::new(&mut builder, 1, 1, 64);
    let (usb_tx, usb_rx) = midi_class.split();
    let usb = builder.build();
    spawner.spawn(usb_task(usb).unwrap());

    static TX_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    static RX_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    let mut uart_config = UartConfig::default();
    uart_config.baudrate = pins::MIDI_BAUD;
    let uart = BufferedUart::new(
        p.UART0,
        p.PIN_16,
        p.PIN_17,
        Irqs,
        TX_BUF.init([0; 64]),
        RX_BUF.init([0; 64]),
        uart_config,
    );
    let (din_tx, din_rx) = uart.split();

    spawner.spawn(usb_in_task(usb_rx).unwrap());
    spawner.spawn(din_in_task(din_rx).unwrap());
    spawner.spawn(out_task(usb_tx, din_tx).unwrap());

    // ── Footswitches: active-LOW with internal pull-ups. Order here is the
    // `ButtonEvent.index` → `config::SWITCH_FOR_BUTTON` mapping. ──────────
    let buttons: [Input<'static>; SWITCH_COUNT] = [
        Input::new(p.PIN_1, Pull::Up),  // SW1
        Input::new(p.PIN_25, Pull::Up), // SW2
        Input::new(p.PIN_24, Pull::Up), // SW3
        Input::new(p.PIN_23, Pull::Up), // SW4
        Input::new(p.PIN_9, Pull::Up),  // A
        Input::new(p.PIN_10, Pull::Up), // B
        Input::new(p.PIN_11, Pull::Up), // C
        Input::new(p.PIN_18, Pull::Up), // D
        Input::new(p.PIN_20, Pull::Up), // UP
        Input::new(p.PIN_19, Pull::Up), // DOWN
    ];
    spawner.spawn(buttons_task(buttons, BUTTON_CH.sender()).unwrap());

    // ── Display task + router ──────────────────────────────────────────
    spawner.spawn(display_task(disp, backlight, DISPLAY_CH.receiver()).unwrap());

    let router = Router {
        config: DEFAULT_CONFIG,
        page: 0,
        midi_channel: wire_channel,
        enc_value: 0,
        toggles: [false; 128],
        press_at: [None; SWITCH_COUNT],
        display: DISPLAY_CH.sender(),
        leds: LED_CH.sender(),
        midi_cmd: MIDI_CMD.sender(),
        sysex_out: SYSEX_OUT.sender(),
    };
    spawner.spawn(
        router_task(
            router,
            BUTTON_CH.receiver(),
            ENC_CH.receiver(),
            EXPR_CH.receiver(),
            MIDI_RX.receiver(),
        )
        .unwrap(),
    );

    // Liveness heartbeat. Also keeps `common` / `ws_program` alive for the
    // WS2812 driver (they're consumed by reference at construction, then idle).
    let mut beat = Ticker::every(Duration::from_secs(5));
    loop {
        beat.next().await;
        info!("app: alive");
    }
}

/// Poll the footswitches, debounce, and emit edge events. (Same per-pin
/// debounce as the skeleton: a level must read stable for `SETTLE_SAMPLES`
/// consecutive polls before it's accepted, then an event fires only on a
/// net change from the last reported state.)
#[embassy_executor::task]
async fn buttons_task(buttons: [Input<'static>; SWITCH_COUNT], sender: ButtonSender) {
    let mut reported = [false; SWITCH_COUNT];
    let mut raw_prev = [false; SWITCH_COUNT];
    let mut stable = [0u8; SWITCH_COUNT];

    let mut poll = Ticker::every(Duration::from_millis(POLL_MS));
    loop {
        for i in 0..SWITCH_COUNT {
            let raw = buttons[i].is_low(); // active LOW → low == pressed
            if raw == raw_prev[i] {
                if stable[i] < SETTLE_SAMPLES {
                    stable[i] += 1;
                }
            } else {
                raw_prev[i] = raw;
                stable[i] = 0;
            }

            if stable[i] >= SETTLE_SAMPLES && reported[i] != raw {
                reported[i] = raw;
                sender
                    .send(ButtonEvent {
                        index: i as u8,
                        pressed: raw,
                    })
                    .await;
            }
        }
        poll.next().await;
    }
}

/// The event router + application state. Single owner of the page index,
/// per-CC toggle state and MIDI channel; turns input events into the
/// actions the active [`config`] page defines.
struct Router {
    config: Config,
    page: usize,
    midi_channel: u8,
    /// Accumulated encoder-driven value for [`ENCODER_CC`] (`0..=127`).
    enc_value: u8,
    /// Per-CC toggle state (on/off), indexed by CC number. Cleared on page
    /// change; synced from incoming MIDI CC.
    toggles: [bool; 128],
    /// Press timestamps for long-press detection, per switch index.
    press_at: [Option<Instant>; SWITCH_COUNT],
    display: DisplaySender,
    leds: leds::LedSender,
    midi_cmd: mux::MidiCmdSender,
    sysex_out: mux::SysExSender,
}

impl Router {
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
        if idx >= SWITCH_COUNT {
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
async fn router_task(
    mut r: Router,
    buttons: ButtonReceiver,
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

// ── MIDI transport tasks (concrete wrappers around the generic mux loops;
// embassy `#[task]`s can't be generic) ──────────────────────────────────

#[embassy_executor::task]
async fn usb_task(mut device: embassy_usb::UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

#[embassy_executor::task]
async fn usb_in_task(usb_rx: UsbMidiRx<'static, UsbDriver>) -> ! {
    mux::usb_in_loop(usb_rx, &MIDI_RX, &SYSEX_IN).await
}

#[embassy_executor::task]
async fn din_in_task(din_rx: BufferedUartRx) -> ! {
    mux::din_in_loop(din_rx, &MIDI_RX, &SYSEX_IN).await
}

#[embassy_executor::task]
async fn out_task(usb_tx: UsbMidiTx<'static, UsbDriver>, din_tx: BufferedUartTx) -> ! {
    mux::out_loop(usb_tx, din_tx, &MIDI_CMD, &SYSEX_OUT).await
}

/// Sole owner of the ST7789. Renders a page/title bar and a status panel,
/// updated on each [`DisplayCmd`].
#[embassy_executor::task]
async fn display_task(mut display: RemedyDisplay, _backlight: Output<'static>, commands: DisplayReceiver) {
    let _ = display.clear(Palette::BLACK.to_rgb565());

    // Title bar: the active page name.
    let mut title: TextPanel<16> = TextPanel::new(
        Point::new(8, 8),
        Size::new(224, 56),
        Palette::WHITE,
        Palette::AZURE,
        &FONT_10X20,
        12,
    );
    title.set_text("MIDI Captain");
    let _ = title.render(&mut display);

    // Status panel: page position, or the most recent action.
    let mut status: TextPanel<32> = TextPanel::new(
        Point::new(8, 96),
        Size::new(224, 88),
        Palette::WHITE,
        Palette::DARK_GREEN,
        &FONT_10X20,
        10,
    );
    status.set_text("booting...");
    let _ = status.render(&mut display);

    loop {
        match commands.receive().await {
            DisplayCmd::Page { name, index, total } => {
                title.set_text(name);
                let _ = title.render(&mut display);
                let mut line: String<32> = String::new();
                let _ = write!(line, "Page {}/{}", index, total);
                status.set_text(&line);
                let _ = status.render(&mut display);
            }
            DisplayCmd::Action { label, toggle, on } => {
                let mut line: String<32> = String::new();
                if toggle {
                    let _ = write!(line, "{} {}", label, if on { "ON" } else { "OFF" });
                } else {
                    let _ = write!(line, "{}", label);
                }
                status.set_text(&line);
                let _ = status.render(&mut display);
            }
        }
    }
}
