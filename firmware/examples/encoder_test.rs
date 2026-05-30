//! `encoder_test` — exercise the rotary encoder driver on real hardware.
//!
//! Stands up [`midicaptain_firmware::hal::encoder::Encoder`] on the board's
//! encoder GPIOs and logs every event over defmt RTT:
//!
//! - each detent of rotation, with the (acceleration-scaled) signed delta
//!   and a running absolute position, and
//! - each debounced push-button press / release.
//!
//! This is the proof-of-life for the encoder workstream: no display, LEDs
//! or MIDI involved, just the decoder and the log.
//!
//! ## Run it
//!
//! With a probe wired to the SWD pads (see `HARDWARE.md`), switch the
//! runner in `.cargo/config.toml` to `probe-rs run --chip RP2040` and:
//!
//! ```text
//! cargo run --release --example encoder_test
//! ```
//!
//! …then turn the knob and press it; the RTT console prints each event.
//! Without a probe the default `elf2uf2-rs -d` runner flashes the UF2 but
//! you won't see the RTT log — a probe is the point of this example.
//!
//! ## Expected
//!
//! - One `Turn` line per click of the detent (not four — sub-detent
//!   quadrature is absorbed inside the driver).
//! - Spinning fast bumps `delta` to ±2 / ±4 (acceleration); slow, careful
//!   clicks stay at ±1.
//! - Clean `PRESS` / `RELEASE` pairs with no bounce chatter.
//! - If the knob counts backwards, the A/B wiring sign is flipped — see
//!   the direction-sign note in `hal/encoder.rs`.

#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use midicaptain_firmware::events::EncoderEvent;
use midicaptain_firmware::hal::encoder::Encoder;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("encoder_test: boot");
    let p = embassy_rp::init(Default::default());

    // Pins per `pins.rs`: ENCODER_A_PIN = 2, ENCODER_B_PIN = 3,
    // ENCODER_SW_PIN = 0. Acceleration stays on (the default) so fast
    // spins are visible in the delta.
    let mut encoder = Encoder::new(p.PIN_2, p.PIN_3, p.PIN_0);

    info!("encoder_test: turn the knob and press it");

    // Running detent position, purely so the log shows cumulative travel.
    let mut position: i32 = 0;

    loop {
        match encoder.next_event().await {
            EncoderEvent::Turn(delta) => {
                position += delta as i32;
                let dir = if delta > 0 { "CW " } else { "CCW" };
                info!("encoder: {} delta={} position={}", dir, delta, position);
            }
            EncoderEvent::Press => info!("encoder: PRESS"),
            EncoderEvent::Release => info!("encoder: RELEASE"),
        }
    }
}
