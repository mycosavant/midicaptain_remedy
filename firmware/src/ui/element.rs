//! Dirty-flag widget trait.
//!
//! Equivalent to CP's `DisplayElement` base class
//! (`remedy/lib/display.py:130-176`) collapsed to a single trait. The
//! per-widget redraw gate is the whole point: full-frame writes to the
//! ST7789 over 24 MHz SPI cost ~38 ms each (`240 * 240 * 16 bits /
//! 24 Mbps`), so skipping redraws when state hasn't changed is the
//! difference between a responsive UI and a slideshow.
//!
//! ## Why a single trait, not `Drawable` + `DirtyTracker` split
//!
//! `embedded_graphics::Drawable` is intentionally side-effect-free: a
//! `&self` method that always draws. Our widgets are stateful — they
//! own a "last-rendered value" so they can short-circuit on idempotent
//! updates. Implementing `Drawable` would force `&self`, which forces
//! interior mutability for the dirty flag, which adds a runtime cost we
//! don't need. A purpose-built trait with `&mut self` is simpler and
//! lets the borrow checker enforce the "one widget redraws per frame"
//! discipline.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::DrawTarget;

pub trait Widget {
    /// Render this widget into `target` if any state has changed since
    /// the last `render` call. Returns `Ok(true)` if drawing actually
    /// happened, `Ok(false)` if the call was a no-op.
    ///
    /// The dirty flag clears on a successful redraw — even partial
    /// failure leaves the widget dirty so the next call retries.
    fn render<D>(&mut self, target: &mut D) -> Result<bool, D::Error>
    where
        D: DrawTarget<Color = Rgb565>;

    /// Force the next `render` call to redraw unconditionally. Use after
    /// the framebuffer has been clobbered (mode switch, screen wipe).
    fn mark_dirty(&mut self);
}
