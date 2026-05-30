//! `display_splash` — bring up the ST7789 and render a static splash.
//!
//! What this proves on real hardware:
//!
//! 1. The 240×240 panel is wired correctly (SPI1 SCK/MOSI on GP14/15,
//!    CS on GP13, DC on GP12, backlight on GP8).
//! 2. The `mipidsi` `display_offset(0, 80)` + `Rotation::Deg180` combo
//!    matches the chassis orientation. Validated by drawing single
//!    pixels at the *logical* corners (0, 0) and (239, 239): if you see
//!    one pixel in the top-left and one in the bottom-right with the
//!    user facing the chassis label, geometry is correct.
//! 3. SPI clocking at 24 MHz is stable enough for full-frame writes.
//!
//! Flash (no probe yet — see `HANDOFF.md`):
//! ```powershell
//! cargo run --release --example display_splash
//! ```
//!
//! Bring the device into BOOTSEL first (hold Switch 1 at power-on, or
//! `py ..\scripts\bootsel_hammer.py` against a running `serial_echo`).

#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Text};
use midicaptain_firmware::display::{self, DisplayPeripherals};
use midicaptain_firmware::pins;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("MIDICaptain display_splash: boot");
    let p = embassy_rp::init(Default::default());

    let (mut display, _backlight) = match display::init(DisplayPeripherals {
        spi:       p.SPI1,
        clk:       p.PIN_14,
        mosi:      p.PIN_15,
        cs:        p.PIN_13,
        dc:        p.PIN_12,
        backlight: p.PIN_8,
    }) {
        Ok(d) => d,
        Err(e) => {
            error!("display init failed: {:?}", e);
            // Hold here so RTT readers can see the error.
            loop {
                Timer::after(Duration::from_secs(1)).await;
            }
        }
    };

    // Solid dark-grey background. Any non-black colour also visually
    // confirms backlight + colour-order in one frame.
    display
        .clear(Rgb565::new(4, 8, 4))
        .expect("clear");

    // Corner-pixel calibration markers. With `display_offset(0, 80)` +
    // `Rotation::Deg180` correct, these land *at* the logical corners
    // of the addressable 240×240 region from the viewer's POV.
    //
    // Red pixel at (0, 0)            ← top-left
    // Green pixel at (239, 239)      ← bottom-right
    //
    // If you see clipping, scrolling, or pixels at unexpected
    // coordinates, the offset or rotation is wrong. Don't paper over
    // it in `display.rs`; fix it there and re-flash.
    Pixel(Point::new(0, 0), Rgb565::RED)
        .draw(&mut display)
        .expect("corner pixel 0");
    Pixel(
        Point::new(
            pins::DISPLAY_WIDTH as i32 - 1,
            pins::DISPLAY_HEIGHT as i32 - 1,
        ),
        Rgb565::GREEN,
    )
    .draw(&mut display)
    .expect("corner pixel 1");

    // Thin border so the active drawable area is obvious.
    Rectangle::new(
        Point::zero(),
        Size::new(pins::DISPLAY_WIDTH.into(), pins::DISPLAY_HEIGHT.into()),
    )
    .into_styled(PrimitiveStyle::with_stroke(Rgb565::new(8, 16, 8), 1))
    .draw(&mut display)
    .expect("border");

    // Splash text, centred. Built-in monospace fonts only — porting
    // the OEM PCF (PTSans) fonts is a follow-up. FONT_10X20 maxes out
    // at "MIDICaptain Remedy" = 18 chars × 10 px = 180 px wide → fits.
    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let sub_style   = MonoTextStyle::new(&FONT_6X10,  Rgb565::new(20, 40, 20));

    let centre_x = (pins::DISPLAY_WIDTH / 2) as i32;
    let centre_y = (pins::DISPLAY_HEIGHT / 2) as i32;

    Text::with_alignment(
        "MIDICaptain Remedy",
        Point::new(centre_x, centre_y - 4),
        title_style,
        Alignment::Center,
    )
    .draw(&mut display)
    .expect("title");

    Text::with_alignment(
        "Rust + Embassy port",
        Point::new(centre_x, centre_y + 18),
        sub_style,
        Alignment::Center,
    )
    .draw(&mut display)
    .expect("subtitle");

    info!("splash rendered — entering idle");

    // Idle. Holding here keeps the display lit and the executor alive
    // for defmt-rtt to keep streaming. Pressing the encoder eventually
    // dumps to BOOTSEL via `serial_echo`'s sibling flow — not wired
    // here yet.
    loop {
        Timer::after(Duration::from_secs(60)).await;
    }
}
