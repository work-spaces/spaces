//! Chart components: plain data in, `superconsole` lines out.

use super::backend::{CELL, Canvas, TerminalBackend, text_width};
use super::{PlotColor, PlotOptions, runs_to_line, styled_span};
use crate::bootstrap::{Component, bold_style, secondary_style};
use crossterm::style::{ContentStyle, StyledContent};
use plotters::coord::ranged1d::Ranged;
use plotters::coord::types::RangedCoordf64;
use plotters::prelude::*;
use std::ops::Range;
use superconsole::{Line, Span};

/// Formats an axis or value label.
pub type Formatter = Box<dyn Fn(f64) -> String>;

const BOTTOM_ROWS: usize = 2;
const MIN_PLOT_COLS: usize = 10;
const MIN_PLOT_ROWS: usize = 3;
const DEFAULT_WIDTH: usize = 80;
const DEFAULT_HEIGHT: usize = 20;

/// Formats a number with up to two decimals, dropping trailing zeros.
pub fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn sanitize(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

fn rgb(color: PlotColor) -> RGBColor {
    let (r, g, b) = color.rgb();
    RGBColor(r, g, b)
}

/// Shortens `text` to at most `max` cells, keeping the end (which is usually the most
/// specific part of rule labels).
fn truncate_start(text: &str, max: usize, ascii: bool) -> String {
    if text_width(text) <= max {
        return text.to_string();
    }
    let ellipsis = if ascii { "..." } else { "…" };
    let budget = max.saturating_sub(text_width(ellipsis));
    let mut tail: Vec<char> = Vec::new();
    let mut width = 0;
    for ch in text.chars().rev() {
        let w = text_width(ch.encode_utf8(&mut [0; 4]));
        if width + w > budget {
            break;
        }
        width += w;
        tail.push(ch);
    }
    let tail: String = tail.into_iter().rev().collect();
    format!("{ellipsis}{tail}")
}

/// Common chart settings: title, footer, size, and rendering options.
struct Frame {
    title: Option<String>,
    footer: Option<String>,
    width: usize,
    height: usize,
    options: PlotOptions,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            title: None,
            footer: None,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            options: PlotOptions::default(),
        }
    }
}

impl Frame {
    fn styled_line(&self, text: &str, style: ContentStyle) -> Line {
        let mut line = Line::default();
        if self.options.color {
            line.push(Span::new_styled_lossy(StyledContent::new(
                style,
                text.to_string(),
            )));
        } else {
            line.push(Span::new_unstyled_lossy(text));
        }
        line
    }

    fn render(
        &self,
        is_empty: bool,
        canvas: impl FnOnce() -> Result<Canvas, String>,
        extra: impl FnOnce() -> Vec<Line>,
    ) -> Vec<Line> {
        let mut lines = Vec::new();
        if let Some(title) = &self.title {
            lines.push(self.styled_line(title, bold_style()));
        }
        if is_empty {
            lines.push(self.styled_line("No data to plot", secondary_style()));
        } else {
            match canvas() {
                Ok(canvas) => {
                    lines.extend(
                        canvas
                            .render(self.options.ascii)
                            .into_iter()
                            .map(|runs| runs_to_line(runs, &self.options)),
                    );
                    lines.extend(extra());
                }
                Err(err) => lines.push(
                    self.styled_line(&format!("Unable to render chart: {err}"), secondary_style()),
                ),
            }
        }
        if let Some(footer) = &self.footer {
            lines.push(self.styled_line(footer, secondary_style()));
        }
        lines
    }
}

macro_rules! impl_frame_builders {
    ($chart:ty) => {
        impl $chart {
            /// Title shown above the chart.
            pub fn title(mut self, title: impl Into<String>) -> Self {
                self.frame.title = Some(title.into());
                self
            }

            /// Footer shown below the chart (for example, summary statistics).
            pub fn footer(mut self, footer: impl Into<String>) -> Self {
                self.frame.footer = Some(footer.into());
                self
            }

            /// Total width in terminal columns.
            pub fn width(mut self, width: usize) -> Self {
                self.frame.width = width;
                self
            }

            /// Total height of the plot (including axis labels) in terminal rows.
            pub fn height(mut self, height: usize) -> Self {
                self.frame.height = height;
                self
            }

            pub fn options(mut self, options: PlotOptions) -> Self {
                self.frame.options = options;
                self
            }
        }
    };
}

