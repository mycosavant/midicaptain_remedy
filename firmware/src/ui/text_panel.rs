//! Labeled text-box widget.
//!
//! Ported from `remedy/lib/display.py:315-424`. Renders a bordered
//! rectangle with up to two centred lines of text, word-wrapped at a
//! caller-supplied character limit. The CP code uses this for the
//! current-song name in setlist mode and for transient status banners.
//!
//! Like [`super::ValueBar`], dirty-flag gating short-circuits the SPI
//! write when neither text nor colour has changed since the last render.

use embedded_graphics::Drawable;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use heapless::String;

use super::element::Widget;
use super::palette::{Color, Palette};

/// Maximum wrapped lines we render. CP hard-limits to 2; we match. Going
/// higher would require redesigning vertical centring.
const MAX_LINES: usize = 2;

/// `CAP` is the max byte length of the stored text. Pick per use site —
/// 32 fits typical song titles, 64 covers longer banner messages.
pub struct TextPanel<const CAP: usize> {
    position: Point,
    size: Size,
    text: String<CAP>,
    fg: Color,
    bg: Color,
    font: &'static MonoFont<'static>,
    max_chars_per_line: usize,
    dirty: bool,
}

impl<const CAP: usize> TextPanel<CAP> {
    pub fn new(
        position: Point,
        size: Size,
        fg: Color,
        bg: Color,
        font: &'static MonoFont<'static>,
        max_chars_per_line: usize,
    ) -> Self {
        Self {
            position,
            size,
            text: String::new(),
            fg,
            bg,
            font,
            max_chars_per_line,
            dirty: true,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replace the displayed text. Marks dirty only if the new string
    /// differs from the current one. Strings longer than `CAP` are
    /// truncated at a char boundary (well-formed for non-ASCII input).
    pub fn set_text(&mut self, s: &str) {
        if s == self.text.as_str() {
            return;
        }
        self.text.clear();
        for ch in s.chars() {
            if self.text.push(ch).is_err() {
                break;
            }
        }
        self.dirty = true;
    }

    pub fn set_fg(&mut self, c: Color) {
        if c != self.fg {
            self.fg = c;
            self.dirty = true;
        }
    }

    pub fn set_bg(&mut self, c: Color) {
        if c != self.bg {
            self.bg = c;
            self.dirty = true;
        }
    }

    /// Greedy word wrap into at most `MAX_LINES` slices, mirroring the
    /// CP `_wrap_text` logic. Returns the number of populated entries
    /// in `lines`. Long words get truncated to `max_chars_per_line` at
    /// the nearest char boundary; this keeps the panel from overflowing
    /// without allocating a separate buffer.
    fn wrap_into<'a>(&'a self, lines: &mut [&'a str; MAX_LINES]) -> usize {
        let text = self.text.as_str();
        let max = self.max_chars_per_line;

        if text.chars().count() <= max {
            lines[0] = text;
            return if text.is_empty() { 0 } else { 1 };
        }

        let mut count = 0;
        let mut line_start = 0usize;
        let mut line_chars = 0usize;
        let mut last_space: Option<usize> = None;
        let mut byte_idx = 0usize;

        for ch in text.chars() {
            let ch_len = ch.len_utf8();
            if ch == ' ' {
                last_space = Some(byte_idx);
            }
            line_chars += 1;
            byte_idx += ch_len;

            if line_chars > max {
                // Break at last_space if available, else hard break here.
                let break_at = last_space.unwrap_or(byte_idx);
                let slice_end = break_at.min(text.len());
                lines[count] = text[line_start..slice_end].trim();
                count += 1;
                if count >= MAX_LINES {
                    return count;
                }
                // Skip the space we broke on, if any.
                line_start = if last_space.is_some() {
                    (slice_end + 1).min(text.len())
                } else {
                    slice_end
                };
                line_chars = text[line_start..byte_idx].chars().count();
                last_space = None;
            }
        }

        if line_start < text.len() && count < MAX_LINES {
            lines[count] = text[line_start..].trim();
            count += 1;
        }

        count
    }
}

impl<const CAP: usize> Widget for TextPanel<CAP> {
    fn render<D>(&mut self, target: &mut D) -> Result<bool, D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        if !self.dirty {
            return Ok(false);
        }

        // Background: dark variant of bg + white 1-px outline.
        let style = PrimitiveStyleBuilder::new()
            .fill_color(self.bg.dark(3).to_rgb565())
            .stroke_color(Palette::WHITE.to_rgb565())
            .stroke_width(1)
            .build();
        Rectangle::new(self.position, self.size)
            .into_styled(style)
            .draw(target)?;

        let mut lines: [&str; MAX_LINES] = [""; MAX_LINES];
        let n = self.wrap_into(&mut lines);

        if n > 0 {
            let text_style = MonoTextStyle::new(self.font, self.fg.to_rgb565());
            let layout = TextStyleBuilder::new()
                .alignment(Alignment::Center)
                .baseline(Baseline::Middle)
                .build();

            let centre_x = self.position.x + (self.size.width as i32) / 2;
            let centre_y = self.position.y + (self.size.height as i32) / 2;
            let line_height = self.font.character_size.height as i32;
            // Two-line block: top line at centre−h/2, bottom at centre+h/2.
            // Single line: drawn at centre. Mirrors CP's anchor_point=(0.5, 0.5).
            let y_offset = if n > 1 { -line_height / 2 } else { 0 };

            for (i, line) in lines.iter().take(n).enumerate() {
                let y = centre_y + y_offset + (i as i32) * line_height;
                Text::with_text_style(line, Point::new(centre_x, y), text_style, layout)
                    .draw(target)?;
            }
        }

        self.dirty = false;
        Ok(true)
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}
