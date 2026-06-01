//! `display_widgets` — exercise the dirty-flag scene-graph layer.
//!
//! Renders a [`ValueBar`] that cycles 0..=127..=0 once per second and a
//! [`TextPanel`] labelled "LOW" / "MID" / "HIGH" based on the bar's
//! value band. The bar redraws every frame (its value always changes);
//! the panel only redraws when the band flips — typically twice per
//! sweep direction.
//!
//! `defmt::info!` logs each frame's draw activity so you can verify
//! gating on real hardware:
//!
//! ```text
//! [INFO ] frame=  17  value= 64  bar=DREW  panel=skipped
//! [INFO ] frame=  18  value= 67  bar=DREW  panel=skipped
//! ...
//! [INFO ] frame=  21  value= 84  bar=DREW  panel=DREW       (MID → HIGH)
//! ```
//!
//! What this proves:
//!
//! 1. The scene-graph layer renders into the ST7789 driver from
//!    [`crate::display`] without poking SPI directly.
//! 2. `Widget::render` short-circuits idempotent updates — the panel's
//!    "DREW" lines should appear only at band crossings.
//! 3. The widget API composes: two independent widgets sharing one
//!    target, no coordination beyond the per-widget dirty flag.
//!
//! Flash (no probe yet):
//! ```powershell
//! cargo run --release --example display_widgets
//! ```

#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker, Timer};
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use midicaptain_firmware::display::{self, DisplayPeripherals};
use midicaptain_firmware::ui::{Palette, TextPanel, ValueBar, Widget};
use {defmt_rtt as _, panic_probe as _};

/// 20 frames per second. Slow enough for a human to watch the sweep,
/// fast enough that the dirty-gating skip:draw ratio is obvious in the
/// log stream (bar always draws, panel only at the three band edges).
const FRAME_PERIOD_MS: u64 = 50;

fn band_label(value: u8) -> &'static str {
    match value {
        0..=41 => "LOW",
        42..=83 => "MID",
        _ => "HIGH",
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("MIDICaptain display_widgets: boot");
    let p = embassy_rp::init(Default::default());

    let (mut display, _backlight) = match display::init(DisplayPeripherals {
        spi:       p.SPI1,
        clk:       p.PIN_14,
        mosi:      p.PIN_15,
        cs:        p.PIN_13,
        dc:        p.PIN_12,
        backlight: p.PIN_8,
        pwm_slice: p.PWM_SLICE4,
    }) {
        Ok(d) => d,
        Err(e) => {
            error!("display init failed: {:?}", e);
            loop {
                Timer::after(Duration::from_secs(1)).await;
            }
        }
    };

    // Solid dark background; widgets repaint their own backgrounds so
    // this only matters for the area around them.
    display
        .clear(Rgb565::new(2, 4, 2))
        .expect("clear");

    // ValueBar: 200 × 40 px near the top, cyan fill, label "VAL".
    let mut bar = ValueBar::new(
        Point::new(20, 40),
        Size::new(200, 40),
        Palette::CYAN,
        "VAL",
        &FONT_10X20,
    );

    // TextPanel: 200 × 80 px below the bar, white-on-blue, max 6 chars
    // per line (FONT_10X20 = 10 px wide → ~18 chars fit; 6 keeps text
    // generously sized for the band labels).
    let mut panel: TextPanel<16> = TextPanel::new(
        Point::new(20, 100),
        Size::new(200, 80),
        Palette::WHITE,
        Palette::BLUE,
        &FONT_10X20,
        6,
    );

    let mut ticker = Ticker::every(Duration::from_millis(FRAME_PERIOD_MS));
    let mut frame: u32 = 0;
    let mut value: u8 = 0;
    let mut direction: i8 = 1;

    loop {
        // Triangle wave 0 → 127 → 0 → 127 ...
        let next = value as i16 + direction as i16 * 3;
        if next >= 127 {
            value = 127;
            direction = -1;
        } else if next <= 0 {
            value = 0;
            direction = 1;
        } else {
            value = next as u8;
        }

        bar.set_value(value);
        panel.set_text(band_label(value));

        let bar_drew = bar.render(&mut display).expect("bar render");
        let panel_drew = panel.render(&mut display).expect("panel render");

        info!(
            "frame={} value={} bar={} panel={}",
            frame,
            value,
            if bar_drew { "DREW" } else { "skipped" },
            if panel_drew { "DREW" } else { "skipped" },
        );

        frame = frame.wrapping_add(1);
        ticker.next().await;
    }
}
