//! Scene-graph layer above the ST7789 driver.
//!
//! Mirrors the role of `remedy/lib/display.py` in the CircuitPython
//! firmware: a small set of dirty-flag widgets the application composes
//! to build the on-device UI. The driver in [`crate::display`] stays a
//! thin facade over `mipidsi`; everything user-facing lives here.
//!
//! ## Design choices that differ from the CP code
//!
//! - **No `DisplayManager` scene tree.** CP needs `displayio.Group` to
//!   piggy-back on the framework's render pump. embedded-graphics has no
//!   such tree — `Drawable::draw` writes directly to the target. A
//!   "scene" is just whichever widgets the app chooses to `render()`
//!   this frame; mode-switching is app state, not display layering.
//! - **Eager const colours.** CP lazily caches `dim()` / `dark()`
//!   variants in dictionaries. We compute Rgb565 constants and
//!   `const fn` helpers at compile time. Same memoisation, zero RAM cost.
//! - **`heapless::String` for text.** No allocator on the device. Each
//!   text-bearing widget owns a fixed-capacity buffer sized to its use
//!   case (song title, value readout, etc.). Capacity is a const generic.
//! - **`Widget::render` returns `bool`.** `true` = redrew this frame,
//!   `false` = skipped because nothing changed. Application loops can
//!   instrument this directly with `defmt::info!`.

pub mod element;
pub mod palette;
pub mod text_panel;
pub mod tuner;
pub mod value_bar;

pub use element::Widget;
pub use palette::{Color, Palette};
pub use text_panel::TextPanel;
pub use tuner::TunerView;
pub use value_bar::ValueBar;