/// Cell layout of a chart: `left` label columns, the plot area, `right` margin columns,
/// and `BOTTOM_ROWS` rows for the x axis and its labels.
struct Layout {
    left: usize,
    plot_cols: usize,
    right: usize,
    plot_rows: usize,
}

impl Layout {
    fn cols(&self) -> usize {
        self.left + self.plot_cols + self.right
    }

    fn rows(&self) -> usize {
        self.plot_rows + BOTTOM_ROWS
    }
}

struct Mesh<'a> {
    x_range: Range<f64>,
    y_range: Range<f64>,
    x_labels: usize,
    y_labels: usize,
    x_fmt: &'a dyn Fn(&f64) -> String,
    y_fmt: &'a dyn Fn(&f64) -> String,
}

type Chart<'a, 'b> =
    ChartContext<'a, TerminalBackend<'b>, Cartesian2d<RangedCoordf64, RangedCoordf64>>;
type DrawResult = Result<(), DrawingAreaErrorKind<std::convert::Infallible>>;

/// Draws the axes with plotters and lets `draw` add the data.
///
/// The y axis is drawn in the last column of the left label area and the x axis in the
/// first row of the bottom label area. Y labels are right-aligned one column before the
/// axis, x labels are centered on their ticks in the row below the x axis.
fn plot(
    layout: &Layout,
    mesh: Mesh<'_>,
    draw: impl FnOnce(&mut Chart<'_, '_>) -> DrawResult,
) -> Result<Canvas, String> {
    let mut canvas = Canvas::new(layout.cols(), layout.rows());
    {
        let root = TerminalBackend::new(&mut canvas).into_drawing_area();
        let result: DrawResult = (|| {
            let mut chart = ChartBuilder::on(&root)
                .margin_right(layout.right as i32 * CELL)
                .x_label_area_size(BOTTOM_ROWS as i32 * CELL)
                .y_label_area_size(layout.left as i32 * CELL)
                .build_cartesian_2d(mesh.x_range, mesh.y_range)?;
            chart
                .configure_mesh()
                .disable_mesh()
                .axis_style(BLACK)
                .label_style(("monospace", CELL as f64).into_font().color(&BLACK))
                .set_tick_mark_size(LabelAreaPosition::Left, CELL)
                .set_tick_mark_size(LabelAreaPosition::Bottom, CELL / 2)
                .x_labels(mesh.x_labels)
                .y_labels(mesh.y_labels)
                .x_label_formatter(mesh.x_fmt)
                .y_label_formatter(mesh.y_fmt)
                .draw()?;
            draw(&mut chart)
        })();
        result.map_err(|err| err.to_string())?;
    }

    // The axes live in different label areas, so join them at the corner. Also extend
    // them one pixel past the plot so ticks at the very ends render as `┤`/`┬`.
    let axis_x = layout.left as i32 * CELL - 1;
    let axis_y = layout.plot_rows as i32 * CELL;
    let plot_end_x = (layout.left + layout.plot_cols) as i32 * CELL - 1;
    canvas.vline(axis_x, axis_y - 1, axis_y);
    canvas.hline(axis_y, axis_x, axis_x + 1);
    canvas.vline(axis_x, -1, 0);
    canvas.hline(axis_y, plot_end_x, plot_end_x + 1);
    Ok(canvas)
}

/// Returns the index for an integral value, or `None` for fractional key points.
fn as_index(value: f64) -> Option<usize> {
    let rounded = value.round();
    ((value - rounded).abs() < 1e-6 && rounded >= 0.0).then_some(rounded as usize)
}

// ---------------------------------------------------------------------------
// BarChart
// ---------------------------------------------------------------------------

/// Horizontal bar chart (one row per bar, first bar on top) with the value printed
/// after each bar.
pub struct BarChart {
    bars: Vec<(String, f64)>,
    value_formatter: Formatter,
    frame: Frame,
}

impl_frame_builders!(BarChart);

impl BarChart {
    pub fn new(bars: Vec<(String, f64)>) -> Self {
        Self {
            bars,
            value_formatter: Box::new(format_number),
            frame: Frame::default(),
        }
    }

    pub fn value_formatter(mut self, formatter: impl Fn(f64) -> String + 'static) -> Self {
        self.value_formatter = Box::new(formatter);
        self
    }

