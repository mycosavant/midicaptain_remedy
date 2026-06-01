//! Scrolling list widget for the settings menu and the config editor.
//!
//! Replaces the old one-item-at-a-time text box: shows several rows at once
//! (a title bar + up to [`VISIBLE`] rows), with the selected row highlighted
//! and an "editing" emphasis when the encoder is changing its value. Like the
//! other [`Widget`]s it dirty-tracks **per row**, so moving the cursor repaints
//! only the two rows that changed (old + new selection), not the whole screen —
//! keeping the executor stall tiny and the UI snappy.
//!
//! The caller (menu / editor) formats each visible row into a short string and
//! sends a [`crate::events::DisplayCmd::List`] snapshot; this widget diffs it
//! against what it last drew. Rows use the same bold cell font as the page grid
//! (legible at arm's length, per bench feedback) at a density of [`VISIBLE`]
//! rows per screen.

use embedded_graphics::Drawable;
use embedded_graphics::mono_font::ascii::FONT_8X13_BOLD;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use heapless::String;

use super::element::Widget;
use super::palette::{Color, Palette};
use crate::events::{ListLine, LIST_MAX_ROWS};

/// Title-bar height (matches the page grid's header).
const HEADER_H: u32 = 22;
/// Per-row height. `FONT_8X13_BOLD` is 13 px; the rest is padding.
const ROW_H: i32 = 26;
/// Y of the first row (just below the title bar).
const ROW0_Y: i32 = 24;
/// Left text inset.
const PAD_X: i32 = 8;
/// Rows visible at once: `(240 − ROW0_Y) / ROW_H`. Eight comfortably-legible
/// rows — the "6, maybe more" density target from bench feedback.
pub const VISIBLE: usize = ((240 - ROW0_Y) / ROW_H) as usize;
/// Max rows a snapshot can carry ([`crate::events::LIST_MAX_ROWS`]). Above
/// [`VISIBLE`] the list scrolls to keep the cursor in view.
pub const MAX_ROWS: usize = LIST_MAX_ROWS;

const TITLE_CAP: usize = 16;

// ── Dark theme (matches page_grid) ─────────────────────────────────────────
const BG: Color = Palette::BLACK;
/// Idle row label — bright near-white grey (legible without stealing focus).
const LABEL: Color = Color::rgb(190, 190, 190);
/// Selected-row highlight fill (cursor resting) and its brighter editing fill.
const SEL_BG: Color = Color::rgb(0, 40, 70);
const SEL_EDIT_BG: Color = Color::rgb(0, 80, 130);

pub struct ListView {
    title: String<TITLE_CAP>,
    rows: [ListLine; MAX_ROWS],
    count: usize,
    selected: usize,
    editing: bool,
    /// First visible row index (scroll offset).
    scroll: usize,
    dirty_title: bool,
    /// Per-row repaint flags, indexed by **visible slot** (`0..VISIBLE`).
    dirty_slot: [bool; VISIBLE],
}

impl ListView {
    pub fn new() -> Self {
        Self {
            title: String::new(),
            rows: core::array::from_fn(|_| String::new()),
            count: 0,
            selected: 0,
            editing: false,
            scroll: 0,
            dirty_title: true,
            dirty_slot: [true; VISIBLE],
        }
    }

