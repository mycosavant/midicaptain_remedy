//! Chromatic-tuner view widget.
//!
//! Ported from `remedy/lib/tuner.py::TunerDisplay`, adapted to
//! embedded-graphics primitives. Shows three things, colour-coded by how far
//! off-pitch the note is:
//!
//! - the **note name + octave** (e.g. `A4`, `C#3`), large and centred;
//! - the **cents deviation** as text (`+12c`, `-7c`);
//! - a **needle bar** — a fixed track with a white centre tick and a coloured
//!   needle that swings left (flat) / right (sharp) of centre.
//!
//! Colour follows the CP thresholds: green in-tune (`|c| ≤ 3`), yellow close
//! (`|c| ≤ 10`), red sharp, blue flat, grey when no note is detected.
//!
//! Dirty-gating, like the other widgets: a `render` with the same note and
//! cents is a no-op. The common case — only the cents changed while the same
//! note rings — delta-paints just the needle and the cents text, so the
//! needle tracks the guitar without flicker or a full-screen redraw.

use core::fmt::Write as _;

use embedded_graphics::Drawable;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use heapless::String;

use super::element::Widget;
use super::palette::{Color, Palette};

/// Note names (sharps), indexed by `note % 12`. Mirrors
/// `tuner.py::NOTE_NAMES_SHARP`.
const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// In-tune / close thresholds in cents (CP `IN_TUNE_THRESHOLD` / `CLOSE`).
const IN_TUNE: u16 = 3;
const CLOSE: u16 = 10;

/// Cents at the ends of the bar. Beyond this the needle pins to the edge.
const BAR_RANGE: i32 = 50;

// Geometry (240×240 panel).
const NOTE_CENTRE: Point = Point::new(120, 56);
const NOTE_BOX: Rectangle = Rectangle::new(Point::new(28, 36), Size::new(184, 40));
const CENTS_CENTRE: Point = Point::new(120, 104);
const CENTS_BOX: Rectangle = Rectangle::new(Point::new(28, 86), Size::new(184, 36));

const BAR_X: i32 = 20;
const BAR_Y: i32 = 150;
const BAR_W: i32 = 200;
const BAR_H: i32 = 30;
const NEEDLE_W: i32 = 6;

/// Track background — a dark grey the coloured needle reads against.
const TRACK_BG: Color = Color::rgb(28, 28, 28);

pub struct TunerView {
    font: &'static MonoFont<'static>,
    /// Target state (set by the router via [`Self::set`]).
    note: Option<u8>,
    cents: i16,
    /// Last-rendered state, for change detection.
    rendered_note_text: String<6>,
    rendered_cents: i16,
    rendered_color: Color,
    /// Left edge of the needle on the last paint (for delta erase).
    last_needle_left: i32,
    /// Force a full structural repaint (first render / after a screen wipe).
    full_dirty: bool,
}

impl TunerView {
    pub fn new(font: &'static MonoFont<'static>) -> Self {
        Self {
            font,
            note: None,
            cents: 0,
            rendered_note_text: String::new(),
            rendered_cents: 0,
            rendered_color: Palette::GREY,
            last_needle_left: 0,
            full_dirty: true,
        }
    }

    /// Update the readout. Cheap — change detection happens in `render`.
    pub fn set(&mut self, note: Option<u8>, cents: i16) {
        self.note = note;
        self.cents = cents;
    }

    /// Note name + octave for the current note (`--` when none). MIDI octave
    /// convention: note 60 = `C4`, so octave = `note / 12 − 1`.
    fn note_text(&self) -> String<6> {
        let mut s = String::new();
        match self.note {
            None => {
                let _ = s.push_str("--");
            }
            Some(n) => {
                let _ = s.push_str(NOTE_NAMES[(n % 12) as usize]);
                let _ = write!(s, "{}", (n as i16) / 12 - 1);
            }
        }
        s
    }

    /// Colour for the current note + cents (grey when no note).
    fn color(&self) -> Color {
        if self.note.is_none() {
            return Palette::GREY;
        }
        let a = self.cents.unsigned_abs();
        if a <= IN_TUNE {
            Palette::GREEN
        } else if a <= CLOSE {
            Palette::YELLOW
        } else if self.cents > 0 {
            Palette::RED
        } else {
            Palette::BLUE
        }
    }