    fn render_canvas(&self) -> Result<Canvas, String> {
        let n = self.bars.len();
        let ascii = self.frame.options.ascii;
        let values: Vec<f64> = self
            .bars
            .iter()
            .map(|(_, value)| sanitize(*value))
            .collect();
        let max = values.iter().copied().fold(0.0, f64::max);
        let max = if max > 0.0 { max } else { 1.0 };
        let value_texts: Vec<String> = self
            .bars
            .iter()
            .map(|(_, value)| (self.value_formatter)(*value))
            .collect();
        let value_width = value_texts.iter().map(|t| text_width(t)).max().unwrap_or(0);
        let max_label = (self.frame.width / 3).max(8);
        let labels: Vec<String> = self
            .bars
            .iter()
            .map(|(label, _)| truncate_start(label, max_label, ascii))
            .collect();
        let label_width = labels
            .iter()
            .map(|l| text_width(l))
            .max()
            .unwrap_or(0)
            .max(1);

        let left = label_width + 2;
        let right = value_width + 2;
        let layout = Layout {
            left,
            plot_cols: self
                .frame
                .width
                .saturating_sub(left + right)
                .max(MIN_PLOT_COLS),
            right,
            plot_rows: n,
        };

        let x_fmt = |value: &f64| (self.value_formatter)(*value);
        // Labels are written below (left-aligned); plotters only draws the ticks.
        let y_fmt = |_: &f64| String::new();
        // Each bar occupies exactly one row: the y range is chosen so that one unit
        // maps to one cell and integer values land in the middle of a row.
        let row = 1.0 / CELL as f64;
        let mesh = Mesh {
            x_range: 0.0..max,
            y_range: -0.5..(n as f64 - 0.5 - row),
            x_labels: (layout.plot_cols / 12).max(2),
            y_labels: n,
            x_fmt: &x_fmt,
            y_fmt: &y_fmt,
        };
        let style = rgb(PlotColor::Primary).filled();
        let mut canvas = plot(&layout, mesh, |chart| {
            chart.draw_series(
                values
                    .iter()
                    .enumerate()
                    .filter(|(_, value)| **value > 0.0)
                    .map(|(index, value)| {
                        let y = (n - 1 - index) as f64;
                        Rectangle::new([(0.0, y - 0.5), (*value, y + 0.5 - row)], style)
                    }),
            )?;
            Ok(())
        })?;

        // Mirror the plotters x mapping to place the values after the bar ends.
        let plot_x0 = layout.left as i32 * CELL;
        let plot_px = (layout.plot_cols as i32 * CELL - 1) as f64;
        for (index, ((value, text), label)) in
            values.iter().zip(&value_texts).zip(&labels).enumerate()
        {
            canvas.put_text(0, index as i32, label, None);
            let col = if *value > 0.0 {
                let end = plot_x0 + (plot_px * value / max + 1e-3).floor() as i32;
                end.div_euclid(CELL) + 2
            } else {
                layout.left as i32 + 1
            };
            canvas.put_text(col, index as i32, text, None);
        }
        Ok(canvas)
    }
}

impl Component for BarChart {
    fn render(&self) -> Vec<Line> {
        self.frame
            .render(self.bars.is_empty(), || self.render_canvas(), Vec::new)
    }
}

// ---------------------------------------------------------------------------
// HistogramChart
// ---------------------------------------------------------------------------

/// A histogram bin covering `[lo, hi)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Bin {
    pub lo: f64,
    pub hi: f64,
    pub count: u64,
}

/// Vertical histogram. Bins are drawn as equal-width columns, so log-scale bins
/// (computed by the caller) work without a log axis.
pub struct HistogramChart {
    bins: Vec<Bin>,
    x_formatter: Formatter,
    frame: Frame,
}

impl_frame_builders!(HistogramChart);

impl HistogramChart {
    pub fn new(bins: Vec<Bin>) -> Self {
        Self {
            bins,
            x_formatter: Box::new(format_number),
            frame: Frame::default(),
        }
    }

    /// Formats the bin edges shown on the x axis.
    pub fn x_formatter(mut self, formatter: impl Fn(f64) -> String + 'static) -> Self {
        self.x_formatter = Box::new(formatter);
        self
    }

