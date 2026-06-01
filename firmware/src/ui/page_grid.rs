//! Performance-screen page grid — the headline on-device widget.
//!
//! Draws a labelled grid of the ten footswitches that mirrors the chassis
//! layout, with each cell showing its label, colour, and live state (toggle
//! on/off, radio selection, multi-state cycle position, momentary hold). It is
//! the on-screen counterpart to the WS2812 LEDs, but adds what LEDs can't: the
//! button *labels* and the *cycle step index* (`2/3`).
//!
//! ## Why this is its own widget (and not ten `TextPanel`s)
//!
//! Like the other [`Widget`]s it owns its last-drawn state so it can
//! short-circuit. The win here is **per-cell** dirty tracking: a full grid
//! repaint is ~most of the screen (≈35 ms over 24 MHz SPI, which *blocks the
//! executor*), but a single toggle press only dirties one cell (~3 ms). The
//! router may send a whole [`DisplayCmd::Page`] snapshot on every change; this
//! widget diffs it against what it last drew and repaints only the cells that
//! actually moved. That keeps the executor stall proportional to the change.
//!
//! [`DisplayCmd::Page`]: crate::events::DisplayCmd::Page
//!
//! ## Layout (240×240)
//!
//! ```text
//!  ┌─────────────────────────────┐  header (page name | pos | program)
//!  ├──────┬──────┬──────┬──────┤
//!  │  1   │  2   │  3   │  4   │   row 0  (scan 0..3)
//!  ├──────┼──────┼──────┼──────┤
//!  │  A   │  B   │  C   │  D   │   row 1  (scan 4..7)
//!  ├──────┴──────┴──────┴──────┤
//!  │   UP            DOWN      │   footer (scan 8, 9)
//!  └─────────────────────────────┘
//! ```
//!
//! The scan-index → screen-position map ([`CELLS`]) is the single place to
//! flip orientation if the numbered/lettered rows read swapped on hardware.

use core::fmt::Write as _;

use embedded_graphics::Drawable;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_8X13_BOLD};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use heapless::String;

use super::element::Widget;
use super::palette::{Color, Palette};
use crate::config::{Label, NAME_CAP, PAGE_BUTTONS};
use crate::events::{Cell, CellState, LedColor};

// ── Geometry (240×240) ───────────────────────────────────────────────────
const HEADER_H: u32 = 22;
const MARGIN: i32 = 3;
const CELL_GAP: i32 = 2;
const CELL_W: u32 = 57;
const ROW_H: u32 = 88;
const ROW0_Y: i32 = 24;
const ROW1_Y: i32 = 114; // ROW0_Y + ROW_H + CELL_GAP
const FOOT_Y: i32 = 204; // ROW1_Y + ROW_H + CELL_GAP
// The footer is a deliberately short, quiet "utility strip" — page nav now, and
// the menu's BACK / SAVE buttons later. Kept low-profile so it never dominates.
const FOOT_H: u32 = 30;
const FOOT_W: u32 = 116; // (240 − 2·MARGIN − CELL_GAP) / 2
/// First scan-index that lives in the footer (UP/DOWN). Cells `>=` this get the
/// muted utility-strip style instead of the active/idle cell style.
const FOOT_INDEX: usize = 8;

/// Width-per-character of [`FONT_8X13_BOLD`] (the cell-label font), used to
/// budget how many characters fit before truncation.
const LABEL_CHAR_W: u32 = 8;

/// Left edge of grid column `col` (0..4).
const fn col_x(col: i32) -> i32 {
    MARGIN + col * (CELL_W as i32 + CELL_GAP)
}

/// Cell rectangles as `(x, y, w, h)`, indexed by **scan-index**
/// (`SW1..SW4, A..D, UP, DOWN`). Rows: SW1-4 on top, A-D below, UP/DOWN in the
/// footer — the evidence-based chassis orientation (`remedy/lib/pins.py`
/// `LED_MAP`, `katana-live.toml`). **To flip top/bottom if it reads inverted on
/// hardware, swap the `ROW0_Y`/`ROW1_Y` blocks below — nothing else changes.**
const CELLS: [(i32, i32, u32, u32); PAGE_BUTTONS] = [
    (col_x(0), ROW0_Y, CELL_W, ROW_H), // 0  SW1
    (col_x(1), ROW0_Y, CELL_W, ROW_H), // 1  SW2
    (col_x(2), ROW0_Y, CELL_W, ROW_H), // 2  SW3
    (col_x(3), ROW0_Y, CELL_W, ROW_H), // 3  SW4
    (col_x(0), ROW1_Y, CELL_W, ROW_H), // 4  A
    (col_x(1), ROW1_Y, CELL_W, ROW_H), // 5  B
    (col_x(2), ROW1_Y, CELL_W, ROW_H), // 6  C
    (col_x(3), ROW1_Y, CELL_W, ROW_H), // 7  D
    (MARGIN, FOOT_Y, FOOT_W, FOOT_H),  // 8  UP
    (MARGIN + FOOT_W as i32 + CELL_GAP, FOOT_Y, FOOT_W, FOOT_H), // 9  DOWN
];

