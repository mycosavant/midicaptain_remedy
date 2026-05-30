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

use defmt::info;
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
use embassy_usb::class::midi::{MidiClass, Receiver as UsbMidiRx, Sender as UsbMidiTx};
use embassy_usb::{Builder, Config as UsbConfig};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::prelude::*;
use heapless::String;
use static_cell::StaticCell;

use midicaptain_firmware::app::{self, Router};
use midicaptain_firmware::config::DEFAULT_CONFIG;
use midicaptain_firmware::display::{self, DisplayPeripherals, RemedyDisplay};
use midicaptain_firmware::events::DisplayCmd;
use midicaptain_firmware::hal::encoder::{self, Encoder};
use midicaptain_firmware::hal::expression::{self, ExpressionInputs};
use midicaptain_firmware::hal::leds::{self, LedDriver};
use midicaptain_firmware::hal::buttons;
use midicaptain_firmware::midi::mux;
use midicaptain_firmware::pins;
use midicaptain_firmware::storage::Storage;
use midicaptain_firmware::ui::{Palette, TextPanel, Widget};
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
    let footswitches: [Input<'static>; buttons::COUNT] = [
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
    spawner.spawn(buttons::buttons_task(footswitches, BUTTON_CH.sender()).unwrap());

    // ── Display task + router ──────────────────────────────────────────
    spawner.spawn(display_task(disp, backlight, DISPLAY_CH.receiver()).unwrap());

    let router = Router::new(
        DEFAULT_CONFIG,
        wire_channel,
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

/// Sole owner of the ST7789. Renders a title bar (active page name) and a
/// status panel (page position, or the most recent action), updated on each
/// [`DisplayCmd`].
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
