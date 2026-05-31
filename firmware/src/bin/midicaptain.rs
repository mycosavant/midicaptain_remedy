//! `midicaptain` — the application binary.
//!
//! Wave-2 integration, now as thin wiring: construct every peripheral and
//! spawn the tasks that make up the graph in `ARCHITECTURE.md`. The
//! application logic lives in [`app::Router`]; the input/output subsystems
//! live in `hal::*` and `midi::*`. Everything communicates only through
//! bounded `embassy_sync` channels — one owner per peripheral.
//!
//! ```text
//!   buttons ─ButtonEvent─┐
//!   encoder ─EncoderEvent┤                ┌─DisplayCmd─▶ display task
//!   expr    ─ExprEvent───┼▶ router task ──┼─LedFrame───▶ leds task
//!   midi-in ─MidiRx──────┘  (app::Router) ├─MidiCmd────▶ midi out (USB+DIN)
//!                                          └─SysEx──────▶ midi out (USB+DIN)
//! ```
//!
//! The bin keeps only the two hardware-owning glue tasks whose peripherals
//! are constructed here: the display task (sole owner of the ST7789) and the
//! MIDI transport wrappers (concrete instances of the generic `midi::mux`
//! loops — embassy `#[task]`s can't be generic).
//!
//! Not yet here (Wave 3): settings menu, tuner mode, device sync-on-boot,
//! webapp sync. They slot in as display modes / features on this base.

#![no_std]
#![no_main]

use core::fmt::Write as _;

use defmt::{info, warn};
use embassy_executor::Spawner;
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
use embassy_usb::class::cdc_acm::{CdcAcmClass, State as CdcState};
use embassy_usb::class::midi::{MidiClass, Receiver as UsbMidiRx, Sender as UsbMidiTx};
use embassy_usb::{Builder, Config as UsbConfig};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::prelude::*;
use heapless::String;
use static_cell::StaticCell;

use midicaptain_firmware::app::{self, Router};
use midicaptain_firmware::config;
use midicaptain_firmware::display::{self, DisplayPeripherals, RemedyDisplay};
use midicaptain_firmware::events::{CalStep, DisplayCmd, MenuKind};
use midicaptain_firmware::hal::encoder::{self, Encoder};
use midicaptain_firmware::hal::expression::{self, ExpressionInputs};
use midicaptain_firmware::hal::leds::{self, LedDriver};
use midicaptain_firmware::hal::buttons;
use midicaptain_firmware::midi::mux;
use midicaptain_firmware::pins;
use midicaptain_firmware::proto;
use midicaptain_firmware::storage::{self, Storage};
use midicaptain_firmware::ui::{Palette, TextPanel, TunerView, Widget};
use {defmt_rtt as _, panic_probe as _};

type UsbDriver = Driver<'static, USB>;

// ── Interrupt bindings (one struct for the whole app) ──────────────────
bind_interrupts!(struct Irqs {
    USBCTRL_IRQ  => UsbIrq<USB>;
    UART0_IRQ    => BufferedInterruptHandler<UART0>;
    ADC_IRQ_FIFO => AdcIrq;
    PIO0_IRQ_0   => PioIrq<PIO0>;
    DMA_IRQ_0    => DmaIrq<DMA_CH0>;
});

