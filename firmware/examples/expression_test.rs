//! `expression_test` — bring up the two expression pedals and stream their
//! mapped `0..=127` values over defmt RTT.
//!
//! Wiring (see `pins.rs` / `HARDWARE.md`):
//! - Pedal 0: TRS tip → GP27 (ADC1)
//! - Pedal 1: TRS tip → GP28 (ADC2)
//! - Sleeve → GND, ring → 3.3 V (so the wiper sweeps the full ADC range)
//!
//! What you should see: with a probe attached (`cargo run --release
//! --example expression_test`), sweeping a pedal heel-to-toe prints a
//! stream like `pedal 0 -> 0 … 64 … 127`. Values appear *only when they
//! change* — hold the pedal still and RTT goes quiet, proving the
//! dirty-flag gating and smoothing. An idle/unconnected ADC input floats,
//! so an unplugged jack may emit a little noise near one end; that's
//! expected.
//!
//! This exercises the real [`expression_task`] and its [`ExprEvent`]
//! channel exactly as the application binary will — the task is the
//! producer, this `main` loop stands in for the router as the consumer.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::adc::{Adc, Config as AdcConfig, InterruptHandler as AdcInterruptHandler};
use embassy_rp::bind_interrupts;
use midicaptain_firmware::events::ExprEvent;
use midicaptain_firmware::hal::expression::{expression_task, ExprChannel, ExpressionInputs};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    ADC_IRQ_FIFO => AdcInterruptHandler;
});

/// The sampler → consumer channel. In the app this lives in the binary and
/// its receiver goes to the router; here `main` drains it directly.
static EXPR_CH: ExprChannel = ExprChannel::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("expression_test: boot");
    let p = embassy_rp::init(Default::default());

    // Async ADC, IRQ-driven (no busy-wait between conversions).
    let adc = Adc::new(p.ADC, Irqs, AdcConfig::default());

    // GP27 = pedal 0 (ADC1), GP28 = pedal 1 (ADC2). Calibration is left at
    // the 0..=4095 default — sweep the full pedal travel to hit 0 and 127.
    let inputs = ExpressionInputs::new(adc, p.PIN_27, p.PIN_28);

    spawner.spawn(expression_task(inputs, EXPR_CH.sender()).unwrap());

    // Stand in for the router: log every changed pedal value.
    let receiver = EXPR_CH.receiver();
    loop {
        let ExprEvent { pedal, value } = receiver.receive().await;
        info!("pedal {} -> {}", pedal, value);
    }
}