/// Small corner tag per scan-index — the physical switch name.
const TAGS: [&str; PAGE_BUTTONS] = ["1", "2", "3", "4", "A", "B", "C", "D", "UP", "DN"];

// ── Dark theme (default) ─────────────────────────────────────────────────
// The UI defaults to dark mode: a black field with hue-tinted cells that sit
// near-black when idle and lift to a brighter edge + subtle fill when active.
// These few constants are the whole palette, so a future light theme (or a
// per-config theme once presets carry one) is a localized swap.
/// Screen background.
const BG: Color = Palette::BLACK;
/// Accent gain. Config LED colours sit at a current-safe ~⅕ level
/// (`config::color::L = 0x30`); ×3 lifts them to a medium on-screen hue — bright
/// enough to read, without the blown-out primary-colour ("is this inverted?")
/// look a larger gain gave. Lower this to darken the active accents further.
const ACCENT_GAIN: u16 = 3;
/// Idle / utility-strip cell background — a near-black neutral, lifted just off
/// the field so a cell's outline is still visible but the screen stays dark and
/// high-contrast (a darker tile reads as more legible against the bright label —
/// bench-confirmed). A lighter `rgb(22, 22, 22)` reads nice but a touch washed
/// out; keep it noted as the "dim" theme variant for the future theme config.
const CELL_IDLE_BG: Color = Color::rgb(10, 10, 10);
/// Idle / utility-strip label — a bright near-white grey. The grid exists to
/// show every button's label, so idle labels stay legible; active labels go full
/// white (the dim↔brighten contrast). Independent of the accent so a dark hue
/// (e.g. a blue button) can't render its idle text invisibly.
const LABEL_IDLE: Color = Color::rgb(192, 192, 192);

/// A button's on-screen accent hue, derived from its (dim) LED colour.
/// Saturating, so a config already using bright values stays put.
fn led_to_display(c: LedColor) -> Color {
    let up = |v: u8| (v as u16 * ACCENT_GAIN).min(255) as u8;
    Color::rgb(up(c.r), up(c.g), up(c.b))
}

/// Whether a cell is drawn "lit" (active colour) vs. "dim" (idle). Mirrors the
/// LED feedback: a toggle/radio/momentary is lit when on/selected/held; a cycle
/// is lit off its base state (`pos > 1`); a plain bound button is always lit.
fn is_lit(state: CellState) -> bool {
    match state {
        CellState::Empty => false,
        CellState::Plain => true,
        CellState::Toggle(b) | CellState::Radio(b) | CellState::Momentary(b) => b,
        CellState::Cycle { pos, .. } => pos > 1,
    }
}

/// An empty cell placeholder for array initialisation.
fn blank_cell() -> Cell {
    Cell {
        label: Label::new(),
        color: LedColor { r: 0, g: 0, b: 0 },
        state: CellState::Empty,
    }
}

pub struct PageGrid {
    name: String<NAME_CAP>,
    index: u8,
    total: u8,
    program: u8,
    cells: [Cell; PAGE_BUTTONS],
    /// Per-cell "needs repaint" flag — the whole point of the widget.
    dirty: [bool; PAGE_BUTTONS],
    /// Per-cell transient highlight (a [`DisplayCmd::Flash`]). Drawn in the
    /// pressed style until the display task clears it.
    ///
    /// [`DisplayCmd::Flash`]: crate::events::DisplayCmd::Flash
    flashing: [bool; PAGE_BUTTONS],
    header_dirty: bool,
}

impl PageGrid {
    pub fn new() -> Self {
        Self {
            name: String::new(),
            index: 0,
            total: 0,
            program: 0,
            cells: core::array::from_fn(|_| blank_cell()),
            dirty: [true; PAGE_BUTTONS],
            flashing: [false; PAGE_BUTTONS],
            header_dirty: true,
        }
    }

