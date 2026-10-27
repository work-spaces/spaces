//! A `plotters` drawing backend that renders into a grid of terminal cells.
//!
//! Each cell is divided into `CELL x CELL` sub-pixels so that plotters can lay out
//! charts with sub-cell precision:
//!
//! - axis-aligned axis/tick lines become box-drawing characters,
//! - filled rectangles (bars) become full or eighth blocks,
//! - other lines and pixels (data series) become braille dots,
//! - text is written one character per cell.

use super::PlotColor;
use plotters_backend::text_anchor::{HPos, VPos};
use plotters_backend::{
    BackendCoord, BackendStyle, BackendTextStyle, DrawingBackend, DrawingErrorKind, rasterizer,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Sub-pixels per terminal cell in each direction.
pub(crate) const CELL: i32 = 8;

const ARM_UP: u8 = 1;
const ARM_DOWN: u8 = 2;
const ARM_LEFT: u8 = 4;
const ARM_RIGHT: u8 = 8;

const BLOCKS_BOTTOM: [char; 7] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇'];
const BLOCKS_LEFT: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
const BLOCK_FULL: char = '█';

/// Display width of `text` in terminal cells.
pub(crate) fn text_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

#[derive(Clone, Copy, Default)]
struct Cell {
    text: Option<char>,
    wide_tail: bool,
    text_color: Option<PlotColor>,
    /// Bit `y * CELL + x` is set when the sub-pixel is covered by a filled rectangle.
    block: u64,
    /// Braille dot bits (U+2800 encoding).
    dots: u8,
    point: bool,
    color: Option<PlotColor>,
}

#[derive(Clone, Copy, Debug)]
struct HSeg {
    y: i32,
    x0: i32,
    x1: i32,
}

#[derive(Clone, Copy, Debug)]
struct VSeg {
    x: i32,
    y0: i32,
    y1: i32,
}

/// A run of characters on a rendered row that share the same color.
pub(crate) type Run = (String, Option<PlotColor>);

/// The character grid that plotters draws into (through `TerminalBackend`).
pub(crate) struct Canvas {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    hsegs: Vec<HSeg>,
    vsegs: Vec<VSeg>,
}

impl Canvas {
    pub(crate) fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![Cell::default(); cols * rows],
            hsegs: Vec::new(),
            vsegs: Vec::new(),
        }
    }

    pub(crate) fn pixel_size(&self) -> (u32, u32) {
        (
            self.cols as u32 * CELL as u32,
            self.rows as u32 * CELL as u32,
        )
    }

    fn cell_mut(&mut self, col: i32, row: i32) -> Option<&mut Cell> {
        if col < 0 || row < 0 || col as usize >= self.cols || row as usize >= self.rows {
            return None;
        }
        Some(&mut self.cells[row as usize * self.cols + col as usize])
    }

    fn cell(&self, col: usize, row: usize) -> &Cell {
        &self.cells[row * self.cols + col]
    }

    pub(crate) fn hline(&mut self, y: i32, x0: i32, x1: i32) {
        self.hsegs.push(HSeg {
            y,
            x0: x0.min(x1),
            x1: x0.max(x1),
        });
    }

    pub(crate) fn vline(&mut self, x: i32, y0: i32, y1: i32) {
        self.vsegs.push(VSeg {
            x,
            y0: y0.min(y1),
            y1: y0.max(y1),
        });
    }

    fn set_dot(&mut self, (x, y): BackendCoord, color: Option<PlotColor>) {
        let (col, row) = (x.div_euclid(CELL), y.div_euclid(CELL));
        let (dx, dy) = (
            x.rem_euclid(CELL) / (CELL / 2),
            y.rem_euclid(CELL) / (CELL / 4),
        );
        let bit = match (dx, dy) {
            (0, 0) => 0x01,
            (0, 1) => 0x02,
            (0, 2) => 0x04,
            (0, _) => 0x40,
            (_, 0) => 0x08,
            (_, 1) => 0x10,
            (_, 2) => 0x20,
            (_, _) => 0x80,
        };
        if let Some(cell) = self.cell_mut(col, row) {
            cell.dots |= bit;
            cell.color = color.or(cell.color);
        }
    }

    fn set_point(&mut self, (x, y): BackendCoord, color: Option<PlotColor>) {
        if let Some(cell) = self.cell_mut(x.div_euclid(CELL), y.div_euclid(CELL)) {
            cell.point = true;
            cell.color = color.or(cell.color);
        }
    }

    /// Fills the rectangle (both corners inclusive) with block sub-pixels.
    fn fill_rect(&mut self, from: BackendCoord, to: BackendCoord, color: Option<PlotColor>) {
        let (w, h) = self.pixel_size();
        let x0 = from.0.min(to.0).max(0);
        let x1 = from.0.max(to.0).min(w as i32 - 1);
        let y0 = from.1.min(to.1).max(0);
        let y1 = from.1.max(to.1).min(h as i32 - 1);
        if x0 > x1 || y0 > y1 {
            return;
        }
        for row in y0.div_euclid(CELL)..=y1.div_euclid(CELL) {
            for col in x0.div_euclid(CELL)..=x1.div_euclid(CELL) {
                let lx0 = (x0 - col * CELL).max(0);
                let lx1 = (x1 - col * CELL).min(CELL - 1);
                let ly0 = (y0 - row * CELL).max(0);
                let ly1 = (y1 - row * CELL).min(CELL - 1);
                let row_bits = ((1u64 << (lx1 - lx0 + 1)) - 1) << lx0;
                let mut mask = 0u64;
                for ly in ly0..=ly1 {
                    mask |= row_bits << (ly * CELL);
                }
                if let Some(cell) = self.cell_mut(col, row) {
                    cell.block |= mask;
                    cell.color = color.or(cell.color);
                }
            }
        }
    }

    /// Writes `text` starting at the given cell. Characters outside the grid are clipped.
    pub(crate) fn put_text(&mut self, col: i32, row: i32, text: &str, color: Option<PlotColor>) {
        let mut col = col;
        for ch in text.chars() {
            let width = UnicodeWidthChar::width(ch).unwrap_or(0) as i32;
            if width == 0 {
                continue;
            }
            let fits = col >= 0 && (col + width) as usize <= self.cols;
            if fits {
                if let Some(cell) = self.cell_mut(col, row) {
                    cell.text = Some(ch);
                    cell.wide_tail = false;
                    cell.text_color = color;
                }
                if width == 2
                    && let Some(cell) = self.cell_mut(col + 1, row)
                {
                    cell.text = None;
                    cell.wide_tail = true;
                }
            }
            col += width;
        }
    }

    /// Computes which box-drawing arms are visible in a cell.
    fn box_arms(&self, col: usize, row: usize) -> u8 {
        let cx0 = col as i32 * CELL;
        let cx1 = cx0 + CELL - 1;
        let cy0 = row as i32 * CELL;
        let cy1 = cy0 + CELL - 1;

        let hs: Vec<&HSeg> = self
            .hsegs
            .iter()
            .filter(|h| h.y >= cy0 && h.y <= cy1 && h.x1 >= cx0 && h.x0 <= cx1)
            .collect();
        let vs: Vec<&VSeg> = self
            .vsegs
            .iter()
            .filter(|v| v.x >= cx0 && v.x <= cx1 && v.y1 >= cy0 && v.y0 <= cy1)
            .collect();

        match (hs.first(), vs.first()) {
            (None, None) => 0,
            (Some(h0), Some(v0)) => {
                // Arms are relative to the crossing point so that ticks and corners
                // render as `┤ ┬ └ ┼`.
                let (vx, hy) = (v0.x, h0.y);
                let mut arms = 0;
                if hs.iter().any(|h| h.x0 < vx) {
                    arms |= ARM_LEFT;
                }
                if hs.iter().any(|h| h.x1 > vx) {
                    arms |= ARM_RIGHT;
                }
                if vs.iter().any(|v| v.y0 < hy) {
                    arms |= ARM_UP;
                }
                if vs.iter().any(|v| v.y1 > hy) {
                    arms |= ARM_DOWN;
                }
                arms
            }
            (Some(_), None) => {
                let coverage: i32 = hs.iter().map(|h| h.x1.min(cx1) - h.x0.max(cx0) + 1).sum();
                if coverage >= CELL / 2 {
                    ARM_LEFT | ARM_RIGHT
                } else {
                    0
                }
            }
            (None, Some(_)) => {
                let coverage: i32 = vs.iter().map(|v| v.y1.min(cy1) - v.y0.max(cy0) + 1).sum();
                if coverage >= CELL / 2 {
                    ARM_UP | ARM_DOWN
                } else {
                    0
                }
            }
        }
    }

    fn glyph(&self, col: usize, row: usize, ascii: bool) -> (char, Option<PlotColor>) {
        let cell = self.cell(col, row);
        if let Some(ch) = cell.text {
            return (ch, cell.text_color);
        }
        if let Some(ch) = box_glyph(self.box_arms(col, row), ascii) {
            return (ch, None);
        }
        if cell.point {
            return (if ascii { '*' } else { '●' }, cell.color);
        }
        if let Some(ch) = block_glyph(cell.block, ascii) {
            return (ch, cell.color);
        }
        if let Some(ch) = dots_glyph(cell.dots, ascii) {
            return (ch, cell.color);
        }
        (' ', None)
    }

    /// Renders the grid into rows of same-colored runs, with trailing spaces trimmed.
    pub(crate) fn render(&self, ascii: bool) -> Vec<Vec<Run>> {
        let mut result = Vec::with_capacity(self.rows);
        for row in 0..self.rows {
            let mut chars: Vec<(char, Option<PlotColor>)> = Vec::with_capacity(self.cols);
            let mut col = 0;
            while col < self.cols {
                let cell = self.cell(col, row);
                if cell.wide_tail {
                    chars.push((' ', None));
                    col += 1;
                    continue;
                }
                let (ch, color) = self.glyph(col, row, ascii);
                let is_wide = cell.text.is_some() && UnicodeWidthChar::width(ch) == Some(2);
                if is_wide && col + 1 < self.cols && self.cell(col + 1, row).wide_tail {
                    chars.push((ch, color));
                    col += 2;
                    continue;
                }
                if is_wide {
                    chars.push((' ', None));
                } else {
                    chars.push((ch, if ch == ' ' { None } else { color }));
                }
                col += 1;
            }
            while chars.last().is_some_and(|(ch, _)| *ch == ' ') {
                chars.pop();
            }

            let mut runs: Vec<Run> = Vec::new();
            for (ch, color) in chars {
                match runs.last_mut() {
                    Some((text, run_color)) if *run_color == color => text.push(ch),
                    _ => runs.push((ch.to_string(), color)),
                }
            }
            result.push(runs);
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn to_plain_string(&self, ascii: bool) -> String {
        self.render(ascii)
            .into_iter()
            .map(|runs| runs.into_iter().map(|(text, _)| text).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn box_glyph(arms: u8, ascii: bool) -> Option<char> {
    if arms == 0 {
        return None;
    }
    let vertical = arms & (ARM_UP | ARM_DOWN) != 0;
    let horizontal = arms & (ARM_LEFT | ARM_RIGHT) != 0;
    if ascii {
        return Some(match (vertical, horizontal) {
            (true, false) => '|',
            (false, true) => '-',
            _ => '+',
        });
    }
    let up = arms & ARM_UP != 0;
    let down = arms & ARM_DOWN != 0;
    let left = arms & ARM_LEFT != 0;
    let right = arms & ARM_RIGHT != 0;
    Some(match (up, down, left, right) {
        (true, true, true, true) => '┼',
        (true, true, true, false) => '┤',
        (true, true, false, true) => '├',
        (false, true, true, true) => '┬',
        (true, false, true, true) => '┴',
        (true, false, false, true) => '└',
        (true, false, true, false) => '┘',
        (false, true, false, true) => '┌',
        (false, true, true, false) => '┐',
        (_, _, false, false) => '│',
        _ => '─',
    })
}

fn block_glyph(mask: u64, ascii: bool) -> Option<char> {
    if mask == 0 {
        return None;
    }
    let half = (CELL * CELL / 2) as u32;
    if ascii {
        return (mask.count_ones() >= half).then_some('#');
    }
    if mask == u64::MAX {
        return Some(BLOCK_FULL);
    }
    let rows: Vec<u8> = (0..CELL).map(|r| (mask >> (r * CELL)) as u8).collect();

    // Bottom-anchored (vertical bars): full rows at the bottom, empty rows above.
    let full_bottom = rows.iter().rev().take_while(|r| **r == 0xFF).count();
    if full_bottom > 0 && rows[..CELL as usize - full_bottom].iter().all(|r| *r == 0) {
        return Some(BLOCKS_BOTTOM[full_bottom - 1]);
    }

    // Left-anchored (horizontal bars): every row covers the same leftmost columns.
    let first = rows[0];
    if first != 0 && rows.iter().all(|r| *r == first) && (first as u16 + 1).is_power_of_two() {
        let width = first.count_ones() as usize;
        return Some(BLOCKS_LEFT[width - 1]);
    }

    (mask.count_ones() >= half).then_some(BLOCK_FULL)
}

fn dots_glyph(dots: u8, ascii: bool) -> Option<char> {
    if dots == 0 {
        return None;
    }
    if ascii {
        let top = dots & (0x01 | 0x02 | 0x08 | 0x10) != 0;
        let bottom = dots & (0x04 | 0x20 | 0x40 | 0x80) != 0;
        return Some(match (top, bottom) {
            (true, false) => '\'',
            (false, true) => '.',
            _ => ':',
        });
    }
    char::from_u32(0x2800 + dots as u32)
}

/// The `plotters` drawing backend. It borrows the canvas so that the canvas can be
/// read back after plotters drops its drawing areas.
pub(crate) struct TerminalBackend<'a> {
    canvas: &'a mut Canvas,
}

impl<'a> TerminalBackend<'a> {
    pub(crate) fn new(canvas: &'a mut Canvas) -> Self {
        Self { canvas }
    }
}

type BackendResult = Result<(), DrawingErrorKind<std::convert::Infallible>>;

impl DrawingBackend for TerminalBackend<'_> {
    type ErrorType = std::convert::Infallible;

    fn get_size(&self) -> (u32, u32) {
        self.canvas.pixel_size()
    }

    fn ensure_prepared(&mut self) -> BackendResult {
        Ok(())
    }

    fn present(&mut self) -> BackendResult {
        Ok(())
    }

    fn draw_pixel(
        &mut self,
        point: BackendCoord,
        color: plotters_backend::BackendColor,
    ) -> BackendResult {
        // Lines are anti-aliased by the rasterizer; only keep the stronger half.
        if color.alpha >= 0.5 {
            self.canvas.set_dot(point, PlotColor::from_rgb(color.rgb));
        }
        Ok(())
    }

    fn draw_line<S: BackendStyle>(
        &mut self,
        from: BackendCoord,
        to: BackendCoord,
        style: &S,
    ) -> BackendResult {
        let color = style.color();
        if color.alpha == 0.0 {
            return Ok(());
        }
        let palette = PlotColor::from_rgb(color.rgb);
        if palette.is_none() && from.1 == to.1 {
            self.canvas.hline(from.1, from.0, to.0);
            return Ok(());
        }
        if palette.is_none() && from.0 == to.0 {
            self.canvas.vline(from.0, from.1, to.1);
            return Ok(());
        }
        rasterizer::draw_line(self, from, to, style)
    }

    fn draw_rect<S: BackendStyle>(
        &mut self,
        upper_left: BackendCoord,
        bottom_right: BackendCoord,
        style: &S,
        fill: bool,
    ) -> BackendResult {
        let color = style.color();
        if color.alpha == 0.0 {
            return Ok(());
        }
        if fill {
            self.canvas
                .fill_rect(upper_left, bottom_right, PlotColor::from_rgb(color.rgb));
            Ok(())
        } else {
            rasterizer::draw_rect(self, upper_left, bottom_right, style, false)
        }
    }

    fn draw_circle<S: BackendStyle>(
        &mut self,
        center: BackendCoord,
        _radius: u32,
        style: &S,
        _fill: bool,
    ) -> BackendResult {
        let color = style.color();
        if color.alpha > 0.0 {
            self.canvas
                .set_point(center, PlotColor::from_rgb(color.rgb));
        }
        Ok(())
    }

    fn draw_text<TStyle: BackendTextStyle>(
        &mut self,
        text: &str,
        style: &TStyle,
        pos: BackendCoord,
    ) -> BackendResult {
        let color = style.color();
        if color.alpha == 0.0 {
            return Ok(());
        }
        let width = text_width(text) as i32 * CELL;
        let anchor = style.anchor();
        let dx = match anchor.h_pos {
            HPos::Left => 0,
            HPos::Right => -width,
            HPos::Center => -width / 2,
        };
        let dy = match anchor.v_pos {
            VPos::Top => 0,
            VPos::Center => -CELL / 2,
            VPos::Bottom => -CELL,
        };
        let col = (pos.0 + dx + CELL / 2).div_euclid(CELL);
        let row = (pos.1 + dy + CELL / 2).div_euclid(CELL);
        self.canvas
            .put_text(col, row, text, PlotColor::from_rgb(color.rgb));
        Ok(())
    }

    fn estimate_text_size<TStyle: BackendTextStyle>(
        &self,
        text: &str,
        _style: &TStyle,
    ) -> Result<(u32, u32), DrawingErrorKind<Self::ErrorType>> {
        Ok((text_width(text) as u32 * CELL as u32, CELL as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_glyphs() {
        assert_eq!(block_glyph(u64::MAX, false), Some('█'));
        // bottom 3 rows
        assert_eq!(block_glyph(0xFFFFFF << 40, false), Some('▃'));
        // left 2 columns on every row
        assert_eq!(block_glyph(0x0303030303030303, false), Some('▎'));
        assert_eq!(block_glyph(0x0303030303030303, true), None);
        assert_eq!(block_glyph(u64::MAX, true), Some('#'));
        assert_eq!(block_glyph(0, false), None);
    }

    #[test]
    fn box_glyphs() {
        assert_eq!(box_glyph(ARM_UP | ARM_DOWN | ARM_LEFT, false), Some('┤'));
        assert_eq!(box_glyph(ARM_UP | ARM_RIGHT, false), Some('└'));
        assert_eq!(box_glyph(ARM_UP | ARM_RIGHT, true), Some('+'));
        assert_eq!(box_glyph(ARM_LEFT | ARM_RIGHT, true), Some('-'));
        assert_eq!(box_glyph(0, false), None);
    }

    #[test]
    fn clipping_out_of_bounds() {
        let mut canvas = Canvas::new(4, 2);
        canvas.fill_rect((-100, -100), (1000, 1000), Some(PlotColor::Primary));
        canvas.set_dot((-5, 3), None);
        canvas.set_dot((500, 3), None);
        canvas.set_point((32, 16), None);
        canvas.put_text(-2, 0, "abcdef", None);
        canvas.put_text(2, 5, "zz", None);
        canvas.hline(-50, -10, 1000);
        canvas.vline(1000, -10, 1000);
        let text = canvas.to_plain_string(true);
        assert_eq!(text, "cdef\n####");
    }

    #[test]
    fn wide_text() {
        let mut canvas = Canvas::new(5, 1);
        canvas.put_text(0, 0, "日本x", None);
        assert_eq!(canvas.to_plain_string(false), "日本x");
        let mut canvas = Canvas::new(3, 1);
        canvas.put_text(1, 0, "日本", None);
        assert_eq!(canvas.to_plain_string(false), " 日");
    }
}
