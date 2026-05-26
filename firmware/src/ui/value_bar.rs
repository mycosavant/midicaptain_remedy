//! Horizontal value bar widget (0..=127, MIDI-CC scale).
//!
//! Ported from `remedy/lib/display.py:178-313`. Same visual language:
//! dim background rectangle, white 1-px outline, full-colour fill that
//! grows left-to-right with the value, and an optional centred label.
//!
//! Per-widget dirty gating: setting `value(v)` to the same `v` is a
//! no-op. The next `render` call short-circuits without touching SPI.
//! That's the design's whole reason to exist — bar widgets are the
//! single biggest expression-pedal redraw source on the CP firmware.

use embedded_graphics::Drawable;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::element::Widget;
use super::palette::{Color, Palette};

/// MIDI-CC max value. 0..=127 inclusive.
const VALUE_MAX: u8 = 127;

pub struct ValueBar {
    position: Point,
    size: Size,
    color: Color,
    value: u8,
    label: &'static str,
    font: &'static MonoFont<'static>,
    dirty: bool,
}

impl ValueBar {
    pub fn new(
        position: Point,
        size: Size,
        color: Color,
        label: &'static str,
        font: &'static MonoFont<'static>,
    ) -> Self {
        Self {
            position,
            size,
            color,
            value: 0,
            label,
            font,
            dirty: true,
        }
    }

    pub fn value(&self) -> u8 {
        self.value
    }

    /// Set the displayed value. Clamps to 0..=127. Marks dirty only if
    /// the clamped value actually differs from the current state.
    pub fn set_value(&mut self, v: u8) {
        let v = v.min(VALUE_MAX);
        if v != self.value {
            self.value = v;
            self.dirty = true;
        }
    }

    pub fn set_color(&mut self, c: Color) {
        if c != self.color {
            self.color = c;
            self.dirty = true;
        }
    }

    pub fn set_label(&mut self, label: &'static str) {
        if !core::ptr::eq(label.as_ptr(), self.label.as_ptr()) || label != self.label {
            self.label = label;
            self.dirty = true;
        }
    }

    fn fill_width(&self) -> u32 {
        // value/127 of (width - 2), with a 1-px minimum so the bar is
        // always visible. Same formula as the CP `_calculate_bar_width`.
        let inner = self.size.width.saturating_sub(2);
        let scaled = (self.value as u32 * inner) / VALUE_MAX as u32;
        scaled.max(1)
    }
}

impl Widget for ValueBar {
    fn render<D>(&mut self, target: &mut D) -> Result<bool, D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        if !self.dirty {
            return Ok(false);
        }

        // Outer: dim background + white outline. dark(factor=3) matches
        // the CP `PALETTE.dark(self._color)` call.
        let bg = self.color.dark(3).to_rgb565();
        let outline_style = PrimitiveStyleBuilder::new()
            .fill_color(bg)
            .stroke_color(Palette::WHITE.to_rgb565())
            .stroke_width(1)
            .build();
        Rectangle::new(self.position, self.size)
            .into_styled(outline_style)
            .draw(target)?;

        // Inner: full-brightness fill, 1-px inset from the outline. Height
        // = outer height − 2 (so the outline is still visible). Width
        // grows with the value.
        let inner_origin = self.position + Point::new(1, 1);
        let inner_size = Size::new(self.fill_width(), self.size.height.saturating_sub(2));
        let fill_style = PrimitiveStyleBuilder::new()
            .fill_color(self.color.to_rgb565())
            .build();
        Rectangle::new(inner_origin, inner_size)
            .into_styled(fill_style)
            .draw(target)?;

        // Centred label on top of the bar. CP draws white-on-bar; we
        // match. If the label is empty, skip the text draw entirely.
        if !self.label.is_empty() {
            let centre = Point::new(
                self.position.x + (self.size.width as i32) / 2,
                self.position.y + (self.size.height as i32) / 2,
            );
            let text_style = MonoTextStyle::new(self.font, Palette::WHITE.to_rgb565());
            let layout = TextStyleBuilder::new()
                .alignment(Alignment::Center)
                .baseline(Baseline::Middle)
                .build();
            Text::with_text_style(self.label, centre, text_style, layout).draw(target)?;
        }

        self.dirty = false;
        Ok(true)
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}
