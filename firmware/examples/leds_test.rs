//! `leds_test` — exercise the `hal::leds` task end-to-end.
//!
//! Proves the LED subsystem the way it will actually be driven in the app:
//! a producer pushes [`LedFrame`]s into a channel, and the spawned
//! [`leds_task`] owns the WS2812 chain and renders them. This is the
//! `examples/blink.rs` driver setup plus the channel + task plumbing from
//! `ARCHITECTURE.md`.
//!
//! What you should see on hardware: a single switch lit at a time, walking
//! the chain S1 → … → DOWN and cycling red → green → blue. After each full
//! lap, the whole board glows very faintly for one beat — that's the
//! [`idle_dim`](leds::idle_dim) helper applied across every switch,
//! demonstrating the toggled-off brightness level.
//!
//! Build/flash (no probe): hold BOOTSEL while applying USB, then
//! `cargo run --release --example leds_test`. `elf2uf2-rs -d` drops the UF2.
//!
//! CURRENT SAFETY: every colour here keeps all channels ≤ 32, and only one
//! switch (three pixels) is lit at a time during the chase. Do not raise
//! these levels — 30 pixels at full brightness draws over an amp.

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
use midicaptain_firmware::events::{LedColor, LedFrame};
use midicaptain_firmware::hal::leds::{self, leds_task, LedDriver};
use midicaptain_firmware::pins;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => PioIrq<PIO0>;
    DMA_IRQ_0  => DmaIrq<DMA_CH0>;
});

/// Per-channel test ceiling. Kept ≤ 32 so the strip never pulls dangerous
/// current — see the module-level CURRENT SAFETY note.
const LEVEL: u8 = 24;

/// All-off colour and frame, used as the base each chase step builds on.
const OFF: LedColor = LedColor { r: 0, g: 0, b: 0 };
const BLANK: LedFrame = LedFrame {
    switches: [OFF; pins::Switch::COUNT],
};

/// Three dim primaries to cycle through, all within [`LEVEL`].
const PALETTE: [LedColor; 3] = [
    LedColor { r: LEVEL, g: 0, b: 0 },
    LedColor { r: 0, g: LEVEL, b: 0 },
    LedColor { r: 0, g: 0, b: LEVEL },
];

/// A frame with exactly one switch lit.
fn one_lit(switch: usize, color: LedColor) -> LedFrame {
    let mut frame = BLANK;
    frame.switches[switch] = color;
    frame
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("leds_test: boot");
    let p = embassy_rp::init(Default::default());

    // WS2812 driver on GP7, identical setup to examples/blink.rs. `common`
    // and `program` must outlive the driver; they live here in `main`,
    // which loops forever below, so they are effectively 'static.
    let Pio { mut common, sm0, .. } = Pio::new(p.PIO0, Irqs);
    let program = PioWs2812Program::new(&mut common);
    let ws: LedDriver = PioWs2812::new(
        &mut common,
        sm0,
        p.DMA_CH0,
        Irqs,
        p.PIN_7, // pins::NEOPIXEL_PIN
        &program,
    );

    // The LED task owns the driver for the rest of the program.
    spawner.spawn(leds_task(ws, LED_CH.receiver()).unwrap());
    let frames = LED_CH.sender();

    // ~150 ms/step makes the chase comfortably visible.
    let mut tick = Ticker::every(Duration::from_millis(150));
    let mut step: usize = 0;
    loop {
        let switch = step % pins::Switch::COUNT;
        let color = PALETTE[step % PALETTE.len()];

        // Deterministic test → block-the-producer so every step renders.
        // The production router instead uses `try_send` (drop-newest) for
        // these time-sensitive frames, per ARCHITECTURE.md rule 2.
        frames.send(one_lit(switch, color)).await;
        info!(
            "leds_test: lit switch {} -> ({}, {}, {})",
            switch, color.r, color.g, color.b
        );
        tick.next().await;

        // End of a lap: show the idle-dim level across the whole board.
        if switch == pins::Switch::COUNT - 1 {
            let base = LedColor {
                r: LEVEL,
                g: LEVEL,
                b: LEVEL,
            };
            let idle = leds::idle_dim(base);
            frames
                .send(LedFrame {
                    switches: [idle; pins::Switch::COUNT],
                })
                .await;
            info!(
                "leds_test: idle wash ({}, {}, {}) = idle_dim of ({}, {}, {})",
                idle.r, idle.g, idle.b, base.r, base.g, base.b
            );
            tick.next().await;
        }

        step = step.wrapping_add(1);
    }
}

/// Frame channel: the example's `main` produces, [`leds_task`] consumes.
static LED_CH: leds::LedChannel = leds::LedChannel::new();
