use crossterm::style::{self, Attribute, Attributes, Color, ContentStyle, StyledContent};
use superconsole::{Line, Span};

// ---------------------------------------------------------------------------
// Duration formatting
// ---------------------------------------------------------------------------

/// Formats a duration in minutes and seconds, e.g. `1m30s` or `2.5s`.
pub fn format_duration(secs: f64) -> String {
    if secs > 100.0 {
        let mins = secs as u64 / 60;
        let remaining_secs = secs as u64 % 60;
        format!("{mins}m{remaining_secs:02}s")
    } else if secs > 10.0 {
        format!("{secs:.1}s")
    } else {
        format!("{secs:.2}s")
    }
}

// ---------------------------------------------------------------------------
// Progress bar rendering
// ---------------------------------------------------------------------------

/// State for a single actively running process shown as a progress bar.
/// Mirrors the indicatif template: `{elapsed_precise}|{bar:.cyan/blue}|{name}: {message}`
pub struct ActiveProgress {
    pub name: String,
    pub prefix: String,
    pub message: String,
    /// Current progress position.
    pub position: u64,
    /// Total steps, or `None` for an indeterminate spinner.
    pub total: Option<u64>,
    pub start_time: std::time::Instant,
    pub variant: Variant,
}

/// Width of the bar fill section in characters.
const BAR_WIDTH: usize = 16;

/// Characters used for a bounded bar: filled, tip, empty.
const BAR_CHARS_BOUNDED: (char, char, char) = ('#', '>', '-');

/// Frames for the indeterminate spinner, cycled by time elapsed.
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// How many milliseconds each spinner frame is shown.
const SPINNER_FRAME_MS: u128 = 100;

impl ActiveProgress {
    pub fn new(
        name: String,
        prefix: String,
        message: String,
        position: u64,
        total: Option<u64>,
    ) -> Self {
        Self {
            name,
            prefix,
            message,
            position,
            total,
            start_time: std::time::Instant::now(),
            variant: Variant::Info,
        }
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub fn render_bar(&self, max_width: usize) -> anyhow::Result<Line> {
        let elapsed = self.start_time.elapsed();
        let secs = elapsed.as_secs_f64();
        let elapsed_str = format!("  {}", format_duration(secs));

        let bar_str: String = if let Some(total) = self.total {
            let (filled_char, tip_char, empty_char) = BAR_CHARS_BOUNDED;
            let filled_count = if total == 0 {
                0
            } else {
                (self.position as usize * BAR_WIDTH / total as usize).min(BAR_WIDTH)
            };
            let filled = filled_char
                .to_string()
                .repeat(filled_count.saturating_sub(1));
            let tip = if filled_count > 0 && filled_count < BAR_WIDTH {
                tip_char.to_string()
            } else if filled_count > 0 {
                filled_char.to_string()
            } else {
                String::new()
            };
            let empty = empty_char
                .to_string()
                .repeat(BAR_WIDTH.saturating_sub(filled_count));
            format!("{filled}{tip}{empty}")
        } else {
            let frame_idx =
                (elapsed.as_millis() / SPINNER_FRAME_MS) as usize % SPINNER_FRAMES.len();
            SPINNER_FRAMES[frame_idx].to_string()
        };

        let bar_width = if self.total.is_some() { BAR_WIDTH } else { 1 };

        let bar_style = self.variant.style();

        let prefix_style = ContentStyle {
            foreground_color: self.variant.style().foreground_color,
            background_color: None,
            underline_color: None,
            attributes: Attributes::from(Attribute::Bold),
        };

        // Format: `[H:MM:SS]|<bar>|name: message`
        let mut line = Line::default();
        line.push(Span::new_unstyled_lossy(&elapsed_str));
        line.push(Span::new_unstyled_lossy("|"));

        let bar_span = Span::new_styled_lossy(StyledContent::new(bar_style, bar_str));
        line.push(bar_span);

        line.push(Span::new_unstyled_lossy("|"));

        let prefix_text = format!("{}: ", self.prefix);
        let prefix_span = Span::new_styled_lossy(StyledContent::new(prefix_style, prefix_text));
        line.push(prefix_span);

        // Truncate message to fit remaining width
        let fixed_width = elapsed_str.len() + 1 + bar_width + 1 + self.prefix.len() + 2;
        let msg_max = max_width.saturating_sub(fixed_width);
        let message: String = if self.message.chars().count() > msg_max {
            let truncated: String = self
                .message
                .chars()
                .take(msg_max.saturating_sub(4))
                .collect();
            format!("{}...  ", truncated)
        } else {
            self.message.chars().take(msg_max).collect()
        };
        line.push(Span::new_unstyled_lossy(&message));

        Ok(line)
    }
}

// ---------------------------------------------------------------------------
// Shared ContentStyle helpers (migrated from lib.rs)
// ---------------------------------------------------------------------------

pub fn primary_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::Blue),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