    /// Adopt a fresh snapshot, flagging only what changed. Cells whose label /
    /// colour / state are unchanged are left clean (no repaint). A cell whose
    /// state changes also clears any pending flash on it (the real state takes
    /// over the highlight).
    pub fn set_snapshot(
        &mut self,
        name: &str,
        index: u8,
        total: u8,
        program: u8,
        cells: [Cell; PAGE_BUTTONS],
    ) {
        if self.name.as_str() != name || self.index != index || self.total != total || self.program != program {
            self.name.clear();
            for ch in name.chars() {
                if self.name.push(ch).is_err() {
                    break;
                }
            }
            self.index = index;
            self.total = total;
            self.program = program;
            self.header_dirty = true;
        }
        for (i, cell) in cells.into_iter().enumerate() {
            if self.cells[i] != cell {
                self.cells[i] = cell;
                self.flashing[i] = false;
                self.dirty[i] = true;
            }
        }
    }

    /// Start a transient highlight on cell `index` (a non-latching press).
    pub fn flash(&mut self, index: usize) {
        if let Some(f) = self.flashing.get_mut(index) {
            *f = true;
            self.dirty[index] = true;
        }
    }

    /// Clear a transient highlight, restoring the cell's real state next render.
    pub fn unflash(&mut self, index: usize) {
        if let Some(f) = self.flashing.get_mut(index) {
            if *f {
                *f = false;
                self.dirty[index] = true;
            }
        }
    }

    fn draw_header<D>(&self, target: &mut D) -> Result<(), D::Error>
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

        let name_style = MonoTextStyle::new(&FONT_8X13_BOLD, Palette::WHITE.to_rgb565());
        let left = TextStyleBuilder::new()
            .alignment(Alignment::Left)
            .baseline(Baseline::Middle)
            .build();
        Text::with_text_style(self.name.as_str(), Point::new(6, (HEADER_H / 2) as i32), name_style, left)
            .draw(target)?;