    fn render_canvas(&self) -> Result<Canvas, String> {
        let bins = &self.bins;
        let n = bins.len();
        let max_count = bins.iter().map(|b| b.count).max().unwrap_or(0).max(1);
        let left = max_count.to_string().len() + 2;
        let cells_per_bin = (self.frame.width.saturating_sub(left + 2) / n).max(1);
        let plot_cols = cells_per_bin * n;
        let layout = Layout {
            left,
            plot_cols,
            right: self.frame.width.saturating_sub(left + plot_cols).max(2),
            plot_rows: self
                .frame
                .height
                .saturating_sub(BOTTOM_ROWS)
                .max(MIN_PLOT_ROWS),
        };

        // One x unit is one bin and maps exactly to `cells_per_bin` cells.
        let bin_px = (cells_per_bin as i32 * CELL) as f64;
        let x_end = (plot_cols as i32 * CELL - 1) as f64 / bin_px;
        let x_fmt = |value: &f64| match as_index(*value) {
            Some(index) if index < n => (self.x_formatter)(bins[index].lo),
            Some(index) if index == n => (self.x_formatter)(bins[n - 1].hi),
            _ => String::new(),
        };
        let y_fmt = |value: &f64| as_index(*value).map(|v| v.to_string()).unwrap_or_default();
        let mesh = Mesh {
            x_range: 0.0..x_end,
            y_range: 0.0..max_count as f64,
            x_labels: (plot_cols / 10).clamp(1, n),
            y_labels: (layout.plot_rows / 2).max(3).min(max_count as usize + 1),
            x_fmt: &x_fmt,
            y_fmt: &y_fmt,
        };
        // Leave a one-column gap between bars when there is room for it.
        let bar_cells = if cells_per_bin >= 2 {
            cells_per_bin - 1
        } else {
            1
        };
        let bar_width = (bar_cells as i32 * CELL - 1) as f64 / bin_px;
        let style = rgb(PlotColor::Primary).filled();
        plot(&layout, mesh, |chart| {
            chart.draw_series(
                bins.iter()
                    .enumerate()
                    .filter(|(_, bin)| bin.count > 0)
                    .map(|(index, bin)| {
                        let x = index as f64;
                        Rectangle::new([(x, 0.0), (x + bar_width, bin.count as f64)], style)
                    }),
            )?;
            Ok(())
        })
    }
}

impl Component for HistogramChart {
    fn render(&self) -> Vec<Line> {
        self.frame
            .render(self.bins.is_empty(), || self.render_canvas(), Vec::new)
    }
}

// ---------------------------------------------------------------------------
// LineChart
// ---------------------------------------------------------------------------

/// A named series of `(x, y)` points.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub points: Vec<(f64, f64)>,
}

/// Line chart with one colored line per series and a legend below the plot.
pub struct LineChart {
    series: Vec<Series>,
    x_formatter: Formatter,
    y_formatter: Formatter,
    frame: Frame,
}

impl_frame_builders!(LineChart);

impl LineChart {
    pub fn new(series: Vec<Series>) -> Self {
        Self {
            series,
            x_formatter: Box::new(format_number),
            y_formatter: Box::new(format_number),
            frame: Frame::default(),
        }
    }

    pub fn x_formatter(mut self, formatter: impl Fn(f64) -> String + 'static) -> Self {
        self.x_formatter = Box::new(formatter);
        self
    }

    pub fn y_formatter(mut self, formatter: impl Fn(f64) -> String + 'static) -> Self {
        self.y_formatter = Box::new(formatter);
        self
    }

    fn is_empty(&self) -> bool {
        self.series.iter().all(|s| s.points.is_empty())
    }

