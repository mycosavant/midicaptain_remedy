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
    /// Structural redraw needed: first render, colour/label change, or an
    /// explicit `mark_dirty` after the framebuffer was clobbered. Repaints
    /// the whole widget (outline + fill + label).
    full_dirty: bool,
    /// Value changed since last render — only the fill *delta* needs
    /// painting, never the full background. This is what eliminates the
    /// per-frame flicker: growing paints the new segment in fg, shrinking
    /// clears the vacated segment to bg.
    value_dirty: bool,
    /// Fill width (px) painted on the last render, for delta computation.
    last_fill_w: u32,
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
            full_dirty: true,
            value_dirty: false,
            last_fill_w: 0,
        }
    }

    pub fn value(&self) -> u8 {
        self.value
    }

    /// Set the displayed value. Clamps to 0..=127. Flags a value-only
    /// update (cheap delta paint) when the clamped value actually changes.
    pub fn set_value(&mut self, v: u8) {
        let v = v.min(VALUE_MAX);
        if v != self.value {
            self.value = v;
            self.value_dirty = true;
        }
    }

    /// Colour change forces a structural redraw (fg + bg both move).
    pub fn set_color(&mut self, c: Color) {
        if c != self.color {
            self.color = c;
            self.full_dirty = true;
        }
    }

    /// Label change forces a structural redraw.
    pub fn set_label(&mut self, label: &'static str) {
        if label != self.label {
            self.label = label;
            self.full_dirty = true;
        }
    }

    fn fill_width(&self) -> u32 {
        // value/127 of (width - 2), with a 1-px minimum so the bar is
        // always visible. Same formula as the CP `_calculate_bar_width`.
        let inner = self.size.width.saturating_sub(2);
        let scaled = (self.value as u32 * inner) / VALUE_MAX as u32;
        scaled.max(1)
    }

    /// Repaint the centred label. Called after any fill change, since the
    /// moving fill can overwrite the glyphs. Text is glyph-only (no
    /// background box), so repainting in place does not flicker.
    fn draw_label<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        if self.label.is_empty() {
            return Ok(());
        }
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
        Ok(())
    }
}

impl Widget for ValueBar {
    fn render<D>(&mut self, target: &mut D) -> Result<bool, D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        if !self.full_dirty && !self.value_dirty {
            return Ok(false);
        }

        let fg = self.color.to_rgb565();
        // dark(factor=3) matches the CP `PALETTE.dark(self._color)` call.
        let bg = self.color.dark(3).to_rgb565();
        let inner_x = self.position.x + 1;
        let inner_y = self.position.y + 1;
        let inner_h = self.size.height.saturating_sub(2);
        let new_w = self.fill_width();

        if self.full_dirty {
            // Outline + dim background, drawn once per structural change.
            Rectangle::new(self.position, self.size)
                .into_styled(
                    PrimitiveStyleBuilder::new()
                        .fill_color(bg)
                        .stroke_color(Palette::WHITE.to_rgb565())
                        .stroke_width(1)
                        .build(),
                )
                .draw(target)?;

            // Full fill, 1-px inset so the outline stays visible.
            Rectangle::new(Point::new(inner_x, inner_y), Size::new(new_w, inner_h))
                .into_styled(PrimitiveStyleBuilder::new().fill_color(fg).build())
                .draw(target)?;

            self.draw_label(target)?;

            self.last_fill_w = new_w;
            self.full_dirty = false;
            self.value_dirty = false;
            return Ok(true);
        }

        // Value-only update: paint just the delta strip. No full-area
        // repaint → no flicker.
        if new_w == self.last_fill_w {
            // Sub-pixel change: value moved but the fill rounds to the same
            // width. Nothing visible to draw.
            self.value_dirty = false;
            return Ok(false);
        }

        if new_w > self.last_fill_w {
            // Grew: paint the newly-revealed segment in fg.
            Rectangle::new(
                Point::new(inner_x + self.last_fill_w as i32, inner_y),
                Size::new(new_w - self.last_fill_w, inner_h),
            )
            .into_styled(PrimitiveStyleBuilder::new().fill_color(fg).build())
            .draw(target)?;
        } else {
            // Shrank: clear the vacated segment back to bg.
            Rectangle::new(
                Point::new(inner_x + new_w as i32, inner_y),
                Size::new(self.last_fill_w - new_w, inner_h),
            )
            .into_styled(PrimitiveStyleBuilder::new().fill_color(bg).build())
            .draw(target)?;
        }

        self.draw_label(target)?;

        self.last_fill_w = new_w;
        self.value_dirty = false;
        Ok(true)
    }

    fn mark_dirty(&mut self) {
        self.full_dirty = true;
    }
}
