//! Terminal charts rendered with `plotters`.
//!
//! The chart components take plain data and render to `superconsole` lines, so callers
//! never depend on `plotters` directly.

mod backend;
mod charts;

pub use charts::{BarChart, Bin, HistogramChart, LineChart, Series};

pub use crate::bootstrap::terminal_width;

use crate::bootstrap::{TypographyMode, Variant, typography_mode};
use crossterm::style::StyledContent;
use superconsole::{Line, Span};

/// Rendering options shared by all charts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlotOptions {
    /// Only use ASCII glyphs (no box drawing, blocks, or braille).
    pub ascii: bool,
    /// Emit colored spans.
    pub color: bool,
}

impl Default for PlotOptions {
    fn default() -> Self {
        Self {
            ascii: typography_mode() == TypographyMode::Ascii,
            color: true,
        }
    }
}

/// Palette used for chart elements. Series `i` uses `PlotColor::series(i)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlotColor {
    Primary,
    Info,
    Success,
    Warning,
    Danger,
    Secondary,
}

impl PlotColor {
    pub const PALETTE: [PlotColor; 6] = [
        PlotColor::Primary,
        PlotColor::Info,
        PlotColor::Success,
        PlotColor::Warning,
        PlotColor::Danger,
        PlotColor::Secondary,
    ];

    pub fn series(index: usize) -> Self {
        Self::PALETTE[index % Self::PALETTE.len()]
    }

    pub fn variant(self) -> Variant {
        match self {
            PlotColor::Primary => Variant::Primary,
            PlotColor::Info => Variant::Info,
            PlotColor::Success => Variant::Success,
            PlotColor::Warning => Variant::Warning,
            PlotColor::Danger => Variant::Danger,
            PlotColor::Secondary => Variant::Secondary,
        }
    }

    /// RGB keys used to pass the palette through plotters. Any other color
    /// (for example the black axes and labels) renders unstyled.
    pub(crate) fn rgb(self) -> (u8, u8, u8) {
        match self {
            PlotColor::Primary => (13, 110, 253),
            PlotColor::Info => (13, 202, 240),
            PlotColor::Success => (25, 135, 84),
            PlotColor::Warning => (255, 193, 7),
            PlotColor::Danger => (220, 53, 69),
            PlotColor::Secondary => (108, 117, 125),
        }
    }

    pub(crate) fn from_rgb(rgb: (u8, u8, u8)) -> Option<Self> {
        Self::PALETTE.iter().copied().find(|c| c.rgb() == rgb)
    }
}

pub(crate) fn styled_span(text: &str, color: Option<PlotColor>, options: &PlotOptions) -> Span {
    match color {
        Some(color) if options.color => Span::new_styled_lossy(StyledContent::new(
            color.variant().style(),
            text.to_string(),
        )),
        _ => Span::new_unstyled_lossy(text),
    }
}

pub(crate) fn runs_to_line(runs: Vec<backend::Run>, options: &PlotOptions) -> Line {
    let mut line = Line::default();
    for (text, color) in runs {
        line.push(styled_span(&text, color, options));
    }
    line
}