    fn render_canvas(&self) -> Result<Canvas, String> {
        let points = self
            .series
            .iter()
            .flat_map(|s| s.points.iter())
            .filter(|(x, y)| x.is_finite() && y.is_finite());
        let (mut x_min, mut x_max, mut y_min, mut y_max) = (f64::MAX, f64::MIN, 0.0_f64, f64::MIN);
        for (x, y) in points {
            x_min = x_min.min(*x);
            x_max = x_max.max(*x);
            y_min = y_min.min(*y);
            y_max = y_max.max(*y);
        }
        if x_max <= x_min {
            x_min -= 0.5;
            x_max += 0.5;
        }
        if y_max <= y_min {
            y_max = y_min + 1.0;
        }
        y_max += (y_max - y_min) * 0.05;

        let plot_rows = self
            .frame
            .height
            .saturating_sub(BOTTOM_ROWS)
            .max(MIN_PLOT_ROWS);
        let y_hint = (plot_rows / 3).max(2);
        let y_keys = RangedCoordf64::from(y_min..y_max).key_points(y_hint);
        let label_width = y_keys
            .iter()
            .map(|v| text_width(&(self.y_formatter)(*v)))
            .max()
            .unwrap_or(1);
        let x_label_width = text_width(&(self.x_formatter)(x_min))
            .max(text_width(&(self.x_formatter)(x_max)))
            .max(1);
        let left = label_width + 2;
        let right = x_label_width / 2 + 1;
        let layout = Layout {
            left,
            plot_cols: self
                .frame
                .width
                .saturating_sub(left + right)
                .max(MIN_PLOT_COLS),
            right,
            plot_rows,
        };

        let x_fmt = |value: &f64| (self.x_formatter)(*value);
        let y_fmt = |value: &f64| (self.y_formatter)(*value);
        let mesh = Mesh {
            x_range: x_min..x_max,
            y_range: y_min..y_max,
            x_labels: (layout.plot_cols / (x_label_width + 4)).max(2),
            y_labels: y_hint,
            x_fmt: &x_fmt,
            y_fmt: &y_fmt,
        };
        let max_markers = layout.plot_cols / 4;
        plot(&layout, mesh, |chart| {
            for (index, series) in self.series.iter().enumerate() {
                let color = rgb(PlotColor::series(index));
                let mut points: Vec<(f64, f64)> = series
                    .points
                    .iter()
                    .copied()
                    .filter(|(x, y)| x.is_finite() && y.is_finite())
                    .collect();
                points.sort_by(|a, b| a.0.total_cmp(&b.0));
                if points.len() > 1 {
                    chart.draw_series(LineSeries::new(points.iter().copied(), color))?;
                }
                if points.len() == 1 || points.len() <= max_markers {
                    chart.draw_series(
                        points
                            .iter()
                            .map(|point| Circle::new(*point, 1, color.filled())),
                    )?;
                }
            }
            Ok(())
        })
    }

    fn legend(&self) -> Vec<Line> {
        let options = &self.frame.options;
        let marker = if options.ascii { "--" } else { "━━" };
        let mut lines = Vec::new();
        let mut line = Line::default();
        let mut line_width = 0;
        for (index, series) in self.series.iter().enumerate() {
            let entry_width = text_width(marker) + 1 + text_width(&series.name) + 3;
            if line_width > 0 && line_width + entry_width > self.frame.width {
                lines.push(std::mem::take(&mut line));
                line_width = 0;
            }
            line.push(styled_span(marker, Some(PlotColor::series(index)), options));
            line.push(Span::new_unstyled_lossy(format!(" {}   ", series.name)));
            line_width += entry_width;
        }
        if line_width > 0 {
            lines.push(line);
        }
        lines
    }
}

