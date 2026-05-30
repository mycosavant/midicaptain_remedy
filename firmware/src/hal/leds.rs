//! `leds` — the WS2812 NeoPixel task.
//!
//! Sole owner of the single-wire LED chain on `GP7`. It consumes
//! [`LedFrame`]s off a bounded channel and clocks them out via PIO0 + DMA,
//! exactly the driver pattern proven in `examples/blink.rs`.
//!
//! ## Frame → pixel mapping
//!
//! A [`LedFrame`] carries one [`LedColor`] per footswitch — 10 entries, in
//! [`pins::Switch::ALL`] order. The chassis wires **three** physical
//! WS2812 pixels under each switch, so this module fans each colour across
//! that switch's contiguous run of three pixels (see [`pins::LED_RANGES`] /
//! [`pins::Switch::led_range`]). The result is the 30-pixel buffer the
//! driver streams in chain order. This mirrors `LED_MAP` in
//! `remedy/lib/pins.py`.
//!
//! ## Brightness
//!
//! The CircuitPython firmware shows a button at full colour when its
//! toggle is **on** and at a dimmed level when **idle/off** (see
//! `remedy/main.py::update_leds` and `ColorPalette.dim` in
//! `remedy/lib/display.py`). We keep that policy where it belongs — in the
//! event router, which decides per switch whether to send the base colour
//! or its dimmed variant — and expose [`idle_dim`] / [`dim`] as the helper
//! that does the dimming. The LED task itself renders whatever colour it
//! receives, faithfully; it never second-guesses the frame.
//!
//! ## Current safety
//!
//! 30 pixels at full white is well over an amp — far past what the USB
//! bus or the chassis wiring wants to source. Callers are responsible for
//! keeping aggregate brightness sane; the idle-dim helper exists partly so
//! that the resting state (most LEDs idle) draws very little. Test code
//! must keep every channel ≤ 32.

use embassy_rp::peripherals::PIO0;
use embassy_rp::pio_programs::ws2812::{Grb, PioWs2812};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender};
use smart_leds::RGB8;

use crate::events::{LedColor, LedFrame};
use crate::pins;

/// Channel depth for inbound LED frames. Frames are time-sensitive and
/// idempotent — the newest fully describes the desired state — so a
/// producer that finds the queue full should `try_send` and drop, per
/// `ARCHITECTURE.md` rule 2. Two slots absorb a render in flight plus the
/// next pending frame.
pub const LED_Q: usize = 2;

/// The LED frame channel type. Declare one as a `static` in the binary:
/// `static LED_CH: leds::LedChannel = leds::LedChannel::new();`
pub type LedChannel = Channel<CriticalSectionRawMutex, LedFrame, LED_Q>;

/// Producer handle — held by the event router.
pub type LedSender = Sender<'static, CriticalSectionRawMutex, LedFrame, LED_Q>;

/// Consumer handle — held by [`leds_task`].
pub type LedReceiver = Receiver<'static, CriticalSectionRawMutex, LedFrame, LED_Q>;

/// Concrete WS2812 driver for our chain: PIO0, state machine 0, all 30
/// pixels, default GRB byte order. The DMA channel is type-erased inside
/// the driver, so it does not appear in this alias — the binary picks the
/// channel (`DMA_CH0` in the example) when it constructs the driver.
pub type LedDriver = PioWs2812<'static, PIO0, 0, { pins::NEOPIXEL_COUNT }, Grb>;

/// Divisor matching `ColorPalette.dim(factor=12)` in
/// `remedy/lib/display.py` — the reference dim level for an idle LED.
pub const IDLE_DIM_FACTOR: u8 = 12;

/// Convert a channel-contract [`LedColor`] to the driver's [`RGB8`].
#[inline]
pub const fn to_rgb8(c: LedColor) -> RGB8 {
    RGB8 {
        r: c.r,
        g: c.g,
        b: c.b,
    }
}

/// Dim a colour by integer division on each channel, mirroring CP's
/// `PALETTE.dim`: `c / factor`. A `factor` of 0 is clamped to 1 (identity)
/// so this can never divide by zero.
#[inline]
pub const fn dim(c: LedColor, factor: u8) -> LedColor {
    let f = if factor == 0 { 1 } else { factor };
    LedColor {
        r: c.r / f,
        g: c.g / f,
        b: c.b / f,
    }
}

/// Dim a colour to its resting/toggled-off level using [`IDLE_DIM_FACTOR`].
/// This is what the router applies to a switch whose toggle is off.
#[inline]
pub const fn idle_dim(c: LedColor) -> LedColor {
    dim(c, IDLE_DIM_FACTOR)
}

/// Expand a 10-entry [`LedFrame`] into the 30-pixel WS2812 buffer.
///
/// Each switch's colour fills its three contiguous pixels. `frame.switches`
/// and [`pins::Switch::ALL`] share the same order, so they zip directly;
/// [`pins::Switch::led_range`] gives the `(start, count)` slice per switch.
pub fn expand(frame: &LedFrame) -> [RGB8; pins::NEOPIXEL_COUNT] {
    let mut buf = [RGB8::default(); pins::NEOPIXEL_COUNT];
    for (color, switch) in frame.switches.iter().zip(pins::Switch::ALL) {
        let (start, count) = switch.led_range();
        buf[start..start + count].fill(to_rgb8(*color));
    }
    buf
}

/// Drive the WS2812 chain.
///
/// Blanks the strip on entry, then renders each [`LedFrame`] as it arrives.
/// The driver is constructed in the binary (it owns the PIO/DMA interrupt
/// binding) and moved in here; `common`/`program` stay alive in the
/// binary's `main`, which never returns.
#[embassy_executor::task]
pub async fn leds_task(mut driver: LedDriver, frames: LedReceiver) {
    // Power-on state: all pixels off, so a half-written strip from a prior
    // boot doesn't linger.
    driver.write(&[RGB8::default(); pins::NEOPIXEL_COUNT]).await;

    loop {
        let frame = frames.receive().await;
        let buf = expand(&frame);
        driver.write(&buf).await;
    }
}