pub fn default_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::DarkGrey),
        background_color: None,
        underline_color: None,
        attributes: Attributes::default(),
    }
}

pub fn info_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::Cyan),
        background_color: None,
        underline_color: None,
        attributes: Attributes::default(),
    }
}

pub fn success_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::DarkGreen),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

pub fn warning_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::DarkYellow),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

pub fn danger_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::DarkRed),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

pub fn bold_style() -> ContentStyle {
    ContentStyle {
        foreground_color: None,
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

// ---------------------------------------------------------------------------
// Typography API - Bootstrap-inspired elements
// ---------------------------------------------------------------------------

/// Typography variant (Bootstrap-inspired color schemes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Success,
    Danger,
    Warning,
    Info,
    Light,
    Dark,
    Default,
}

impl Variant {
    pub fn style(&self) -> ContentStyle {
        match self {
            Variant::Primary => primary_style(),
            Variant::Secondary => ContentStyle {
                foreground_color: Some(Color::Grey),
                background_color: None,
                underline_color: None,
                attributes: Attributes::default(),
            },
            Variant::Success => success_style(),
            Variant::Danger => danger_style(),
            Variant::Warning => warning_style(),
            Variant::Info => info_style(),
            Variant::Light => ContentStyle {
                foreground_color: Some(Color::White),
                background_color: None,
                underline_color: None,
                attributes: Attributes::default(),
            },
            Variant::Dark => ContentStyle {
                foreground_color: Some(Color::Black),
                background_color: None,
                underline_color: None,
                attributes: Attributes::from(Attribute::Bold),
            },
            Variant::Default => default_style(),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions for creating styled spans
// ---------------------------------------------------------------------------

fn styled_span(style: ContentStyle, text: String) -> Span {
    Span::new_styled_lossy(style::StyledContent::new(style, text))
}

fn unstyled_span(text: String) -> Span {
    Span::new_unstyled_lossy(text)
}

// ---------------------------------------------------------------------------
// Header component
// ---------------------------------------------------------------------------

/// Header levels (h1-h6)
#[derive(Debug, Clone, Copy)]
pub enum HeaderLevel {
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
}

/// Header component for displaying titles and section headers
pub struct Header {
    level: HeaderLevel,
    text: String,
    variant: Variant,
}

impl Header {
    pub fn new(level: HeaderLevel, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
            variant: Variant::Primary,
        }
    }

    pub fn h1(text: impl Into<String>) -> Self {
        Self::new(HeaderLevel::H1, text)
    }

    pub fn h2(text: impl Into<String>) -> Self {
        Self::new(HeaderLevel::H2, text)
    }

    pub fn h3(text: impl Into<String>) -> Self {
        Self::new(HeaderLevel::H3, text)
    }

    pub fn h4(text: impl Into<String>) -> Self {
        Self::new(HeaderLevel::H4, text)
    }

    pub fn h5(text: impl Into<String>) -> Self {
        Self::new(HeaderLevel::H5, text)
    }

    pub fn h6(text: impl Into<String>) -> Self {
        Self::new(HeaderLevel::H6, text)
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Render header as multiple lines (includes blank line before, underline for h1/h2, blank line after)
    pub fn render(&self) -> Vec<Line> {
        let style = self.variant.style();
        let mut lines = Vec::new();

        // Blank line before
        lines.push(Line::default());

        // Main header line
        let mut header_line = Line::default();
        match self.level {
            HeaderLevel::H1 | HeaderLevel::H2 => {
                header_line.push(styled_span(style, self.text.clone()));
            }
            HeaderLevel::H3 => {
                header_line.push(unstyled_span("### ".to_string()));
                header_line.push(styled_span(style, self.text.clone()));
            }
            HeaderLevel::H4 => {
                header_line.push(unstyled_span("#### ".to_string()));
                header_line.push(styled_span(style, self.text.clone()));
            }
            HeaderLevel::H5 => {
                header_line.push(unstyled_span("##### ".to_string()));
                header_line.push(styled_span(style, self.text.clone()));
            }
            HeaderLevel::H6 => {
                header_line.push(unstyled_span("###### ".to_string()));
                header_line.push(styled_span(style, self.text.clone()));
            }
        }
        lines.push(header_line);

        // Underline for H1 and H2
        match self.level {
            HeaderLevel::H1 => {
                let mut underline = Line::default();
                underline.push(styled_span(style, "=".repeat(self.text.len())));
                lines.push(underline);
            }
            HeaderLevel::H2 => {
                let mut underline = Line::default();
                underline.push(styled_span(style, "-".repeat(self.text.len())));
                lines.push(underline);
            }
            _ => {}
        }

        // Blank line after
        lines.push(Line::default());

        lines
    }
}

// ---------------------------------------------------------------------------
// Divider component
// ---------------------------------------------------------------------------

/// Divider styles
#[derive(Debug, Clone, Copy)]
pub enum DividerStyle {
    Solid,
    Dashed,
    Dotted,
    Double,
}

/// Divider component for visual separation
pub struct Divider {
    style: DividerStyle,
    width: Option<usize>,
    variant: Variant,
}

impl Divider {
    pub fn new() -> Self {
        Self {
            style: DividerStyle::Solid,
            width: None,
            variant: Variant::Default,
        }
    }

    pub fn style(mut self, style: DividerStyle) -> Self {
        self.style = style;
        self
    }

    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    fn get_char(&self) -> &str {
        match self.style {
            DividerStyle::Solid => "─",
            DividerStyle::Dashed => "╌",
            DividerStyle::Dotted => "·",
            DividerStyle::Double => "═",
        }
    }

    pub fn render(&self) -> Line {
        let width = self.width.unwrap_or(80);
        let line_str = self.get_char().repeat(width);
        let style = self.variant.style();

        let mut line = Line::default();
        line.push(styled_span(style, line_str));
        line
    }
}

impl Default for Divider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Paragraph component
// ---------------------------------------------------------------------------

/// Paragraph component for body text
pub struct Paragraph {
    text: String,
    variant: Variant,
}

impl Paragraph {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            variant: Variant::Default,
        }
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub fn render(&self) -> Line {
        let style = self.variant.style();
        let mut line = Line::default();
        line.push(styled_span(style, self.text.clone()));
        line
    }
}

// ---------------------------------------------------------------------------
// List component
// ---------------------------------------------------------------------------

/// List style (ordered or unordered)
#[derive(Debug, Clone, Copy)]
pub enum ListStyle {
    Unordered,
    Ordered,
}

/// List component for displaying items
pub struct List {
    items: Vec<String>,
    style: ListStyle,
    variant: Variant,
}

impl List {
    pub fn new(style: ListStyle) -> Self {
        Self {
            items: Vec::new(),
            style,
            variant: Variant::Default,
        }
    }

    pub fn unordered() -> Self {
        Self::new(ListStyle::Unordered)
    }

    pub fn ordered() -> Self {
        Self::new(ListStyle::Ordered)
    }

    pub fn item(mut self, item: impl Into<String>) -> Self {
        self.items.push(item.into());
        self
    }

    pub fn items(mut self, items: Vec<String>) -> Self {
        self.items = items;
        self
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub fn render(&self) -> Vec<Line> {
        let style = self.variant.style();
        let mut lines = Vec::new();

        for (idx, item) in self.items.iter().enumerate() {
            let mut line = Line::default();

            let prefix = match self.style {
                ListStyle::Unordered => "  • ".to_string(),
                ListStyle::Ordered => format!("  {}. ", idx + 1),
            };

            line.push(unstyled_span(prefix));
            line.push(styled_span(style, item.clone()));
            lines.push(line);
        }

        lines
    }
}

// ---------------------------------------------------------------------------
// Link component
// ---------------------------------------------------------------------------

/// Link component for displaying URLs or references
pub struct Link {
    text: String,
    url: Option<String>,
    variant: Variant,
}

impl Link {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            url: None,
            variant: Variant::Info,
        }
    }

    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub fn render(&self) -> Line {
        let mut style = self.variant.style();
        style.attributes = style.attributes.with(Attribute::Underlined);

        let mut line = Line::default();
        line.push(styled_span(style, self.text.clone()));

        if let Some(url) = &self.url {
            line.push(unstyled_span(format!(" ({})", url)));
        }

        line
    }
}

// ---------------------------------------------------------------------------
// Blockquote component
// ---------------------------------------------------------------------------

/// Blockquote component for quotations
pub struct Blockquote {
    text: String,
    variant: Variant,
}

impl Blockquote {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            variant: Variant::Secondary,
        }
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub fn render(&self) -> Vec<Line> {
        let style = self.variant.style();
        let mut lines = Vec::new();

        for text_line in self.text.lines() {
            let mut line = Line::default();
            line.push(unstyled_span("│ ".to_string()));
            line.push(styled_span(style, text_line.to_string()));
            lines.push(line);
        }

        lines
    }
}

// ---------------------------------------------------------------------------
// Table component
// ---------------------------------------------------------------------------

/// Table alignment
#[derive(Debug, Clone, Copy)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Table component for tabular data
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    alignments: Vec<Align>,
    variant: Variant,
}