    /// Adopt a fresh snapshot, flagging only what changed. `rows` is the full
    /// list; `selected` is the cursor row; `editing` brightens the cursor to
    /// signal the encoder is changing its value.
    pub fn set(&mut self, title: &str, rows: &[ListLine], selected: usize, editing: bool) {
        if self.title.as_str() != title {
            self.title.clear();
            let _ = self.title.push_str(title);
            self.dirty_title = true;
        }

        let count = rows.len().min(MAX_ROWS);
        let selected = selected.min(count.saturating_sub(1));
        // Scroll so the cursor stays on screen (only matters when count > VISIBLE).
        let scroll = if selected < self.scroll {
            selected
        } else if selected >= self.scroll + VISIBLE {
            selected + 1 - VISIBLE
        } else {
            self.scroll
        };
        // A scroll change relabels every slot → repaint all of them.
        if scroll != self.scroll {
            self.scroll = scroll;
            self.dirty_slot = [true; VISIBLE];
        }
        // Selection / editing change repaints the old + new cursor slots.
        if selected != self.selected || editing != self.editing {
            self.mark_slot(self.selected);
            self.mark_slot(selected);
        }
        // Content changes repaint their slot.
        for (i, row) in rows.iter().take(count).enumerate() {
            if self.rows[i].as_str() != row.as_str() {
                self.rows[i].clear();
                let _ = self.rows[i].push_str(row);
                self.mark_slot(i);
            }
        }
        // Rows that vanished (count shrank) clear their slot.
        if count < self.count {
            for i in count..self.count {
                self.mark_slot(i);
            }
        }
        self.count = count;
        self.selected = selected;
        self.editing = editing;
    }

    /// Mark the visible slot showing row `row_idx` dirty, if it's on screen.
    fn mark_slot(&mut self, row_idx: usize) {
        if row_idx >= self.scroll {
            let slot = row_idx - self.scroll;
            if slot < VISIBLE {
                self.dirty_slot[slot] = true;
            }
        }
    }

    fn draw_title<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        Rectangle::new(Point::new(0, 0), Size::new(240, HEADER_H))
            .into_styled(
                PrimitiveStyleBuilder::new()
                    .fill_color(Palette::AZURE.dark(5).to_rgb565())
                    .build(),
            )
            .draw(target)?;
        let style = MonoTextStyle::new(&FONT_8X13_BOLD, Palette::WHITE.to_rgb565());
        let layout = TextStyleBuilder::new()
            .alignment(Alignment::Left)
            .baseline(Baseline::Middle)
            .build();
        Text::with_text_style(
            self.title.as_str(),
            Point::new(PAD_X, (HEADER_H / 2) as i32),
            style,
            layout,
        )
        .draw(target)
        .map(|_| ())
    }

    /// Draw visible slot `slot` (`0..VISIBLE`): the row at `scroll + slot`, or a
    /// cleared band if that's past the end of the list.
    fn draw_slot<D>(&self, target: &mut D, slot: usize) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let y = ROW0_Y + (slot as i32) * ROW_H;
        let row_idx = self.scroll + slot;
        let rect = Rectangle::new(Point::new(0, y), Size::new(240, ROW_H as u32));

        if row_idx >= self.count {
            // Past the end — clear the band.
            rect.into_styled(PrimitiveStyleBuilder::new().fill_color(BG.to_rgb565()).build())
                .draw(target)?;
            return Ok(());
        }

        let selected = row_idx == self.selected;
        let (bg, fg) = if selected && self.editing {
            (SEL_EDIT_BG, Palette::WHITE)
        } else if selected {
            (SEL_BG, Palette::WHITE)
        } else {
            (BG, LABEL)
        };
        rect.into_styled(PrimitiveStyleBuilder::new().fill_color(bg.to_rgb565()).build())
            .draw(target)?;

        let style = MonoTextStyle::new(&FONT_8X13_BOLD, fg.to_rgb565());
        let layout = TextStyleBuilder::new()
            .alignment(Alignment::Left)
            .baseline(Baseline::Middle)
            .build();
        Text::with_text_style(
            self.rows[row_idx].as_str(),
            Point::new(PAD_X, y + ROW_H / 2),
            style,
            layout,
        )
        .draw(target)
        .map(|_| ())
    }
}

impl Default for ListView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ListView {
    fn render<D>(&mut self, target: &mut D) -> Result<bool, D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let mut drew = false;
        if self.dirty_title {
            self.draw_title(target)?;
            self.dirty_title = false;
            drew = true;
        }
        for slot in 0..VISIBLE {
            if self.dirty_slot[slot] {
                self.draw_slot(target, slot)?;
                self.dirty_slot[slot] = false;
                drew = true;
            }
        }
        Ok(drew)
    }

    fn mark_dirty(&mut self) {
        self.dirty_title = true;
        self.dirty_slot = [true; VISIBLE];
    }
}