    /// Left edge (px) of the needle for a cents value, clamped to the track.
    fn needle_left(cents: i16) -> i32 {
        let c = (cents as i32).clamp(-BAR_RANGE, BAR_RANGE);
        let lo = BAR_X + 1 + NEEDLE_W / 2;
        let hi = BAR_X + BAR_W - 1 - NEEDLE_W / 2;
        let centre = lo + ((c + BAR_RANGE) * (hi - lo)) / (2 * BAR_RANGE);
        centre - NEEDLE_W / 2
    }

    fn fill<D>(target: &mut D, rect: Rectangle, color: Color) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        rect.into_styled(PrimitiveStyleBuilder::new().fill_color(color.to_rgb565()).build())
            .draw(target)
    }

    fn draw_centre_tick<D>(target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let cx = BAR_X + BAR_W / 2;
        Self::fill(
            target,
            Rectangle::new(Point::new(cx - 1, BAR_Y + 1), Size::new(2, (BAR_H - 2) as u32)),
            Palette::WHITE,
        )
    }

    fn draw_needle<D>(&self, target: &mut D, left: i32, color: Color) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        Self::fill(
            target,
            Rectangle::new(Point::new(left, BAR_Y + 1), Size::new(NEEDLE_W as u32, (BAR_H - 2) as u32)),
            color,
        )
    }

    fn draw_text<D>(&self, target: &mut D, text: &str, centre: Point, color: Color) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let style = MonoTextStyle::new(self.font, color.to_rgb565());
        let layout = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Middle)
            .build();
        Text::with_text_style(text, centre, style, layout).draw(target)?;
        Ok(())
    }
}

impl Widget for TunerView {
    fn render<D>(&mut self, target: &mut D) -> Result<bool, D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let note_text = self.note_text();
        let color = self.color();
        let note_changed = note_text != self.rendered_note_text;
        let color_changed = color != self.rendered_color;
        let cents_changed = self.cents != self.rendered_cents;

        if !self.full_dirty && !note_changed && !color_changed && !cents_changed {
            return Ok(false);
        }

        // A full repaint is needed whenever the note name or colour changes,
        // because every element is drawn in `color` and the text glyphs don't
        // clear their own background (a colour change would otherwise ghost).
        let full = self.full_dirty || note_changed || color_changed;

        // Cents text always rebuilt; the box is cleared first so the previous
        // value is removed (text is glyph-only).
        let mut cents_text: String<6> = String::new();
        if self.note.is_some() {
            let c = self.cents;
            let _ = write!(cents_text, "{}{}c", if c > 0 { "+" } else { "" }, c);
        }

        if full {
            // Note name (cleared box → fresh glyph in the new colour).
            Self::fill(target, NOTE_BOX, Palette::BLACK)?;
            self.draw_text(target, &note_text, NOTE_CENTRE, color)?;

            // Cents text.
            Self::fill(target, CENTS_BOX, Palette::BLACK)?;
            if !cents_text.is_empty() {
                self.draw_text(target, &cents_text, CENTS_CENTRE, color)?;
            }

            // Track: dark fill + white outline.
            Rectangle::new(Point::new(BAR_X, BAR_Y), Size::new(BAR_W as u32, BAR_H as u32))
                .into_styled(
                    PrimitiveStyleBuilder::new()
                        .fill_color(TRACK_BG.to_rgb565())
                        .stroke_color(Palette::WHITE.to_rgb565())
                        .stroke_width(1)
                        .build(),
                )
                .draw(target)?;
            Self::draw_centre_tick(target)?;

            let left = Self::needle_left(self.cents);
            self.draw_needle(target, left, color)?;
            self.last_needle_left = left;
        } else {
            // Fast path: same note + colour, only cents moved. Redraw the
            // cents text and delta-paint the needle.
            Self::fill(target, CENTS_BOX, Palette::BLACK)?;
            if !cents_text.is_empty() {
                self.draw_text(target, &cents_text, CENTS_CENTRE, color)?;
            }

            let left = Self::needle_left(self.cents);
            if left != self.last_needle_left {
                // Erase the old needle, restore the centre tick (the old
                // needle may have covered it), then draw at the new position.
                self.draw_needle(target, self.last_needle_left, TRACK_BG)?;
                Self::draw_centre_tick(target)?;
                self.draw_needle(target, left, color)?;
                self.last_needle_left = left;
            }
        }

        self.rendered_note_text = note_text;
        self.rendered_cents = self.cents;
        self.rendered_color = color;
        self.full_dirty = false;
        Ok(true)
    }

    fn mark_dirty(&mut self) {
        self.full_dirty = true;
    }
}
