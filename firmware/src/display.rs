//! ST7789 240×240 TFT driver wrapper.
//!
//! Thin facade around `mipidsi::Display` that bakes in the MIDI Captain's
//! quirks so the rest of the firmware doesn't have to remember them:
//!
//! - **No display offset + `Deg0` rotation** — geometry verified on
//!   hardware 2026-05-30. The panel is physically mounted inverted in the
//!   chassis. The CircuitPython driver (`adafruit_st7789`) expresses the
//!   correction as `rotation=180` + `rowstart=80`; **do NOT copy those
//!   numbers into mipidsi.** mipidsi 0.10 has a different convention: it
//!   keeps `display_offset` constant across rotation (recomputing the
//!   address window internally), so the 240×240 window centres at
//!   `display_offset(0, 0)`, and `Rotation::Deg0` already reads upright on
//!   this inverted panel. Using mipidsi `Deg180` + offset 80 (the naive CP
//!   translation) lands the image inverted with an 80-row (⅓-screen) band
//!   of stale GRAM — the exact bug seen during bring-up.
//! - **24 MHz SPI clock**. Matches the CircuitPython firmware exactly
//!   (see `remedy/lib/display.py::_init_display`); going faster needs
//!   signal-integrity validation on the flex cable.
//! - **No reset pin**. The panel has no RST line wired to the RP2040.
//!   The controller initialises via soft-reset DCS commands (handled by
//!   `mipidsi` when `Builder::new` is called without `.reset_pin(...)`).
//!
//! The CP-side `_nvm_guarded_reset` / `release_displays` dance in
//! `remedy/lib/display.py` is **NOT needed** here: that worked around a
//! CircuitPython 10 bug where `import storage` in `boot.py` claimed the
//! SPI1 pins as a side effect. We own the hardware directly — there is
//! no analogous claim path in Rust. Don't reintroduce it.
//!
//! This module exposes just `init` and concrete type aliases. Higher-
//! level scene-graph patterns (the CP `DisplayElement` dirty-flag base
//! class, `ValueBar`, `TextPanel`) are deliberately omitted from this
//! first cut — they belong in a later layer once we have a real
//! application binary owning the redraw cadence. Callers can use
//! `embedded_graphics` `Drawable`s directly against the returned
//! `Display`.

use embassy_rp::Peri;
use embassy_rp::gpio::Output;
use embassy_rp::peripherals::SPI1;
use embassy_rp::spi::{self, Spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use mipidsi::interface::SpiInterface;
use mipidsi::models::ST7789;
use mipidsi::options::{Orientation, Rotation};
use mipidsi::{Builder, NoResetPin};
use static_cell::StaticCell;

use crate::pins;

/// SPI-byte buffer the `mipidsi` interface batches pixel data into.
/// Bigger → fewer SPI transactions per full-screen redraw. 4 KiB is a
/// pragmatic middle ground on the RP2040's 264 KiB SRAM: enough to hold
/// ~17 lines of RGB565 (`240 × 2 = 480 bytes/line`) per flush, small
/// enough to leave headroom for the rest of the firmware.
const SPI_BUFFER_BYTES: usize = 4096;

/// Concrete `embedded_hal::spi::SpiDevice` we hand to `mipidsi`.
/// `ExclusiveDevice` wraps the blocking embassy-rp SPI bus with its
/// chip-select pin so it satisfies the `SpiDevice` contract (transaction
/// = assert CS, transfer, deassert CS).
pub type Spi1Device = ExclusiveDevice<
    Spi<'static, SPI1, spi::Blocking>,
    Output<'static>,
    Delay,
>;

/// Fully-typed display the rest of the firmware sees. `RST = NoResetPin`
/// because the panel has no reset line.
pub type RemedyDisplay = mipidsi::Display<
    SpiInterface<'static, Spi1Device, Output<'static>>,
    ST7789,
    NoResetPin,
>;

/// Errors surfaced from `init`. Kept opaque on purpose — none of these
/// are recoverable at runtime; the caller just panics or boots into a
/// known-good fallback.
#[derive(Debug, defmt::Format)]
pub enum InitError {
    /// `mipidsi` rejected the size/offset combo or the controller failed
    /// to acknowledge a setup command.
    Driver,
}

/// Raw peripherals the display driver claims.
///
/// Bundled into a struct so the call site at `embassy_rp::init` reads
/// cleanly and so we can later swap `SPI1` ↔ `SPI0` without touching
/// every caller. The pins are passed as concrete embassy-rp types
/// (rather than the `u8` constants in `pins.rs`) because that's what
/// `Spi::new_blocking_txonly` and `Output::new` consume.
pub struct DisplayPeripherals {
    pub spi:       Peri<'static, SPI1>,
    pub clk:       Peri<'static, embassy_rp::peripherals::PIN_14>,
    pub mosi:      Peri<'static, embassy_rp::peripherals::PIN_15>,
    pub cs:        Peri<'static, embassy_rp::peripherals::PIN_13>,
    pub dc:        Peri<'static, embassy_rp::peripherals::PIN_12>,
    pub backlight: Peri<'static, embassy_rp::peripherals::PIN_8>,
}

/// Bring up the ST7789. Returns the initialised display *and* the
/// backlight pin (held by the caller; dropping it darkens the screen).
///
/// Blocks the executor for ~150 ms during the ST7789 init sequence —
/// call this once during boot before spawning long-running tasks.
pub fn init(peri: DisplayPeripherals) -> Result<(RemedyDisplay, Output<'static>), InitError> {
    // Backlight is plain GPIO high for now. A PWMOut wrapper lands when
    // the settings-menu "Display Brightness" item ports over.
    let backlight = Output::new(peri.backlight, embassy_rp::gpio::Level::High);

    // SPI1 in write-only mode — display has no MISO line.
    let mut spi_config = spi::Config::default();
    spi_config.frequency = pins::DISPLAY_SPI_BAUD;
    let spi_bus = Spi::new_blocking_txonly(peri.spi, peri.clk, peri.mosi, spi_config);

    // `ExclusiveDevice` glues bus + CS into an `SpiDevice`. The `Delay`
    // it holds is used for inter-transaction settling; the default
    // `embassy_time::Delay` busy-waits via the time driver.
    let cs = Output::new(peri.cs, embassy_rp::gpio::Level::High);
    let spi_device = ExclusiveDevice::new(spi_bus, cs, Delay)
        .map_err(|_| InitError::Driver)?;

    // The DCS interface batches pixel data through this static scratch
    // buffer. Must outlive the display, hence `StaticCell`.
    static SPI_BUF: StaticCell<[u8; SPI_BUFFER_BYTES]> = StaticCell::new();
    let buffer = SPI_BUF.init([0; SPI_BUFFER_BYTES]);

    let dc = Output::new(peri.dc, embassy_rp::gpio::Level::Low);
    let di = SpiInterface::new(spi_device, dc, buffer);

    let mut delay = Delay;
    let display = Builder::new(ST7789, di)
        .display_size(pins::DISPLAY_WIDTH, pins::DISPLAY_HEIGHT)
        // Geometry verified on hardware — see module header. offset (0,0)
        // centres the 240×240 window; Deg0 reads upright on this
        // chassis-inverted panel under mipidsi's convention.
        .display_offset(0, 0)
        .orientation(Orientation::new().rotate(Rotation::Deg0))
        .init(&mut delay)
        .map_err(|_| InitError::Driver)?;

    Ok((display, backlight))
}