impl Component for LineChart {
    fn render(&self) -> Vec<Line> {
        self.frame
            .render(self.is_empty(), || self.render_canvas(), || self.legend())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ascii() -> PlotOptions {
        PlotOptions {
            ascii: true,
            color: false,
        }
    }

    fn unicode() -> PlotOptions {
        PlotOptions {
            ascii: false,
            color: false,
        }
    }

    fn to_text(lines: Vec<Line>) -> String {
        lines
            .iter()
            .map(|line| line.to_unstyled().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn bar_chart_ascii() {
        let chart = BarChart::new(vec![
            ("build".to_string(), 10.0),
            ("check".to_string(), 5.0),
            ("fmt".to_string(), 0.0),
        ])
        .width(40)
        .options(ascii());
        let text = to_text(chart.render());
        let expected = [
            "build +############################# 10",
            "check +############### 5",
            "fmt   + 0",
            "      ++---------------------------+",
            "       0                           10",
        ]
        .join("\n");
        assert_eq!(text, expected);
    }

    #[test]
    fn bar_chart_unicode() {
        let chart = BarChart::new(vec![
            ("//spaces:build".to_string(), 9.74),
            ("//spaces:check".to_string(), 2.08),
            ("//:setup".to_string(), 0.0000072),
        ])
        .title("Slowest rules")
        .value_formatter(|v| format!("{v:.2}s"))
        .width(60)
        .options(unicode());
        let text = to_text(chart.render());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0], "Slowest rules");
        assert!(lines[1].starts_with("//spaces:build ┤███"));
        assert!(lines[1].ends_with("█ 9.74s"));
        assert!(lines[2].starts_with("//spaces:check ┤███"));
        assert!(lines[2].ends_with(" 2.08s"));
        // the tiny value still shows a sliver
        assert_eq!(lines[3], "//:setup       ┤▏ 0.00s");
        assert!(lines[4].starts_with("               └┬─"));
        let bar_len = |line: &str| line.chars().filter(|c| *c == '█').count();
        assert!(bar_len(lines[1]) > bar_len(lines[2]));
        assert!(lines[5].contains("5.00s"));
    }

    #[test]
    fn histogram_ascii() {
        let bins = (0..5)
            .map(|i| Bin {
                lo: i as f64,
                hi: (i + 1) as f64,
                count: [1, 4, 2, 0, 3][i],
            })
            .collect();
        let chart = HistogramChart::new(bins)
            .width(24)
            .height(6)
            .options(ascii());
        let text = to_text(chart.render());
        let expected = [
            "4 +   ##",
            "  |   ##       ##",
            "2 +   ## ##    ##",
            "0 +## ## ##    ##",
            "  ++--------------",
            "   0",
        ]
        .join("\n");
        assert_eq!(text, expected);
    }

    #[test]
    fn histogram_unicode() {
        let bins = (0..10)
            .map(|i| Bin {
                lo: 10f64.powi(i - 3),
                hi: 10f64.powi(i - 2),
                count: [0, 1, 3, 8, 12, 7, 2, 0, 1, 0][i as usize],
            })
            .collect();
        let chart = HistogramChart::new(bins)
            .title("spaces run")
            .x_formatter(|v| format!("{v}"))
            .footer("n=34")
            .width(50)
            .height(8)
            .options(unicode());
        let text = to_text(chart.render());
        let lines: Vec<&str> = text.lines().collect();
        // title + 6 plot rows + axis + labels + footer
        assert_eq!(lines.len(), 10);
        assert_eq!(lines[0], "spaces run");
        assert_eq!(lines[1], "   │                ███");
        assert!(lines[2].starts_with("10 ┤"));
        assert!(lines[6].starts_with(" 0 ┤    ▄▄▄ ███"));
        assert!(lines[7].starts_with("   └┬───"));
        assert!(lines[8].contains("0.001"));
        assert_eq!(lines[9], "n=34");
    }

    #[test]
    fn line_chart() {
        let series = vec![
            Series {
                name: "run".to_string(),
                points: (0..20).map(|i| (i as f64, (i * i) as f64)).collect(),
            },
            Series {
                name: "sync".to_string(),
                points: (0..20).map(|i| (i as f64, 100.0 + i as f64)).collect(),
            },
        ];
        let chart = LineChart::new(series.clone())
            .width(50)
            .height(10)
            .options(unicode());
        let text = to_text(chart.render());
        let lines: Vec<&str> = text.lines().collect();
        // 8 plot rows + axis + labels + legend
        assert_eq!(lines.len(), 11);
        assert!(lines.iter().any(|l| l.starts_with("200 ┤")));
        assert!(
            lines
                .iter()
                .any(|l| l.chars().any(|c| ('\u{2801}'..='\u{28FF}').contains(&c)))
        );
        assert!(lines[8].starts_with("    └┬─"));
        assert_eq!(lines[10], "━━ run   ━━ sync");

        let chart = LineChart::new(series).width(50).height(10).options(ascii());
        let text = to_text(chart.render());
        assert!(text.is_ascii());
        assert!(text.ends_with("-- run   -- sync"));
        for line in text.lines() {
            assert!(line.chars().count() <= 50);
        }
    }

    #[test]
    fn empty_and_degenerate() {
        let text = to_text(BarChart::new(vec![]).options(ascii()).render());
        assert_eq!(text, "No data to plot");
        let text = to_text(HistogramChart::new(vec![]).options(ascii()).render());
        assert_eq!(text, "No data to plot");
        let text = to_text(LineChart::new(vec![]).options(ascii()).render());
        assert_eq!(text, "No data to plot");

        // single point, tiny sizes, and non-finite values must not panic
        let chart = LineChart::new(vec![Series {
            name: "x".to_string(),
            points: vec![(5.0, f64::NAN), (1.0, 2.0)],
        }])
        .width(1)
        .height(1)
        .options(ascii());
        assert!(!chart.render().is_empty());
        let chart = BarChart::new(vec![("a".to_string(), f64::INFINITY)])
            .width(0)
            .options(ascii());
        assert!(!chart.render().is_empty());
        let chart = HistogramChart::new(vec![Bin {
            lo: 0.0,
            hi: 0.0,
            count: 0,
        }])
        .width(0)
        .height(0)
        .options(ascii());
        assert!(!chart.render().is_empty());
    }
}
