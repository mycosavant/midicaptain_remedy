//! Pin map for the Paint Audio MIDI Captain.
//!
//! Mirrors `remedy/lib/pins.py` from the CircuitPython reference firmware.
//! When the OEM PCB silkscreens diverge from the reverse-engineered map,
//! update this file (and `HARDWARE.md`) — every other module routes
//! through here.
//!
//! Conventions:
//! - Footswitches: active LOW with internal pull-up. `Level::Low` means
//!   "pressed". The CP firmware drove these with `digitalio.Pull.UP` and
//!   read `False` on press; Embassy's `Input` with `Pull::Up` is identical.
//! - Display is mounted upside-down in the chassis. Initialize the
//!   `mipidsi` driver with `Orientation::Portrait(true).flip_horizontal()`
//!   or rotate by 180° when the display module lands.
//!
//! This module deliberately holds only constants and small type aliases —
//! no peripheral construction. Examples take `embassy_rp::init(...)` and
//! reach for `Peripherals::PIN_xx` directly, using these constants as
//! pin-number documentation. A later refactor may introduce
//! `assign-resources!`-style typed bundles per task.

#![allow(dead_code)]

// ── Footswitches ───────────────────────────────────────────────────────
// "Numbered" bottom row. SW1 doubles as the boot-mode detect pin in the
// CP firmware; with our own bootloader we may repurpose this — keep it as
// an input for now.
pub const SW_1_PIN:    u8 = 1;
pub const SW_2_PIN:    u8 = 25;
pub const SW_3_PIN:    u8 = 24;
pub const SW_4_PIN:    u8 = 23;

// "Lettered" top row.
pub const SW_A_PIN:    u8 = 9;
pub const SW_B_PIN:    u8 = 10;
pub const SW_C_PIN:    u8 = 11;
pub const SW_D_PIN:    u8 = 18;

// Navigation pair.
pub const SW_UP_PIN:   u8 = 20;
pub const SW_DOWN_PIN: u8 = 19;

/// Total physical footswitches (excluding the encoder push-button).
pub const FOOTSWITCH_COUNT: usize = 10;

// ── Rotary encoder ─────────────────────────────────────────────────────
pub const ENCODER_A_PIN:  u8 = 2;  // Quadrature phase A
pub const ENCODER_B_PIN:  u8 = 3;  // Quadrature phase B
pub const ENCODER_SW_PIN: u8 = 0;  // Push-button (active LOW)

// ── NeoPixels (WS2812B, single chain) ──────────────────────────────────
pub const NEOPIXEL_PIN:   u8    = 7;
pub const NEOPIXEL_COUNT: usize = 30; // 10 switches × 3 LEDs each

/// Switch label (parallel array index for [`LED_RANGES`] and the typed
/// pin map at the bottom of this module).
///
/// Order is the order the LEDs appear in the WS2812 daisy chain, NOT the
/// physical layout on the chassis. See `remedy/lib/pins.py::LED_MAP`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Switch {
    S1, S2, S3, S4,
    Up,
    A, B, C, D,
    Down,
}

impl Switch {
    pub const COUNT: usize = 10;

    /// Iterator-friendly array in chain order.
    pub const ALL: [Switch; Self::COUNT] = [
        Switch::S1, Switch::S2, Switch::S3, Switch::S4,
        Switch::Up,
        Switch::A,  Switch::B,  Switch::C,  Switch::D,
        Switch::Down,
    ];

    /// Index into the NeoPixel chain for the first LED of this switch.
    /// Each switch owns three contiguous LEDs.
    pub const fn led_start(self) -> usize {
        (self as usize) * 3
    }

    /// (start, count) range of NeoPixel indices for this switch.
    pub const fn led_range(self) -> (usize, usize) {
        (self.led_start(), 3)
    }
}

/// LED-index ranges parallel to [`Switch::ALL`], in chain order.
/// Equivalent to the `LED_MAP` dict in the CP reference.
pub const LED_RANGES: [(usize, usize); Switch::COUNT] = [
    (0, 3),   // S1
    (3, 3),   // S2
    (6, 3),   // S3
    (9, 3),   // S4
    (12, 3),  // Up
    (15, 3),  // A
    (18, 3),  // B
    (21, 3),  // C
    (24, 3),  // D
    (27, 3),  // Down
];

// ── ST7789 240×240 TFT ────────────────────────────────────────────────
// On RP2040, GP14/GP15 are SPI1 SCK/TX. CS/DC are plain GPIO outputs.
// Backlight is PWM-driven (CP used pwmio at default freq); plain GPIO
// works as well if you don't need dimming.
pub const SPI_CLK_PIN:    u8 = 14;  // SPI1 SCK
pub const SPI_MOSI_PIN:   u8 = 15;  // SPI1 TX  (display is write-only)
pub const TFT_CS_PIN:     u8 = 13;
pub const TFT_DC_PIN:     u8 = 12;
pub const TFT_BACKLIGHT_PIN: u8 = 8; // PWM-capable

pub const DISPLAY_WIDTH:    u16  = 240;
pub const DISPLAY_HEIGHT:   u16  = 240;
// Panel is physically chassis-inverted, but mipidsi 0.10's rotation/offset
// convention differs from CircuitPython's: verified on hardware that
// Rotation::Deg0 + display_offset(0,0) reads upright and centred. (CP's
// adafruit_st7789 uses rotation=180 + rowstart=80 for the same result — do
// not copy those numbers into mipidsi.) See display.rs module header.
pub const DISPLAY_ROTATION_DEG: u16 = 0;
pub const DISPLAY_SPI_BAUD: u32  = 24_000_000;

// ── DIN MIDI (5-pin) over UART0 ────────────────────────────────────────
// GP16 = UART0 TX, GP17 = UART0 RX (RP2040 alt-function 2). Standard
// MIDI baud is exactly 31250.
pub const MIDI_TX_PIN: u8  = 16;
pub const MIDI_RX_PIN: u8  = 17;
pub const MIDI_BAUD:   u32 = 31_250;

// ── Expression pedals (TRS, tip = wiper) ──────────────────────────────
// ADC1 (GP27) and ADC2 (GP28). Calibration min/max live in flash; the CP
// firmware put them in NVM because its filesystem was read-only.
pub const EXPRESSION_1_PIN: u8 = 27;
pub const EXPRESSION_2_PIN: u8 = 28;

// Optional battery-voltage divider on ADC3 (GP29). Present on some
// boards, absent on others — code that uses this must tolerate it being
// unimplemented.
pub const BATTERY_VOLTAGE_PIN: u8 = 29;

/// ADC reference (RP2040 internal Vref ≈ 3.3 V).
pub const ADC_REF_VOLTAGE: f32 = 3.3;

// ── Reserved / unused GPIOs ───────────────────────────────────────────
// GP4, GP5, GP6, GP21, GP22, GP26. Available for future expansion.

// ── USB device identity ───────────────────────────────────────────────
// The Raspberry Pi vendor ID. KEEP THIS — scripts/bootsel_hammer.py
// matches devices by VID, and any change here breaks the recovery flow.
pub const USB_VID: u16 = 0x2E8A;
/// Development-time PID. Pick a value outside the Pico SDK's well-known
/// range to avoid confusing host udev rules / drivers. Update when this
/// firmware ships under a stable identity.
pub const USB_PID: u16 = 0x102D;
pub const USB_MANUFACTURER: &str = "Paint Audio";
pub const USB_PRODUCT:      &str = "MIDICaptain Remedy (Rust)";
