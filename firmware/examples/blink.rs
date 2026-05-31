//! `blink` — cycle the first NeoPixel in the chain through red → green →
//! blue. Confirms three things at once:
//!
//! 1. The chip is alive (we got past `embassy_rp::init`).
//! 2. The PIO + DMA WS2812 driver works against GP7.
//! 3. The Embassy executor is ticking under us.
//!
//! Build/flash (no probe): hold BOOTSEL on the MIDI Captain while applying
//! USB, then `cargo run --example blink`. `elf2uf2-rs -d` will drop the
//! UF2 onto the mounted RPI-RP2 drive and the device will reboot into
//! this binary.
//!
//! The chain has 30 LEDs (`NEOPIXEL_COUNT`). We only set LED 0 here so
//! the difference between "first LED working" and "whole strip working"
//! is visible without sunglasses.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::dma::InterruptHandler as DmaIrq;
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::pio::{InterruptHandler as PioIrq, Pio};
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program};
use embassy_time::{Duration, Ticker};
use midicaptain_firmware::pins;
use smart_leds::RGB8;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => PioIrq<PIO0>;
    DMA_IRQ_0  => DmaIrq<DMA_CH0>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("MIDICaptain blink: boot");
    let p = embassy_rp::init(Default::default());

    // PIO0 + state machine 0 drives the WS2812 chain. DMA_CH0 streams
    // 24-bit RGB words at the WS2812 line rate (~800 kHz).
    let Pio { mut common, sm0, .. } = Pio::new(p.PIO0, Irqs);

    let program = PioWs2812Program::new(&mut common);
    let mut ws: PioWs2812<'_, PIO0, 0, { pins::NEOPIXEL_COUNT }, _> =
        PioWs2812::new(
            &mut common,
            sm0,
            p.DMA_CH0,
            Irqs,
            p.PIN_7, // pins::NEOPIXEL_PIN
            &program,
        );

    // Whole-strip frame buffer; we only touch index 0, but the driver
    // clocks out the full chain on every write, so the unused LEDs need
    // to be explicitly zeroed (RGB8::default == black).
    let mut frame = [RGB8::default(); pins::NEOPIXEL_COUNT];

    let mut tick = Ticker::every(Duration::from_millis(333));
    let palette = [
        RGB8 { r: 32, g: 0,  b: 0  }, // dim red — full brightness blinds
        RGB8 { r: 0,  g: 32, b: 0  }, // dim green
        RGB8 { r: 0,  g: 0,  b: 32 }, // dim blue
    ];

    let mut step: usize = 0;
    loop {
        frame[0] = palette[step % palette.len()];
        ws.write(&frame).await;
        info!(
            "tick {} → ({}, {}, {})",
            step, frame[0].r, frame[0].g, frame[0].b
        );
        step = step.wrapping_add(1);
        tick.next().await;
    }
}