        let mut info: String<16> = String::new();
        let _ = write!(info, "{}/{} PC{}", self.index, self.total, self.program);
        let info_style = MonoTextStyle::new(&FONT_6X10, Palette::WHITE.to_rgb565());
        let right = TextStyleBuilder::new()
            .alignment(Alignment::Right)
            .baseline(Baseline::Middle)
            .build();
        Text::with_text_style(info.as_str(), Point::new(234, (HEADER_H / 2) as i32), info_style, right)
            .draw(target)
            .map(|_| ())
    }

    fn draw_cell<D>(&self, target: &mut D, i: usize) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let (x, y, w, h) = CELLS[i];
        let cell = &self.cells[i];
        let rect = Rectangle::new(Point::new(x, y), Size::new(w, h));
        let accent = led_to_display(cell.color);
        let flashing = self.flashing[i];
        let footer = i >= FOOT_INDEX;

        // Unbound, not flashing: blank slot with a faint frame so the grid
        // structure stays visible.
        if matches!(cell.state, CellState::Empty) && !flashing {
            rect.into_styled(
                PrimitiveStyleBuilder::new()
                    .fill_color(BG.to_rgb565())
                    .stroke_color(Color::rgb(18, 18, 18).to_rgb565())
                    .stroke_width(1)
                    .build(),
            )
            .draw(target)?;
            return Ok(());
        }

        // Dark-theme colours by state. Idle cells sit on a dark-neutral tile
        // (lifted off the black field so the cell shape + a readable grey label
        // are always visible) with a dim hue-tinted edge; an active cell lifts to
        // a tinted fill + bright edge + white label (the dim↔brighten contrast).
        // The footer is a quiet utility strip — same dark tile, accent label; a
        // press briefly inverts the cell for a high-contrast pop.
        let (bg, border, fg, stroke) = if flashing {
            (accent, Palette::WHITE, Palette::BLACK, 2u32)
        } else if footer {
            (CELL_IDLE_BG, accent.dim(2), accent, 1)
        } else if is_lit(cell.state) {
            (accent.dark(6), accent, Palette::WHITE, 2)
        } else {
            (CELL_IDLE_BG, accent.dim(2), LABEL_IDLE, 1)
        };

        rect.into_styled(
            PrimitiveStyleBuilder::new()
                .fill_color(bg.to_rgb565())
                .stroke_color(border.to_rgb565())
                .stroke_width(stroke)
                .build(),
        )
        .draw(target)?;

        // Corner tag (switch name) — main cells only. The short footer reads
        // cleanly from its label alone, so its tag is omitted.
        if !footer {
            let tag_style = MonoTextStyle::new(&FONT_6X10, border.to_rgb565());
            Text::with_text_style(
                TAGS[i],
                Point::new(x + 5, y + 9),
                tag_style,
                TextStyleBuilder::new()
                    .alignment(Alignment::Left)
                    .baseline(Baseline::Middle)
                    .build(),
            )
            .draw(target)?;
        }

        // Label, truncated to the cell width.
        let tall = h >= 60;
        let cx = x + (w as i32) / 2;
        let label_cy = if tall { y + 30 } else { y + (h as i32) / 2 };
        let max_chars = (w / LABEL_CHAR_W) as usize;
        let mut buf: String<16> = String::new();
        fit_label(&cell.label, max_chars, &mut buf);
        let label_style = MonoTextStyle::new(&FONT_8X13_BOLD, fg.to_rgb565());
        Text::with_text_style(
            buf.as_str(),
            Point::new(cx, label_cy),
            label_style,
            TextStyleBuilder::new()
                .alignment(Alignment::Center)
                .baseline(Baseline::Middle)
                .build(),
        )
        .draw(target)?;

        // State glyph in the lower area (skipped while flashing).
        if !flashing {
            self.draw_glyph(target, cell.state, Point::new(cx, y + (h as i32) - 16), border, fg)?;
        }
        Ok(())
    }

    fn draw_glyph<D>(
        &self,
        target: &mut D,
        state: CellState,
        centre: Point,
        lit: Color,
        fg: Color,
    ) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        const R: i32 = 6;
        let top_left = Point::new(centre.x - R, centre.y - R);
        match state {
            CellState::Toggle(on) | CellState::Radio(on) => {
                let style = if on {
                    PrimitiveStyleBuilder::new().fill_color(lit.to_rgb565()).build()
                } else {
                    PrimitiveStyleBuilder::new()
                        .stroke_color(fg.to_rgb565())
                        .stroke_width(2)
                        .build()
                };
                Circle::new(top_left, (R * 2) as u32).into_styled(style).draw(target)?;
            }
            CellState::Momentary(held) => {
                let style = if held {
                    PrimitiveStyleBuilder::new().fill_color(lit.to_rgb565()).build()
                } else {
                    PrimitiveStyleBuilder::new()
                        .stroke_color(fg.to_rgb565())
                        .stroke_width(2)
                        .build()
                };
                Rectangle::new(top_left, Size::new((R * 2) as u32, (R * 2) as u32))
                    .into_styled(style)
                    .draw(target)?;
            }
            CellState::Cycle { pos, len } => {
                let mut s: String<8> = String::new();
                if pos == 0 {
                    let _ = write!(s, "-/{}", len);
                } else {
                    let _ = write!(s, "{}/{}", pos, len);
                }
                let style = MonoTextStyle::new(&FONT_6X10, fg.to_rgb565());
                Text::with_text_style(
                    s.as_str(),
                    centre,
                    style,
                    TextStyleBuilder::new()
                        .alignment(Alignment::Center)
                        .baseline(Baseline::Middle)
                        .build(),
                )
                .draw(target)?;
            }
            CellState::Plain | CellState::Empty => {}
        }
        Ok(())
    }
}

impl Default for PageGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for PageGrid {
    fn render<D>(&mut self, target: &mut D) -> Result<bool, D::Error>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let mut drew = false;
        if self.header_dirty {
            self.draw_header(target)?;
            self.header_dirty = false;
            drew = true;
        }
        for i in 0..PAGE_BUTTONS {
            if self.dirty[i] {
                self.draw_cell(target, i)?;
                self.dirty[i] = false;
                drew = true;
            }
        }
        Ok(drew)
    }

    fn mark_dirty(&mut self) {
        self.header_dirty = true;
        self.dirty = [true; PAGE_BUTTONS];
    }
}

/// Copy `label` into `buf`, truncating to `max` characters with a trailing `~`
/// when it would overflow the cell. The ASCII mono font has no `…`, so `~`
/// stands in as the elision marker.
fn fit_label(label: &str, max: usize, buf: &mut String<16>) {
    buf.clear();
    if max == 0 {
        return;
    }
    if label.chars().count() <= max {
        for ch in label.chars() {
            if buf.push(ch).is_err() {
                break;
            }
        }
        return;
    }
    for ch in label.chars().take(max.saturating_sub(1)) {
        if buf.push(ch).is_err() {
            break;
        }
    }
    let _ = buf.push('~');
}