impl Table {
    pub fn new() -> Self {
        Self {
            headers: Vec::new(),
            rows: Vec::new(),
            alignments: Vec::new(),
            variant: Variant::Default,
        }
    }

    pub fn headers(mut self, headers: Vec<String>) -> Self {
        self.alignments = vec![Align::Left; headers.len()];
        self.headers = headers;
        self
    }

    pub fn alignments(mut self, alignments: Vec<Align>) -> Self {
        self.alignments = alignments;
        self
    }

    pub fn row(mut self, row: Vec<String>) -> Self {
        self.rows.push(row);
        self
    }

    pub fn rows(mut self, rows: Vec<Vec<String>>) -> Self {
        self.rows = rows;
        self
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    fn calculate_column_widths(&self) -> Vec<usize> {
        let mut widths = self.headers.iter().map(|h| h.len()).collect::<Vec<_>>();

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        widths
    }

    fn format_cell(&self, content: &str, width: usize, align: Align) -> String {
        match align {
            Align::Left => format!("{:<width$}", content, width = width),
            Align::Center => {
                let total_padding = width.saturating_sub(content.len());
                let left_padding = total_padding / 2;
                let right_padding = total_padding - left_padding;
                format!(
                    "{}{}{}",
                    " ".repeat(left_padding),
                    content,
                    " ".repeat(right_padding)
                )
            }
            Align::Right => format!("{:>width$}", content, width = width),
        }
    }

    pub fn render(&self) -> Vec<Line> {
        if self.headers.is_empty() {
            return Vec::new();
        }

        let widths = self.calculate_column_widths();
        let style = self.variant.style();
        let header_style = ContentStyle {
            foreground_color: style.foreground_color,
            background_color: style.background_color,
            underline_color: style.underline_color,
            attributes: style.attributes.with(Attribute::Bold),
        };

        let mut lines = Vec::new();

        // Top border
        let mut top_line = Line::default();
        let mut top_str = String::from("┌");
        for (i, width) in widths.iter().enumerate() {
            top_str.push_str(&"─".repeat(width + 2));
            if i < widths.len() - 1 {
                top_str.push('┬');
            }
        }
        top_str.push('┐');
        top_line.push(unstyled_span(top_str));
        lines.push(top_line);

        // Headers
        let mut header_line = Line::default();
        header_line.push(unstyled_span("│".to_string()));
        for (i, (header, width)) in self.headers.iter().zip(widths.iter()).enumerate() {
            let align = self.alignments.get(i).copied().unwrap_or(Align::Left);
            let formatted = self.format_cell(header, *width, align);
            header_line.push(unstyled_span(" ".to_string()));
            header_line.push(styled_span(header_style, formatted));
            header_line.push(unstyled_span(" │".to_string()));
        }
        lines.push(header_line);

        // Header separator
        let mut sep_line = Line::default();
        let mut sep_str = String::from("├");
        for (i, width) in widths.iter().enumerate() {
            sep_str.push_str(&"─".repeat(width + 2));
            if i < widths.len() - 1 {
                sep_str.push('┼');
            }
        }
        sep_str.push('┤');
        sep_line.push(unstyled_span(sep_str));
        lines.push(sep_line);

        // Rows
        for row in &self.rows {
            let mut row_line = Line::default();
            row_line.push(unstyled_span("│".to_string()));
            for (i, width) in widths.iter().enumerate() {
                let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
                let align = self.alignments.get(i).copied().unwrap_or(Align::Left);
                let formatted = self.format_cell(cell, *width, align);
                row_line.push(unstyled_span(" ".to_string()));
                row_line.push(styled_span(style, formatted));
                row_line.push(unstyled_span(" │".to_string()));
            }
            lines.push(row_line);
        }

        // Bottom border
        let mut bottom_line = Line::default();
        let mut bottom_str = String::from("└");
        for (i, width) in widths.iter().enumerate() {
            bottom_str.push_str(&"─".repeat(width + 2));
            if i < widths.len() - 1 {
                bottom_str.push('┴');
            }
        }
        bottom_str.push('┘');
        bottom_line.push(unstyled_span(bottom_str));
        lines.push(bottom_line);

        lines
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Card component
// ---------------------------------------------------------------------------

/// Card component for grouped content
pub struct Card {
    title: Option<String>,
    body: String,
    variant: Variant,
    width: Option<usize>,
}

impl Card {
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            title: None,
            body: body.into(),
            variant: Variant::Default,
            width: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    pub fn render(&self) -> Vec<Line> {
        let width = self.width.unwrap_or(60);
        let style = self.variant.style();
        let title_style = ContentStyle {
            foreground_color: style.foreground_color,
            background_color: style.background_color,
            underline_color: style.underline_color,
            attributes: style.attributes.with(Attribute::Bold),
        };

        let mut lines = Vec::new();

        // Top border
        let mut top_line = Line::default();
        top_line.push(unstyled_span(format!("┌{}┐", "─".repeat(width - 2))));
        lines.push(top_line);

        // Title
        if let Some(title) = &self.title {
            let mut title_line = Line::default();
            title_line.push(unstyled_span("│ ".to_string()));
            title_line.push(styled_span(
                title_style,
                format!("{:<width$}", title, width = width - 4),
            ));
            title_line.push(unstyled_span(" │".to_string()));
            lines.push(title_line);

            let mut sep_line = Line::default();
            sep_line.push(unstyled_span(format!("├{}┤", "─".repeat(width - 2))));
            lines.push(sep_line);
        }

        // Body
        for body_line in self.body.lines() {
            let mut line = Line::default();
            line.push(unstyled_span("│ ".to_string()));
            line.push(styled_span(
                style,
                format!("{:<width$}", body_line, width = width - 4),
            ));
            line.push(unstyled_span(" │".to_string()));
            lines.push(line);
        }

        // Bottom border
        let mut bottom_line = Line::default();
        bottom_line.push(unstyled_span(format!("└{}┘", "─".repeat(width - 2))));
        lines.push(bottom_line);

        lines
    }
}

// ---------------------------------------------------------------------------
// Histogram component
// ---------------------------------------------------------------------------

/// A bar in a histogram
#[derive(Debug, Clone)]
pub struct HistogramBar {
    label: String,
    value: usize,
    variant: Variant,
}

impl HistogramBar {
    pub fn new(label: impl Into<String>, value: usize) -> Self {
        Self {
            label: label.into(),
            value,
            variant: Variant::Default,
        }
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }
}

/// A histogram component for displaying bar charts
#[derive(Debug, Clone)]
pub struct Histogram {
    title: String,
    bars: Vec<HistogramBar>,
    bar_width: usize,
    variant: Variant,
}

impl Histogram {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            bars: Vec::new(),
            bar_width: 20,
            variant: Variant::Default,
        }
    }

    pub fn bar_width(mut self, width: usize) -> Self {
        self.bar_width = width;
        self
    }

    pub fn bar(mut self, bar: HistogramBar) -> Self {
        self.bars.push(bar);
        self
    }

    pub fn bars(mut self, bars: Vec<HistogramBar>) -> Self {
        self.bars = bars;
        self
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub fn render(&self) -> Vec<Line> {
        let mut lines = Vec::new();

        // Print title with bold style
        let title_style = bold_style();
        let mut title_line = Line::default();
        title_line.push(styled_span(title_style, self.title.clone()));
        lines.push(title_line);

        if self.bars.is_empty() {
            return lines;
        }

        // Find the maximum value to normalize bar lengths
        let max_value = self.bars.iter().map(|b| b.value).max().unwrap_or(1).max(1);

        // Find the longest label for alignment
        let max_label_len = self.bars.iter().map(|b| b.label.len()).max().unwrap_or(0);

        // Render each bar
        for bar in &self.bars {
            let bar_len = if max_value > 0 {
                bar.value * self.bar_width / max_value
            } else {
                0
            };
            let bar_str = "█".repeat(bar_len);

            let mut bar_line = Line::default();

            // Label
            bar_line.push(styled_span(
                default_style(),
                format!("  {:width$}  ", bar.label, width = max_label_len),
            ));

            // Bar
            bar_line.push(styled_span(
                bar.variant.style(),
                format!("{:width$}", bar_str, width = self.bar_width),
            ));

            // Count
            bar_line.push(unstyled_span(format!("  {}", bar.value)));

            lines.push(bar_line);
        }

        lines
    }
}

// ---------------------------------------------------------------------------
// Convenience functions for quick formatting
// ---------------------------------------------------------------------------

pub fn h1(text: impl Into<String>) -> Vec<Line> {
    Header::h1(text).render()
}

pub fn h2(text: impl Into<String>) -> Vec<Line> {
    Header::h2(text).render()
}

pub fn h3(text: impl Into<String>) -> Vec<Line> {
    Header::h3(text).render()
}

pub fn h4(text: impl Into<String>) -> Vec<Line> {
    Header::h4(text).render()
}

pub fn h5(text: impl Into<String>) -> Vec<Line> {
    Header::h5(text).render()
}

pub fn h6(text: impl Into<String>) -> Vec<Line> {
    Header::h6(text).render()
}

pub fn divider() -> Line {
    Divider::new().render()
}

pub fn paragraph(text: impl Into<String>) -> Line {
    Paragraph::new(text).render()
}

pub fn blockquote(text: impl Into<String>) -> Vec<Line> {
    Blockquote::new(text).render()
}

pub fn link(text: impl Into<String>) -> Line {
    Link::new(text).render()
}

pub fn link_with_url(text: impl Into<String>, url: impl Into<String>) -> Line {
    Link::new(text).url(url).render()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_render() {
        let h1 = Header::h1("Welcome");
        let lines = h1.render();
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_divider() {
        let div = Divider::new().width(20);
        let line = div.render();
        // Line should not be empty
        assert!(line.len() > 0);
    }

    #[test]
    fn test_list() {
        let list = List::unordered().item("First").item("Second").item("Third");
        let lines = list.render();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_table() {
        let table = Table::new()
            .headers(vec!["Name".to_string(), "Age".to_string()])
            .row(vec!["Alice".to_string(), "30".to_string()])
            .row(vec!["Bob".to_string(), "25".to_string()]);
        let lines = table.render();
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_card() {
        let card = Card::new("This is the body").title("Card Title").width(40);
        let lines = card.render();
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_variants() {
        assert!(primary_style().foreground_color.is_some());
        assert!(success_style().foreground_color.is_some());
        assert!(danger_style().foreground_color.is_some());
        assert!(warning_style().foreground_color.is_some());
        assert!(info_style().foreground_color.is_some());
    }

    #[test]
    fn test_histogram() {
        let histogram = Histogram::new("Age distribution")
            .bar_width(20)
            .bar(HistogramBar::new("fresh  < 7d", 5).variant(Variant::Success))
            .bar(HistogramBar::new("aging 7-30d", 3).variant(Variant::Warning))
            .bar(HistogramBar::new("stale  > 30d", 2).variant(Variant::Danger));
        let lines = histogram.render();
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_active_progress_variant() {
        let progress = ActiveProgress::new(
            "test-task".to_string(),
            "Building".to_string(),
            "Compiling...".to_string(),
            50,
            Some(100),
        )
        .variant(Variant::Success);

        assert_eq!(progress.variant, Variant::Success);

        // Test that it renders without errors
        let line = progress.render_bar(80);
        assert!(line.is_ok());
    }

    #[test]
    fn test_active_progress_default_variant() {
        let progress = ActiveProgress::new(
            "test-task".to_string(),
            "Building".to_string(),
            "Compiling...".to_string(),
            50,
            Some(100),
        );

        // Default variant should be Info
        assert_eq!(progress.variant, Variant::Info);
    }
}
