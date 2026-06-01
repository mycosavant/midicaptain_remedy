//! `display_grid` — exercise the performance [`PageGrid`] widget standalone.
//!
//! Drives a fake page through every cell state so the layout, colours, and
//! dirty-flag gating can be eyeballed on the real ST7789 without the full
//! router. Once per ~600 ms it advances a demo step:
//!
//! - a radio group whose selection walks across SW1..SW4,
//! - two toggles flipping in opposition,
//! - a 3-state cycle counting `-/3 → 1/3 → 2/3 → 3/3`,
//! - a momentary cell pulsing held/idle,
//! - two plain nav cells (`BANK+`/`BANK-`) that never change — proving the
//!   per-cell skip — plus a flash on `BANK+` to demo transient feedback.
//!
//! What this proves: the widget renders into [`crate::display`] without poking
//! SPI directly, repaints only the cells that change, and the flash highlight
//! draws and clears cleanly.
//!
//! Flash (no probe): `cargo run --release --example display_grid`.

#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use embedded_graphics::prelude::*;

use midicaptain_firmware::config;
use midicaptain_firmware::display::{self, DisplayPeripherals};
use midicaptain_firmware::events::{Cell, CellState, LedColor};
use midicaptain_firmware::ui::{PageGrid, Palette, Widget};
use {defmt_rtt as _, panic_probe as _};

fn label(s: &str) -> config::Label {
    let mut l = config::Label::new();
    for ch in s.chars() {
        if l.push(ch).is_err() {
            break;
        }
    }
    l
}

fn cell(s: &str, color: LedColor, state: CellState) -> Cell {
    Cell { label: label(s), color, state }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("MIDICaptain display_grid: boot");
    let p = embassy_rp::init(Default::default());

    let (mut display, _backlight) = match display::init(DisplayPeripherals {
        spi: p.SPI1,
        clk: p.PIN_14,
        mosi: p.PIN_15,
        cs: p.PIN_13,
        dc: p.PIN_12,
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
    let _ = display.clear(Palette::BLACK.to_rgb565());

    let mut grid = PageGrid::new();
    let mut step: u32 = 0;

    loop {
        let sel = (step % 4) as u8; // radio selection walks SW1..SW4
        let tog = step.is_multiple_of(2); // toggles flip each step
        let cyc_pos = (step % 4) as u8; // 0 = unset, then 1/3, 2/3, 3/3
        let held = step.is_multiple_of(3); // momentary pulse

        let cells: [Cell; config::PAGE_BUTTONS] = [
            cell("PRE1", config::color::WHITE, CellState::Radio(sel == 0)),
            cell("PRE2", config::color::WHITE, CellState::Radio(sel == 1)),
            cell("PRE3", config::color::WHITE, CellState::Radio(sel == 2)),
            cell("PRE4", config::color::WHITE, CellState::Radio(sel == 3)),
            cell("FX1", config::color::GREEN, CellState::Toggle(tog)),
            cell("FX2", config::color::BLUE, CellState::Toggle(!tog)),
            cell("LVL", config::color::AMBER, CellState::Cycle { pos: cyc_pos, len: 3 }),
            cell("HOLD", config::color::PURPLE, CellState::Momentary(held)),
            cell("BANK+", config::color::CYAN, CellState::Plain),
            cell("BANK-", config::color::CYAN, CellState::Plain),
        ];

        grid.set_snapshot("DEMO", 1, 3, (step % 128) as u8, cells);
        let drew = grid.render(&mut display).unwrap_or(false);
        info!("step={} grid={}", step, if drew { "DREW" } else { "skipped" });

        // Demo the transient flash on BANK+ (scan index 8).
        grid.flash(8);
        let _ = grid.render(&mut display);
        Timer::after(Duration::from_millis(150)).await;
        grid.unflash(8);
        let _ = grid.render(&mut display);

        step = step.wrapping_add(1);
        Timer::after(Duration::from_millis(450)).await;
    }
}