// ── Static channels (live in `.bss`; `Channel::new()` is const) ─────────
static BUTTON_CH: buttons::ButtonChannel = buttons::ButtonChannel::new();
static DISPLAY_CH: app::DisplayChannel = app::DisplayChannel::new();
static ENC_CH: encoder::EncoderChannel = encoder::EncoderChannel::new();
static EXPR_CH: expression::ExprChannel = expression::ExprChannel::new();
static LED_CH: leds::LedChannel = leds::LedChannel::new();
static MIDI_RX: mux::MidiRxChannel = mux::MidiRxChannel::new();
static MIDI_CMD: mux::MidiCmdChannel = mux::MidiCmdChannel::new();
static SYSEX_IN: mux::SysExChannel = mux::SysExChannel::new();
static SYSEX_OUT: mux::SysExChannel = mux::SysExChannel::new();
// Config-sync request/response between the CDC task and the router.
static CONFIG_REQ_CH: app::ConfigReqChannel = app::ConfigReqChannel::new();
static CONFIG_RESP_CH: app::ConfigRespChannel = app::ConfigRespChannel::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("MIDICaptain app: boot");
    let p = embassy_rp::init(Default::default());

    // ── Display (sole owner of SPI1 + the ST7789) ──────────────────────
    // Bring the screen up and spawn its task FIRST, before the slower init
    // (storage, USB, …). The panel then shows "booting…" immediately, so any
    // later hang is visible on-screen instead of leaving it black. A display
    // failure is non-fatal: log it and run headless — MIDI, LEDs and the
    // footswitches don't depend on the screen, so a dead panel (e.g. a loose
    // flex cable) must not brick the controller.
    match display::init(DisplayPeripherals {
        spi: p.SPI1,
        clk: p.PIN_14,
        mosi: p.PIN_15,
        cs: p.PIN_13,
        dc: p.PIN_12,
        backlight: p.PIN_8,
    }) {
        Ok((disp, backlight)) => {
            spawner.spawn(display_task(disp, backlight, DISPLAY_CH.receiver()).unwrap());
        }
        Err(e) => warn!("display init failed ({:?}); running headless", e),
    }

    // ── Footswitches: active-LOW with internal pull-ups. Created here (not
    // at spawn time) so a boot-time recovery combo can be read before
    // settings load. Order defines each switch's `ButtonEvent.index` →
    // `config::SWITCH_FOR_BUTTON` LED mapping. ───────────────────────────
    let footswitches: [Input<'static>; buttons::COUNT] = [
        Input::new(p.PIN_1, Pull::Up),  // SW1
        Input::new(p.PIN_25, Pull::Up), // SW2
        Input::new(p.PIN_24, Pull::Up), // SW3
        Input::new(p.PIN_23, Pull::Up), // SW4
        Input::new(p.PIN_9, Pull::Up),  // A
        Input::new(p.PIN_10, Pull::Up), // B
        Input::new(p.PIN_11, Pull::Up), // C
        Input::new(p.PIN_18, Pull::Up), // D
        Input::new(p.PIN_20, Pull::Up), // UP   (index 8)
        Input::new(p.PIN_19, Pull::Up), // DOWN (index 9)
    ];

    // ── Storage: load persisted settings (defaults on a blank device) ──
    let mut storage = Storage::new(p.FLASH);
    // Recovery hatch: hold UP+DOWN during power-on to wipe persisted settings
    // *and* the stored user config back to factory defaults — `factory_reset`
    // erases the whole flash KV map (clears a bad MIDI channel, pedal
    // calibration, or a pushed config that bricks the UI). Checked before load
    // so the wipe takes effect this boot.
    //
    // The footswitch inputs were created just above with internal pull-ups; the
    // lines take a moment to charge high after `Input::new`, so an immediate
    // read can return a spurious LOW that looks like the combo and wipes
    // everything on a normal boot (observed on hardware — "factory reset" fired
    // with nothing held). Guard against it two ways: let the pull-ups settle
    // first, then require the combo to be *held* — sample twice ~50 ms apart and
    // reset only if both samples agree. A genuine hold passes; a power-on
    // transient does not.
    let combo_held = || footswitches[8].is_low() && footswitches[9].is_low();
    embassy_time::Timer::after_millis(5).await; // let the pull-ups settle
    if combo_held() {
        embassy_time::Timer::after_millis(50).await; // confirm it's a real hold
        if combo_held() {
            warn!("factory reset: UP+DOWN held at boot — erasing settings + config");
            let _ = storage.factory_reset().await;
        } else {
            info!("boot: UP+DOWN transient at power-on, not a hold — no factory reset");
        }
    }
    let settings = storage.load().await;
    info!("settings: {}", settings);

    // ── LEDs (WS2812 on GP7 via PIO0 + DMA) ────────────────────────────
    // `common` / `ws_program` must outlive the driver; they live in `main`,
    // which never returns (heartbeat loop below).
    let Pio { mut common, sm0, .. } = Pio::new(p.PIO0, Irqs);
    let ws_program = PioWs2812Program::new(&mut common);
    let ws: LedDriver = PioWs2812::new(&mut common, sm0, p.DMA_CH0, Irqs, p.PIN_7, &ws_program);
    spawner.spawn(leds::leds_task(ws, LED_CH.receiver()).unwrap());

    // ── Encoder (GP2/GP3 quadrature, GP0 push) ─────────────────────────
    let enc = Encoder::new(p.PIN_2, p.PIN_3, p.PIN_0);
    spawner.spawn(encoder::encoder_task(enc, ENC_CH.sender()).unwrap());

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
        // Config descriptor holds MIDI + CDC interfaces (composite), so it is
        // sized generously to avoid a build-time descriptor overflow.
        static CFG: StaticCell<[u8; 512]> = StaticCell::new();
        static BOS: StaticCell<[u8; 256]> = StaticCell::new();
        static CTL: StaticCell<[u8; 64]> = StaticCell::new();
        Builder::new(
            driver,
            usb_config,
            CFG.init([0; 512]),
            BOS.init([0; 256]),
            &mut [],
            CTL.init([0; 64]),
        )
    };
    let midi_class = MidiClass::new(&mut builder, 1, 1, 64);
    let (usb_tx, usb_rx) = midi_class.split();
    // USB-CDC (ACM) for webapp ↔ device config sync — composite alongside MIDI.
    let cdc_class = {
        static CDC_STATE: StaticCell<CdcState> = StaticCell::new();
        CdcAcmClass::new(&mut builder, CDC_STATE.init(CdcState::new()), 64)
    };
    let usb = builder.build();
    spawner.spawn(usb_task(usb).unwrap());
    spawner.spawn(
        cdc_task(cdc_class, CONFIG_REQ_CH.sender(), CONFIG_RESP_CH.receiver()).unwrap(),
    );

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

    // ── Footswitch scanner (array created above, for the boot recovery combo) ──
    spawner.spawn(buttons::buttons_task(footswitches, BUTTON_CH.sender()).unwrap());

    // ── User config: load from flash (falls back to the baked default on a
    // blank/corrupt device). The scratch buffer is too large to keep inside
    // `Storage` for the program's lifetime, so it lives here — and is then
    // handed to the router, which reuses it to persist configs pushed over USB
    // (a second 8 KB array would be wasteful). ──────────────────────────────
    static CONFIG_SCRATCH: StaticCell<[u8; storage::CONFIG_SCRATCH_LEN]> = StaticCell::new();
    let config_scratch = CONFIG_SCRATCH.init([0; storage::CONFIG_SCRATCH_LEN]);
    let config = storage.load_config(&mut config_scratch[..]).await;
    info!("config: {} page(s)", config.page_count());

    // ── Router (last: it depends on settings + config loaded above, and
    // paints the first page over the display task's "booting…" splash) ──────
    let router = Router::new(
        config,
        settings,
        storage,
        config_scratch,
        DISPLAY_CH.sender(),
        LED_CH.sender(),
        MIDI_CMD.sender(),
        SYSEX_OUT.sender(),
    );
    spawner.spawn(
        app::router_task(
            router,
            BUTTON_CH.receiver(),
            ENC_CH.receiver(),
            EXPR_CH.receiver(),
            MIDI_RX.receiver(),
            CONFIG_REQ_CH.receiver(),
            CONFIG_RESP_CH.sender(),
        )
        .unwrap(),
    );

    // Liveness heartbeat. Also keeps `common` / `ws_program` alive for the
    // WS2812 driver (consumed by reference at construction, then idle).
    let mut beat = embassy_time::Ticker::every(embassy_time::Duration::from_secs(5));
    loop {
        beat.next().await;
        info!("app: alive");
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

// ── USB-CDC config-sync endpoint ────────────────────────────────────────
// Serves the config-sync protocol over CDC: HELLO (link probe), GET_CONFIG
// (read the live config), SET_CONFIG (validate → persist → hot-reload). The
// wire codec is `proto`; the live config and flash live in the router, reached
// over the `config_req`/`config_resp` channels (this task is the sole client,
// so it sends a request then awaits exactly one reply before reading on).

/// Owns the CDC-ACM endpoints. Reassembles COBS frames (host bytes delimited
/// by `0x00`), decodes each via [`proto`], and replies — routing GET/SET to the
/// router over the config channels.
#[embassy_executor::task]
async fn cdc_task(
    mut cdc: CdcAcmClass<'static, UsbDriver>,
    config_req: app::ConfigReqSender,
    config_resp: app::ConfigRespReceiver,
) -> ! {
    static ACC: StaticCell<[u8; proto::MAX_FRAME_LEN]> = StaticCell::new();
    static BODY: StaticCell<[u8; proto::MAX_BODY]> = StaticCell::new();
    static OUT: StaticCell<[u8; proto::MAX_FRAME_LEN]> = StaticCell::new();
    static PAYLOAD: StaticCell<[u8; proto::MAX_PAYLOAD]> = StaticCell::new();
    let acc = ACC.init([0; proto::MAX_FRAME_LEN]);
    let body = BODY.init([0; proto::MAX_BODY]);
    let out = OUT.init([0; proto::MAX_FRAME_LEN]);
    let payload = PAYLOAD.init([0; proto::MAX_PAYLOAD]);
    let mut pkt = [0u8; 64];

    loop {
        cdc.wait_connection().await;
        info!("cdc: host connected");
        let mut acc_len = 0usize;
        loop {
            let n = match cdc.read_packet(&mut pkt).await {
                Ok(n) => n,
                Err(_) => break, // host closed / bus reset → await reconnect
            };
            for &b in &pkt[..n] {
                if b == 0 {
                    handle_cdc_frame(
                        &acc[..acc_len],
                        &mut cdc,
                        &config_req,
                        &config_resp,
                        body,
                        payload,
                        out,
                    )
                    .await;
                    acc_len = 0;
                } else if acc_len < acc.len() {
                    acc[acc_len] = b;
                    acc_len += 1;
                } else {
                    acc_len = 0; // frame too long → resync at the next delimiter
                }
            }
        }
    }
}

/// A decoded, owned request — detached from the `body` decode buffer so that
/// buffer is free to reassemble the reply.
// `Set` owns a whole `RuntimeConfig` (~2.4 KB); the other variants are tiny.
// It's a short-lived stack local (one per frame), and `no_std` has no allocator
// to box into, so the spread is intrinsic and harmless here.
#[allow(clippy::large_enum_variant)]
enum Incoming {
    /// Link probe → reply with the protocol version.
    Hello,
    /// Read the live config from the router.
    Get,
    /// Replace the live config (already deserialized + checked non-empty).
    Set(config::RuntimeConfig),
    /// Reject with this code: bad opcode, or an un-parseable/empty config.
    Bad(proto::ProtoError),
}

/// Decode one frame, act on it, and reply. `body` is the COBS decode scratch
/// (reused to assemble the reply once the request is an owned [`Incoming`]);
/// `payload` serializes the GET-config blob; `out` holds the encoded reply.
async fn handle_cdc_frame(
    frame: &[u8],
    cdc: &mut CdcAcmClass<'static, UsbDriver>,
    config_req: &app::ConfigReqSender,
    config_resp: &app::ConfigRespReceiver,
    body: &mut [u8],
    payload: &mut [u8],
    out: &mut [u8],
) {
    if frame.is_empty() {
        return;
    }
    // Decode the frame; for SET, deserialize + validate the config now — while
    // `body` still holds the decoded payload — into an owned `Incoming`, so
    // `body` is free to reuse for the reply once the borrow ends.
    let (seq, incoming) = {
        let f = match proto::decode(frame, body) {
            Ok(f) => f,
            Err(e) => {
                warn!("cdc: bad frame ({})", e);
                return; // no seq to echo → drop silently
            }
        };
        let inc = match f.cmd {
            proto::cmd::HELLO => Incoming::Hello,
            proto::cmd::GET_CONFIG => Incoming::Get,
            proto::cmd::SET_CONFIG => match config::deserialize(f.payload) {
                Ok(cfg) if cfg.page_count() > 0 => Incoming::Set(cfg),
                _ => Incoming::Bad(proto::ProtoError::BadPayload),
            },
            other => {
                warn!("cdc: unhandled cmd {=u8}", other);
                Incoming::Bad(proto::ProtoError::BadCommand)
            }
        };
        (f.seq, inc)
    };

    match incoming {
        Incoming::Hello => {
            reply(cdc, proto::cmd::HELLO, seq, &[proto::PROTO_VERSION], body, out).await;
        }
        Incoming::Get => {
            config_req.send(app::ConfigReq::Get).await;
            match config_resp.receive().await {
                app::ConfigResp::Config(cfg) => match config::serialize(&cfg, payload) {
                    Ok(blob) => reply(cdc, proto::cmd::GET_CONFIG, seq, blob, body, out).await,
                    Err(e) => {
                        warn!("cdc: config serialize failed ({})", e);
                        error_reply(cdc, seq, proto::ProtoError::StoreFailed, body, out).await;
                    }
                },
                // The router only ever answers Get with Config; anything else
                // is an internal error — relay it rather than panic.
                _ => error_reply(cdc, seq, proto::ProtoError::StoreFailed, body, out).await,
            }
        }
        Incoming::Set(cfg) => {
            config_req.send(app::ConfigReq::Set(cfg)).await;
            match config_resp.receive().await {
                app::ConfigResp::Ok => {
                    reply(cdc, proto::cmd::SET_CONFIG, seq, &[], body, out).await
                }
                app::ConfigResp::Err(e) => error_reply(cdc, seq, e, body, out).await,
                app::ConfigResp::Config(_) => {
                    error_reply(cdc, seq, proto::ProtoError::StoreFailed, body, out).await
                }
            }
        }
        Incoming::Bad(e) => error_reply(cdc, seq, e, body, out).await,
    }
}

/// Encode `(cmd, seq, payload)` into `out` (scratch `body`) and write the frame.
async fn reply(
    cdc: &mut CdcAcmClass<'static, UsbDriver>,
    cmd: u8,
    seq: u8,
    payload: &[u8],
    body: &mut [u8],
    out: &mut [u8],
) {
    match proto::encode(cmd, seq, payload, body, out) {
        Ok(n) => write_cdc_frame(cdc, &out[..n]).await,
        Err(e) => warn!("cdc: encode failed ({})", e),
    }
}

/// Reply with `ERROR(code)`, echoing the request `seq`.
async fn error_reply(
    cdc: &mut CdcAcmClass<'static, UsbDriver>,
    seq: u8,
    code: proto::ProtoError,
    body: &mut [u8],
    out: &mut [u8],
) {
    reply(cdc, proto::cmd::ERROR, seq, &[code as u8], body, out).await;
}

/// Write a full frame to the host, chunked to the CDC max packet size. The
/// trailing `0x00` delimiter is part of `frame`, so the host parses by
/// delimiter regardless of packetisation.
async fn write_cdc_frame(cdc: &mut CdcAcmClass<'static, UsbDriver>, frame: &[u8]) {
    for chunk in frame.chunks(64) {
        if cdc.write_packet(chunk).await.is_err() {
            return; // disconnected mid-write
        }
    }
}

/// Which on-screen layout the display task last painted. The normal layout
/// (title + status panels) and the tuner layout occupy different regions, so a
/// switch between them wipes the screen first — otherwise the old layout's
/// pixels would linger around the new one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Normal,
    Tuner,
}

