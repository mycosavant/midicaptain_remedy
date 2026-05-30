//! Eager, const-evaluated colour palette.
//!
//! The CircuitPython side caches dimmed/darkened variants in dictionaries
//! at runtime. Rust gets the same memoisation for free: every `dim()` /
//! `dark()` call here is `const fn`, so call sites that name them on a
//! named-colour constant fold to a single Rgb565 at compile time.
//!
//! Colours are stored in 8-bit-per-channel form (matching the CP code in
//! `remedy/lib/display.py:44-67`) and converted to `Rgb565` lazily at
//! draw time. This keeps the source legible against the CP reference and
//! lets us derive both the LED-driver representation (8-bit RGB) and the
//! display representation (5/6/5) from one source of truth.

use embedded_graphics::pixelcolor::Rgb565;

/// A linear-light 8-bit-per-channel colour. Mirrors the tuples in
/// `remedy/lib/display.py::ColorPalette::COLORS`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Compile-time RGB565 conversion. ST7789 native format.
    pub const fn to_rgb565(self) -> Rgb565 {
        // 8→5, 8→6, 8→5 bit reduction. `Rgb565::new` validates internally
        // that each channel fits the bit width.
        Rgb565::new(self.r >> 3, self.g >> 2, self.b >> 3)
    }

    /// Heavily attenuated variant — for LED idle state. Matches
    /// `ColorPalette.dim(factor=12)` in the CP code.
    pub const fn dim(self, factor: u8) -> Self {
        Self {
            r: self.r / factor,
            g: self.g / factor,
            b: self.b / factor,
        }
    }

    /// Moderately attenuated variant — for display backgrounds. Matches
    /// `ColorPalette.dark(factor=3)` in the CP code. Same operation as
    /// `dim`, exposed under a different name so call sites read clearly.
    pub const fn dark(self, factor: u8) -> Self {
        self.dim(factor)
    }
}

impl From<Color> for Rgb565 {
    fn from(c: Color) -> Self {
        c.to_rgb565()
    }
}

/// Named colours, mirrored 1:1 from
/// `remedy/lib/display.py::ColorPalette::COLORS`. Add new entries here
/// when the CP side gains them; both sides should agree.
pub struct Palette;

#[allow(dead_code)]
impl Palette {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(255, 0, 0);
    pub const GREEN: Color = Color::rgb(0, 255, 0);
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    pub const YELLOW: Color = Color::rgb(255, 255, 0);
    pub const CYAN: Color = Color::rgb(0, 255, 255);
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);
    pub const ORANGE: Color = Color::rgb(255, 128, 0);
    pub const PURPLE: Color = Color::rgb(128, 0, 255);
    pub const LIME: Color = Color::rgb(128, 255, 0);
    pub const SPRING: Color = Color::rgb(0, 255, 128);
    pub const AZURE: Color = Color::rgb(0, 128, 255);
    pub const VIOLET: Color = Color::rgb(128, 0, 255);
    pub const AMBER: Color = Color::rgb(255, 191, 0);
    pub const GREY: Color = Color::rgb(128, 128, 128);
    pub const DARK_RED: Color = Color::rgb(128, 0, 0);
    pub const DARK_GREEN: Color = Color::rgb(0, 128, 0);
    pub const DARK_BLUE: Color = Color::rgb(0, 0, 128);
    pub const DARK_YELLOW: Color = Color::rgb(128, 128, 0);
    pub const DARK_CYAN: Color = Color::rgb(0, 128, 128);
    pub const DARK_MAGENTA: Color = Color::rgb(128, 0, 128);
}