/// Sole owner of the ST7789. Renders either the normal layout — a title bar
/// (active page name) and a status panel (page position / latest action /
/// menu) — or the tuner layout, switching on each [`DisplayCmd`].
#[embassy_executor::task]
async fn display_task(mut display: RemedyDisplay, _backlight: Output<'static>, commands: app::DisplayReceiver) {
    let _ = display.clear(Palette::BLACK.to_rgb565());

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

    let mut tuner = TunerView::new(&FONT_10X20);
    let mut screen = Screen::Normal;

    loop {
        let cmd = commands.receive().await;

        // Layout transition: wipe the screen and force the incoming layout's
        // widgets to repaint from scratch.
        let want = match cmd {
            DisplayCmd::Tuner { .. } => Screen::Tuner,
            _ => Screen::Normal,
        };
        if want != screen {
            let _ = display.clear(Palette::BLACK.to_rgb565());
            match want {
                Screen::Normal => {
                    title.mark_dirty();
                    status.mark_dirty();
                }
                Screen::Tuner => tuner.mark_dirty(),
            }
            screen = want;
        }

        match cmd {
            DisplayCmd::Page { name, index, total } => {
                title.set_text(&name);
                let _ = title.render(&mut display);
                let mut line: String<32> = String::new();
                let _ = write!(line, "Page {}/{}", index, total);
                status.set_text(&line);
                let _ = status.render(&mut display);
            }
            DisplayCmd::Action { label, toggle, on } => {
                let mut line: String<32> = String::new();
                if toggle {
                    let _ = write!(line, "{} {}", label.as_str(), if on { "ON" } else { "OFF" });
                } else {
                    let _ = write!(line, "{}", label.as_str());
                }
                status.set_text(&line);
                let _ = status.render(&mut display);
            }
            DisplayCmd::Menu { title: item, value, kind, editing } => {
                title.set_text("SETTINGS");
                let _ = title.render(&mut display);
                let marker = if editing { "*" } else { ">" };
                let mut line: String<32> = String::new();
                let _ = match kind {
                    MenuKind::Int => write!(line, "{} {}: {}", marker, item, value),
                    MenuKind::Percent => write!(line, "{} {}: {}%", marker, item, value),
                    MenuKind::Action => write!(line, "> {} (press)", item),
                };
                status.set_text(&line);
                let _ = status.render(&mut display);
            }
            DisplayCmd::Cal { pedal, step, raw } => {
                title.set_text("CALIBRATE");
                let _ = title.render(&mut display);
                let mut line: String<32> = String::new();
                let _ = match step {
                    CalStep::Min => write!(line, "P{} set MIN, SW  ({})", pedal + 1, raw),
                    CalStep::Max => write!(line, "P{} set MAX, SW  ({})", pedal + 1, raw),
                    CalStep::Done => write!(line, "P{} saved!", pedal + 1),
                };
                status.set_text(&line);
                let _ = status.render(&mut display);
            }
            DisplayCmd::Tuner { note, cents } => {
                tuner.set(note, cents);
                let _ = tuner.render(&mut display);
            }
        }
    }
}
