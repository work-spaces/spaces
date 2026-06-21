use crossterm::style::{self, Attribute, Attributes, Color, ContentStyle, StyledContent, Stylize};
use std::sync::OnceLock;
use superconsole::{Line, Span};
use termwiz::cell::Hyperlink;

// ---------------------------------------------------------------------------
// Component Trait
// ---------------------------------------------------------------------------

/// A trait for all typography components that can be rendered to lines.
pub trait Component {
    /// Renders the component as a vector of lines.
    fn render(&self) -> Vec<Line>;
}

// ---------------------------------------------------------------------------
// Typography Mode Configuration
// ---------------------------------------------------------------------------

/// The character set mode for typography rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypographyMode {
    /// ASCII-only characters (maximum compatibility)
    Ascii,
    /// Full Unicode box-drawing and special characters
    #[default]
    Unicode,
    /// Unicode with Nerd Fonts icons (future extension)
    #[allow(dead_code)]
    NerdFonts,
}

static TYPOGRAPHY_MODE: OnceLock<TypographyMode> = OnceLock::new();

/// Sets the global typography mode. This can only be called once.
/// Returns `Ok(())` if the mode was set successfully, or `Err(())` if it was already set.
pub fn set_typography_mode(mode: TypographyMode) -> anyhow::Result<()> {
    TYPOGRAPHY_MODE
        .set(mode)
        .map_err(|_| anyhow::anyhow!("Typography mode already set"))
}

/// Gets the current typography mode. Defaults to Unicode if not explicitly set.
pub fn typography_mode() -> TypographyMode {
    *TYPOGRAPHY_MODE.get_or_init(TypographyMode::default)
}

// ---------------------------------------------------------------------------
// Character Set Definitions
// ---------------------------------------------------------------------------

/// Characters used for a bounded progress bar: filled, tip, empty.
struct BarCharsBounded {
    filled: char,
    tip: char,
    empty: char,
}

const BAR_CHARS_BOUNDED_ASCII: BarCharsBounded = BarCharsBounded {
    filled: '#',
    tip: '>',
    empty: '-',
};

const BAR_CHARS_BOUNDED_UNICODE: BarCharsBounded = BarCharsBounded {
    filled: '█',
    tip: '▒',
    empty: '░',
};

fn get_bar_chars_bounded() -> &'static BarCharsBounded {
    match typography_mode() {
        TypographyMode::Ascii => &BAR_CHARS_BOUNDED_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => &BAR_CHARS_BOUNDED_UNICODE,
    }
}

/// Spinner frames for indeterminate progress.
const SPINNER_FRAMES_ASCII: &[char] = &['|', '/', '-', '\\'];
const SPINNER_FRAMES_UNICODE: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn get_spinner_frames() -> &'static [char] {
    match typography_mode() {
        TypographyMode::Ascii => SPINNER_FRAMES_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => SPINNER_FRAMES_UNICODE,
    }
}

/// Box-drawing characters for tables and cards.
struct BoxChars {
    // Corners
    top_left: &'static str,
    top_right: &'static str,
    bottom_left: &'static str,
    bottom_right: &'static str,
    // Lines
    horizontal: &'static str,
    vertical: &'static str,
    // T-junctions
    left_t: &'static str,
    right_t: &'static str,
    top_t: &'static str,
    bottom_t: &'static str,
    cross: &'static str,
}

const BOX_CHARS_ASCII: BoxChars = BoxChars {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    horizontal: "-",
    vertical: "|",
    left_t: "+",
    right_t: "+",
    top_t: "+",
    bottom_t: "+",
    cross: "+",
};

const BOX_CHARS_UNICODE: BoxChars = BoxChars {
    top_left: "╭",
    top_right: "╮",
    bottom_left: "╰",
    bottom_right: "╯",
    horizontal: "─",
    vertical: "│",
    left_t: "├",
    right_t: "┤",
    top_t: "┬",
    bottom_t: "┴",
    cross: "┼",
};

fn get_box_chars() -> &'static BoxChars {
    match typography_mode() {
        TypographyMode::Ascii => &BOX_CHARS_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => &BOX_CHARS_UNICODE,
    }
}

/// Characters for divider styles.
struct DividerChars {
    solid: &'static str,
    dashed: &'static str,
    dotted: &'static str,
    double: &'static str,
}

const DIVIDER_CHARS_ASCII: DividerChars = DividerChars {
    solid: "-",
    dashed: "-",
    dotted: ".",
    double: "=",
};

const DIVIDER_CHARS_UNICODE: DividerChars = DividerChars {
    solid: "─",
    dashed: "╌",
    dotted: "·",
    double: "═",
};

fn get_divider_chars() -> &'static DividerChars {
    match typography_mode() {
        TypographyMode::Ascii => &DIVIDER_CHARS_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => &DIVIDER_CHARS_UNICODE,
    }
}

/// Characters for histogram bars.
const HISTOGRAM_BAR_ASCII: char = '#';
const HISTOGRAM_BAR_UNICODE: char = '█';

fn get_histogram_bar_char() -> char {
    match typography_mode() {
        TypographyMode::Ascii => HISTOGRAM_BAR_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => HISTOGRAM_BAR_UNICODE,
    }
}

/// Characters for header underlines.
struct HeaderUnderlineChars {
    h1: &'static str,
    h2: &'static str,
}

const HEADER_UNDERLINE_ASCII: HeaderUnderlineChars = HeaderUnderlineChars { h1: "=", h2: "-" };

const HEADER_UNDERLINE_UNICODE: HeaderUnderlineChars = HeaderUnderlineChars {
    h1: "═", h2: "─"
};

fn get_header_underline_chars() -> &'static HeaderUnderlineChars {
    match typography_mode() {
        TypographyMode::Ascii => &HEADER_UNDERLINE_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => &HEADER_UNDERLINE_UNICODE,
    }
}

/// Characters for blockquote prefix.
const BLOCKQUOTE_PREFIX_ASCII: &str = "| ";
const BLOCKQUOTE_PREFIX_UNICODE: &str = "▐ ";

fn get_blockquote_prefix() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => BLOCKQUOTE_PREFIX_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => BLOCKQUOTE_PREFIX_UNICODE,
    }
}

/// Icon characters for status indicators.
const ICON_SUCCESS_UNICODE: &str = "✓";
const ICON_DANGER_UNICODE: &str = "x";
const ICON_WARNING_UNICODE: &str = "▲";
const ICON_INFO_UNICODE: &str = "ℹ";

/// Returns a success icon (checkmark) in Unicode mode, empty string in ASCII mode.
pub fn icon_success() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => "",
        TypographyMode::Unicode | TypographyMode::NerdFonts => ICON_SUCCESS_UNICODE,
    }
}

/// Returns a danger icon (X mark) in Unicode mode, empty string in ASCII mode.
pub fn icon_danger() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => "",
        TypographyMode::Unicode | TypographyMode::NerdFonts => ICON_DANGER_UNICODE,
    }
}

/// Returns a warning icon (warning sign) in Unicode mode, empty string in ASCII mode.
pub fn icon_warning() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => "",
        TypographyMode::Unicode | TypographyMode::NerdFonts => ICON_WARNING_UNICODE,
    }
}

/// Returns an info icon (information symbol) in Unicode mode, empty string in ASCII mode.
pub fn icon_info() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => "",
        TypographyMode::Unicode | TypographyMode::NerdFonts => ICON_INFO_UNICODE,
    }
}

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
            let bar_chars = get_bar_chars_bounded();
            let filled_count = if total == 0 {
                0
            } else {
                (self.position as usize * BAR_WIDTH / total as usize).min(BAR_WIDTH)
            };
            let filled = bar_chars
                .filled
                .to_string()
                .repeat(filled_count.saturating_sub(1));
            let tip = if filled_count > 0 && filled_count < BAR_WIDTH {
                bar_chars.tip.to_string()
            } else if filled_count > 0 {
                bar_chars.filled.to_string()
            } else {
                String::new()
            };
            let empty = bar_chars
                .empty
                .to_string()
                .repeat(BAR_WIDTH.saturating_sub(filled_count));
            format!("{filled}{tip}{empty}")
        } else {
            let spinner_frames = get_spinner_frames();
            let frame_idx =
                (elapsed.as_millis() / SPINNER_FRAME_MS) as usize % spinner_frames.len();
            spinner_frames[frame_idx].to_string()
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
            const ELLIPSIS: &str = "...  ";
            const ELLIPSIS_LEN: usize = 5;

            if msg_max < ELLIPSIS_LEN {
                // Not enough room for ellipsis, return empty or truncated ellipsis
                if msg_max == 0 {
                    String::new()
                } else {
                    ELLIPSIS.chars().take(msg_max).collect()
                }
            } else {
                let truncated: String = self.message.chars().take(msg_max - ELLIPSIS_LEN).collect();
                format!("{}{}", truncated, ELLIPSIS)
            }
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

/// Creates a primary style (blue, bold) - Bootstrap's primary color scheme
pub fn primary_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::Blue),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

/// Creates a secondary style (grey) - Bootstrap's secondary color scheme
pub fn secondary_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::Grey),
        background_color: None,
        underline_color: None,
        attributes: Attributes::default(),
    }
}

/// Creates a default/muted style (dark grey) - for less prominent text
pub fn default_style() -> ContentStyle {
    ContentStyle {
        foreground_color: None,
        background_color: None,
        underline_color: None,
        attributes: Attributes::default(),
    }
}

/// Creates an info style (cyan) - Bootstrap's info color scheme
pub fn info_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::Cyan),
        background_color: None,
        underline_color: None,
        attributes: Attributes::default(),
    }
}

/// Creates a success style (dark green, bold) - Bootstrap's success color scheme
pub fn success_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::DarkGreen),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

/// Creates a warning style (dark yellow, bold) - Bootstrap's warning color scheme
pub fn warning_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::DarkYellow),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

/// Creates a danger style (dark red, bold) - Bootstrap's danger color scheme
pub fn danger_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::DarkRed),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

/// Creates a light style (white) - for light text on dark backgrounds
pub fn light_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::White),
        background_color: None,
        underline_color: None,
        attributes: Attributes::default(),
    }
}

/// Creates a dark style (black, bold) - for dark text on light backgrounds
pub fn dark_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::Black),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

/// Creates a bold style (no color, just bold attribute)
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
    /// Returns the ContentStyle for this variant
    pub fn style(&self) -> ContentStyle {
        match self {
            Variant::Primary => primary_style(),
            Variant::Secondary => secondary_style(),
            Variant::Success => success_style(),
            Variant::Danger => danger_style(),
            Variant::Warning => warning_style(),
            Variant::Info => info_style(),
            Variant::Light => light_style(),
            Variant::Dark => dark_style(),
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

fn hyperlinked_span(style: ContentStyle, text: String, url: String) -> Span {
    let span = Span::new_styled_lossy(style::StyledContent::new(style, text));
    span.with_hyperlink(Some(Hyperlink::new(url)))
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
            variant: Variant::Default,
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

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Render header as multiple lines (includes blank line before, underline for h1/h2, blank line after)
    fn render_impl(&self) -> Vec<Line> {
        let style = self.variant.style().attribute(Attribute::Bold);
        let mut lines = Vec::new();

        // Blank line before
        lines.push(Line::default());

        // Main header line
        let mut header_line = Line::default();
        header_line.push(styled_span(style, self.text.clone()));
        lines.push(header_line);

        // Underline for H1 and H2
        let underline_chars = get_header_underline_chars();
        match self.level {
            HeaderLevel::H1 => {
                let mut underline = Line::default();
                underline.push(styled_span(
                    style,
                    underline_chars.h1.repeat(self.text.len()),
                ));
                lines.push(underline);
            }
            HeaderLevel::H2 => {
                let mut underline = Line::default();
                underline.push(styled_span(
                    style,
                    underline_chars.h2.repeat(self.text.len()),
                ));
                lines.push(underline);
            }
            _ => {}
        }

        lines
    }

    pub fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

impl Component for Header {
    fn render(&self) -> Vec<Line> {
        self.render_impl()
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
        let divider_chars = get_divider_chars();
        match self.style {
            DividerStyle::Solid => divider_chars.solid,
            DividerStyle::Dashed => divider_chars.dashed,
            DividerStyle::Dotted => divider_chars.dotted,
            DividerStyle::Double => divider_chars.double,
        }
    }

    fn render_line(&self) -> Line {
        let width = self.width.unwrap_or(80);
        let line_str = self.get_char().repeat(width);
        let style = self.variant.style();

        let mut line = Line::default();
        line.push(styled_span(style, line_str));
        line
    }

    pub fn render(&self) -> Line {
        self.render_line()
    }
}

impl Component for Divider {
    fn render(&self) -> Vec<Line> {
        vec![self.render_line()]
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

    fn render_line(&self) -> Line {
        let style = self.variant.style();
        let mut line = Line::default();
        line.push(styled_span(style, self.text.clone()));
        line
    }

    pub fn render(&self) -> Line {
        self.render_line()
    }
}

impl Component for Paragraph {
    fn render(&self) -> Vec<Line> {
        vec![self.render_line()]
    }
}

// ---------------------------------------------------------------------------
// Text Alignment Utilities
// ---------------------------------------------------------------------------

/// Aligns text within a specified width
pub struct AlignedText {
    text: String,
    width: usize,
    align: Align,
    variant: Variant,
}

impl AlignedText {
    /// Create a new aligned text with specified width and alignment
    pub fn new(text: impl Into<String>, width: usize, align: Align) -> Self {
        Self {
            text: text.into(),
            width,
            align,
            variant: Variant::Default,
        }
    }

    /// Set the variant/style
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Render the aligned text
    fn render_line(&self) -> Line {
        let style = self.variant.style();
        let text_len = self.text.chars().count();

        let aligned = if text_len >= self.width {
            // Text is too long, just return it as is
            self.text.clone()
        } else {
            let padding = self.width - text_len;
            match self.align {
                Align::Left => format!("{}{}", self.text, " ".repeat(padding)),
                Align::Right => format!("{}{}", " ".repeat(padding), self.text),
                Align::Center => {
                    let left_padding = padding / 2;
                    let right_padding = padding - left_padding;
                    format!(
                        "{}{}{}",
                        " ".repeat(left_padding),
                        self.text,
                        " ".repeat(right_padding)
                    )
                }
            }
        };

        let mut line = Line::default();
        line.push(styled_span(style, aligned));
        line
    }

    pub fn render(&self) -> Line {
        self.render_line()
    }
}

impl Component for AlignedText {
    fn render(&self) -> Vec<Line> {
        vec![self.render_line()]
    }
}

/// Left-align text within the specified width
pub fn align_left(text: impl Into<String>, width: usize) -> AlignedText {
    AlignedText::new(text, width, Align::Left)
}

/// Center-align text within the specified width
pub fn align_center(text: impl Into<String>, width: usize) -> AlignedText {
    AlignedText::new(text, width, Align::Center)
}

/// Right-align text within the specified width
pub fn align_right(text: impl Into<String>, width: usize) -> AlignedText {
    AlignedText::new(text, width, Align::Right)
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

/// Represents a list item that can be either text or a nested list
pub enum ListItem {
    /// Simple text item
    Text(String),
    /// Nested list (for creating hierarchical lists)
    Nested(List),
}

impl From<String> for ListItem {
    fn from(s: String) -> Self {
        ListItem::Text(s)
    }
}

impl From<&str> for ListItem {
    fn from(s: &str) -> Self {
        ListItem::Text(s.to_string())
    }
}

impl From<List> for ListItem {
    fn from(list: List) -> Self {
        ListItem::Nested(list)
    }
}

/// List component for displaying items
pub struct List {
    items: Vec<ListItem>,
    style: ListStyle,
    variant: Variant,
    indent_level: usize,
}

impl List {
    pub fn new(style: ListStyle) -> Self {
        Self {
            items: Vec::new(),
            style,
            variant: Variant::Default,
            indent_level: 0,
        }
    }

    pub fn unordered() -> Self {
        Self::new(ListStyle::Unordered)
    }

    pub fn ordered() -> Self {
        Self::new(ListStyle::Ordered)
    }

    pub fn item(mut self, item: impl Into<ListItem>) -> Self {
        self.items.push(item.into());
        self
    }

    pub fn items(mut self, items: Vec<ListItem>) -> Self {
        self.items = items;
        self
    }

    /// Add a nested list as a list item
    pub fn nested(mut self, nested_list: List) -> Self {
        // Inherit indent level from parent
        let mut nested = nested_list;
        nested.indent_level = self.indent_level + 1;
        nested.variant = self.variant;
        self.items.push(ListItem::Nested(nested));
        self
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    fn render_impl(&self) -> Vec<Line> {
        let style = self.variant.style();
        let mut lines = Vec::new();
        let base_indent = "  ".repeat(self.indent_level);

        let mut text_item_index = 0;

        for item in &self.items {
            match item {
                ListItem::Text(text) => {
                    text_item_index += 1;
                    let mut line = Line::default();

                    let prefix = match self.style {
                        ListStyle::Unordered => {
                            let bullet = match typography_mode() {
                                TypographyMode::Ascii => "-",
                                TypographyMode::Unicode | TypographyMode::NerdFonts => "•",
                            };
                            format!("{}{} ", base_indent, bullet)
                        }
                        ListStyle::Ordered => format!("{}  {}. ", base_indent, text_item_index),
                    };

                    line.push(unstyled_span(prefix));
                    line.push(styled_span(style, text.clone()));
                    lines.push(line);
                }
                ListItem::Nested(nested_list) => {
                    // Render nested list with increased indent
                    let nested_lines = nested_list.render();
                    lines.extend(nested_lines);
                }
            }
        }

        lines
    }

    pub fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

impl Component for List {
    fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

// ---------------------------------------------------------------------------
// Link component
// ---------------------------------------------------------------------------

/// Link component for displaying URLs or references.
///
/// When a URL is provided via `.url()`, this component uses OSC 8 escape sequences
/// to create clickable hyperlinks in compatible terminals (e.g., iTerm2, WezTerm,
/// Windows Terminal, GNOME Terminal).
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::Link;
///
/// // Create a clickable link
/// let link = Link::new("Visit our docs")
///     .url("https://example.com/docs");
/// let line = link.render();
///
/// // Or just styled text without a URL
/// let text = Link::new("See above");
/// let line = text.render();
/// ```
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

    fn render_line(&self) -> Line {
        let mut style = self.variant.style();
        style.attributes = style.attributes.with(Attribute::Underlined);

        let mut line = Line::default();

        // If a URL is provided, use OSC 8 hyperlink support for clickable links
        if let Some(url) = &self.url {
            line.push(hyperlinked_span(style, self.text.clone(), url.clone()));
        } else {
            // No URL provided, just render styled text
            line.push(styled_span(style, self.text.clone()));
        }

        line
    }

    pub fn render(&self) -> Line {
        self.render_line()
    }
}

impl Component for Link {
    fn render(&self) -> Vec<Line> {
        vec![self.render_line()]
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

    fn render_impl(&self) -> Vec<Line> {
        let style = self.variant.style();
        let mut lines = Vec::new();
        let prefix = get_blockquote_prefix();

        for text_line in self.text.lines() {
            let mut line = Line::default();
            line.push(unstyled_span(prefix.to_string()));
            line.push(styled_span(style, text_line.to_string()));
            lines.push(line);
        }

        lines
    }

    pub fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

impl Component for Blockquote {
    fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

// ---------------------------------------------------------------------------
// Description List component
// ---------------------------------------------------------------------------

/// Converts a description value into one or more renderable lines.
pub trait IntoLines {
    fn into_description_lines(self) -> Vec<Line>;
}

fn plain_description_lines(text: &str) -> Vec<Line> {
    text.lines()
        .map(|text_line| {
            let mut line = Line::default();
            line.push(unstyled_span(text_line.to_string()));
            line
        })
        .collect()
}

impl IntoLines for String {
    fn into_description_lines(self) -> Vec<Line> {
        plain_description_lines(&self)
    }
}

impl IntoLines for &str {
    fn into_description_lines(self) -> Vec<Line> {
        plain_description_lines(self)
    }
}

impl IntoLines for &String {
    fn into_description_lines(self) -> Vec<Line> {
        plain_description_lines(self)
    }
}

impl IntoLines for std::borrow::Cow<'_, str> {
    fn into_description_lines(self) -> Vec<Line> {
        plain_description_lines(&self)
    }
}

impl IntoLines for Line {
    fn into_description_lines(self) -> Vec<Line> {
        vec![self]
    }
}

impl IntoLines for &Line {
    fn into_description_lines(self) -> Vec<Line> {
        vec![self.clone()]
    }
}

impl IntoLines for Vec<Line> {
    fn into_description_lines(self) -> Vec<Line> {
        self
    }
}

impl IntoLines for &[Line] {
    fn into_description_lines(self) -> Vec<Line> {
        self.to_vec()
    }
}

impl IntoLines for Span {
    fn into_description_lines(self) -> Vec<Line> {
        let mut line = Line::default();
        line.push(self);
        vec![line]
    }
}

impl IntoLines for Vec<Span> {
    fn into_description_lines(self) -> Vec<Line> {
        let mut line = Line::default();
        line.extend(self);
        vec![line]
    }
}

impl<T> IntoLines for StyledContent<T>
where
    T: std::fmt::Display,
{
    fn into_description_lines(self) -> Vec<Line> {
        let style = *self.style();
        self.content()
            .to_string()
            .lines()
            .map(|text_line| {
                let mut line = Line::default();
                line.push(Span::new_styled_lossy(style::StyledContent::new(
                    style,
                    text_line.to_string(),
                )));
                line
            })
            .collect()
    }
}

/// A term-description pair for use in description lists.
#[derive(Debug, Clone)]
pub struct DescriptionItem {
    term: String,
    description: Vec<Line>,
}

impl DescriptionItem {
    pub fn new(term: impl Into<String>, description: impl IntoLines) -> Self {
        Self {
            term: term.into(),
            description: description.into_description_lines(),
        }
    }
}

/// Description list for displaying term-description pairs (common in CLI tools).
/// Similar to HTML's `<dl>`, `<dt>`, `<dd>` structure.
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::DescriptionList;
///
/// let dl = DescriptionList::new()
///     .item("Name", "Zed")
///     .item("Version", "0.1.0")
///     .item("License", "MIT");
/// let lines = dl.render();
/// ```
pub struct DescriptionList {
    items: Vec<DescriptionItem>,
    variant: Variant,
    compact: bool,
}

impl DescriptionList {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            variant: Variant::Default,
            compact: false,
        }
    }

    /// Add a term-description pair to the list.
    pub fn item(mut self, term: impl Into<String>, description: impl IntoLines) -> Self {
        self.items.push(DescriptionItem::new(term, description));
        self
    }

    /// Add multiple term-description pairs at once.
    pub fn items<T, D>(mut self, items: impl IntoIterator<Item = (T, D)>) -> Self
    where
        T: Into<String>,
        D: IntoLines,
    {
        for (term, desc) in items {
            self.items.push(DescriptionItem::new(term, desc));
        }
        self
    }

    /// Set the color variant for the terms.
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Enable compact mode (no blank lines between items).
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    fn render_impl(&self) -> Vec<Line> {
        let style = self.variant.style();
        let mut lines = Vec::new();

        // Calculate the maximum term width
        let max_term_width = self
            .items
            .iter()
            .map(|item| item.term.len())
            .max()
            .unwrap_or(0);

        for (idx, item) in self.items.iter().enumerate() {
            let term_width = item.term.len();

            // First line: 2 spaces, term (left-justified), padding, then description
            let mut first_line = Line::default();
            first_line.push(unstyled_span("  ".to_string()));
            first_line.push(styled_span(style.bold(), item.term.clone()));

            // Padding to align descriptions: from end of term to description start
            let padding = max_term_width - term_width + 2;

            if let Some(first_desc_line) = item.description.first() {
                first_line.push(unstyled_span(" ".repeat(padding)));
                first_line.extend(first_desc_line.iter().cloned());
            }
            lines.push(first_line);

            // Subsequent description lines (aligned with first description line)
            for desc_line in item.description.iter().skip(1) {
                let mut line = Line::default();
                // Indent to align with description: 2 spaces + max_term_width + padding
                let indent = 2 + max_term_width + 2;
                line.push(unstyled_span(" ".repeat(indent)));
                line.extend(desc_line.iter().cloned());
                lines.push(line);
            }

            // Add spacing between items (unless compact mode or last item)
            if !self.compact && idx < self.items.len() - 1 {
                lines.push(Line::default());
            }
        }

        lines
    }

    pub fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

impl Component for DescriptionList {
    fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

impl Default for DescriptionList {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Inline Text Styling
// ---------------------------------------------------------------------------

/// Inline code text (monospace).
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::code;
///
/// let span = code("npm install");
/// ```
pub fn code(text: impl Into<String>) -> Span {
    let style = ContentStyle {
        foreground_color: Some(Color::Magenta),
        background_color: Some(Color::Black),
        attributes: Attributes::default(),
        underline_color: None,
    };
    styled_span(style, text.into())
}

/// Highlighted/marked text.
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::mark;
///
/// let span = mark("important");
/// ```
pub fn mark(text: impl Into<String>) -> Span {
    let style = ContentStyle {
        foreground_color: Some(Color::Black),
        background_color: Some(Color::Yellow),
        attributes: Attributes::default(),
        underline_color: None,
    };
    styled_span(style, text.into())
}

/// Small/fine print text.
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::small;
///
/// let span = small("© 2024");
/// ```
pub fn small(text: impl Into<String>) -> Span {
    let style = ContentStyle {
        foreground_color: Some(Color::DarkGrey),
        background_color: None,
        attributes: Attributes::default(),
        underline_color: None,
    };
    styled_span(style, text.into())
}

/// Deleted/strikethrough text.
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::del;
///
/// let span = del("obsolete");
/// ```
pub fn del(text: impl Into<String>) -> Span {
    let style = ContentStyle {
        foreground_color: Some(Color::DarkGrey),
        background_color: None,
        attributes: Attributes::from(Attribute::CrossedOut),
        underline_color: None,
    };
    styled_span(style, text.into())
}

/// Inserted/underlined text.
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::ins;
///
/// let span = ins("new feature");
/// ```
pub fn ins(text: impl Into<String>) -> Span {
    let style = ContentStyle {
        foreground_color: Some(Color::Green),
        background_color: None,
        attributes: Attributes::from(Attribute::Underlined),
        underline_color: None,
    };
    styled_span(style, text.into())
}

/// Subscript text (using Unicode subscript characters where possible).
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::sub_text;
///
/// let span = sub_text("2"); // For H₂O
/// ```
pub fn sub_text(text: impl Into<String>) -> Span {
    let text_str = text.into();
    let subscript = convert_to_subscript(&text_str);
    let style = ContentStyle {
        foreground_color: None,
        background_color: None,
        attributes: Attributes::default(),
        underline_color: None,
    };
    styled_span(style, subscript)
}

/// Superscript text (using Unicode superscript characters where possible).
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::sup_text;
///
/// let span = sup_text("2"); // For E=mc²
/// ```
pub fn sup_text(text: impl Into<String>) -> Span {
    let text_str = text.into();
    let superscript = convert_to_superscript(&text_str);
    let style = ContentStyle {
        foreground_color: None,
        background_color: None,
        attributes: Attributes::default(),
        underline_color: None,
    };
    styled_span(style, superscript)
}

/// Keyboard input text.
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::kbd;
///
/// let span = kbd("Ctrl+C");
/// ```
pub fn kbd(text: impl Into<String>) -> Span {
    let style = ContentStyle {
        foreground_color: Some(Color::White),
        background_color: Some(Color::DarkGrey),
        attributes: Attributes::from(Attribute::Bold),
        underline_color: None,
    };
    styled_span(style, text.into())
}

/// Variable name text.
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::var;
///
/// let span = var("user_name");
/// ```
pub fn var(text: impl Into<String>) -> Span {
    let style = ContentStyle {
        foreground_color: Some(Color::Cyan),
        background_color: None,
        attributes: Attributes::from(Attribute::Italic),
        underline_color: None,
    };
    styled_span(style, text.into())
}

/// Sample output text.
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::samp;
///
/// let span = samp("Command output");
/// ```
pub fn samp(text: impl Into<String>) -> Span {
    let style = ContentStyle {
        foreground_color: Some(Color::Green),
        background_color: Some(Color::Black),
        attributes: Attributes::default(),
        underline_color: None,
    };
    styled_span(style, text.into())
}

// Helper functions for subscript/superscript conversion
fn convert_to_subscript(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '0' => '₀',
            '1' => '₁',
            '2' => '₂',
            '3' => '₃',
            '4' => '₄',
            '5' => '₅',
            '6' => '₆',
            '7' => '₇',
            '8' => '₈',
            '9' => '₉',
            '+' => '₊',
            '-' => '₋',
            '=' => '₌',
            '(' => '₍',
            ')' => '₎',
            'a' => 'ₐ',
            'e' => 'ₑ',
            'o' => 'ₒ',
            'x' => 'ₓ',
            'h' => 'ₕ',
            'k' => 'ₖ',
            'l' => 'ₗ',
            'm' => 'ₘ',
            'n' => 'ₙ',
            'p' => 'ₚ',
            's' => 'ₛ',
            't' => 'ₜ',
            _ => c,
        })
        .collect()
}

fn convert_to_superscript(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            '+' => '⁺',
            '-' => '⁻',
            '=' => '⁼',
            '(' => '⁽',
            ')' => '⁾',
            'i' => 'ⁱ',
            'n' => 'ⁿ',
            _ => c,
        })
        .collect()
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
    footers: Vec<String>,
    alignments: Vec<Align>,
    variant: Variant,
    width: Option<usize>,
}

impl Table {
    pub fn new() -> Self {
        Self {
            headers: Vec::new(),
            rows: Vec::new(),
            footers: Vec::new(),
            alignments: Vec::new(),
            variant: Variant::Default,
            width: None,
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

    pub fn footers(mut self, footers: Vec<String>) -> Self {
        self.footers = footers;
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

    fn calculate_column_widths(&self) -> Vec<usize> {
        let mut widths = self.headers.iter().map(|h| h.len()).collect::<Vec<_>>();

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        if !self.footers.is_empty() {
            for (i, cell) in self.footers.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Apply width constraint if specified
        if let Some(target_width) = self.width {
            let num_cols = widths.len();
            if num_cols > 0 {
                // Calculate current total width including borders and padding
                // Format: | col1 | col2 | col3 |
                // Each column has: 1 space + content + 1 space
                // Plus: 1 border at start, 1 border between each column, 1 border at end
                let borders_and_padding = 1 + num_cols * 3; // 1 leading + (space + content + space + |) per column
                let available_content_width = target_width.saturating_sub(borders_and_padding);
                let current_content_width: usize = widths.iter().sum();

                if current_content_width != available_content_width {
                    if current_content_width > available_content_width {
                        // Need to shrink columns proportionally
                        let scale = available_content_width as f64 / current_content_width as f64;
                        let mut new_widths: Vec<usize> = widths
                            .iter()
                            .map(|w| ((*w as f64 * scale).floor() as usize).max(1))
                            .collect();

                        // Distribute any remaining width to ensure we use the full available space
                        let new_total: usize = new_widths.iter().sum();
                        let diff = available_content_width.saturating_sub(new_total);
                        if diff > 0 {
                            // Add extra width to the last column
                            if let Some(last) = new_widths.last_mut() {
                                *last += diff;
                            }
                        }

                        widths = new_widths;
                    } else {
                        // Need to expand columns to fill the target width
                        let extra_width = available_content_width - current_content_width;
                        let per_col = extra_width / num_cols;
                        let remainder = extra_width % num_cols;

                        // Distribute extra width evenly
                        for width in widths.iter_mut() {
                            *width += per_col;
                        }

                        // Add remainder to the last columns
                        for i in 0..remainder {
                            if let Some(w) = widths.get_mut(num_cols - 1 - i) {
                                *w += 1;
                            }
                        }
                    }
                }
            }
        }

        widths
    }

    fn format_cell(&self, content: &str, width: usize, align: Align) -> String {
        let ellipsis = match typography_mode() {
            TypographyMode::Ascii => "...",
            _ => "…",
        };
        let ellipsis_len = ellipsis.chars().count();

        // Truncate if content is too long
        let truncated = if content.chars().count() > width {
            if width > ellipsis_len {
                let take = width - ellipsis_len;
                let truncated_part: String = content.chars().take(take).collect();
                format!("{}{}", truncated_part, ellipsis)
            } else {
                // If width is too small for ellipsis, just truncate
                content.chars().take(width).collect()
            }
        } else {
            content.to_string()
        };

        match align {
            Align::Left => format!("{:<width$}", truncated, width = width),
            Align::Center => {
                let content_len = truncated.chars().count();
                let total_padding = width.saturating_sub(content_len);
                let left_padding = total_padding / 2;
                let right_padding = total_padding - left_padding;
                format!(
                    "{}{}{}",
                    " ".repeat(left_padding),
                    truncated,
                    " ".repeat(right_padding)
                )
            }
            Align::Right => format!("{:>width$}", truncated, width = width),
        }
    }

    fn render_impl(&self) -> Vec<Line> {
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

        let box_chars = get_box_chars();
        let mut lines = Vec::new();

        // Top border
        let mut top_line = Line::default();
        let mut top_str = String::from(box_chars.top_left);
        for (i, width) in widths.iter().enumerate() {
            top_str.push_str(&box_chars.horizontal.repeat(width + 2));
            if i < widths.len() - 1 {
                top_str.push_str(box_chars.top_t);
            }
        }
        top_str.push_str(box_chars.top_right);
        top_line.push(unstyled_span(top_str));
        lines.push(top_line);

        // Headers
        let mut header_line = Line::default();
        header_line.push(unstyled_span(box_chars.vertical.to_string()));
        for (i, (header, width)) in self.headers.iter().zip(widths.iter()).enumerate() {
            let align = self.alignments.get(i).copied().unwrap_or(Align::Left);
            let formatted = self.format_cell(header, *width, align);
            header_line.push(unstyled_span(" ".to_string()));
            header_line.push(styled_span(header_style, formatted));
            header_line.push(unstyled_span(format!(" {}", box_chars.vertical)));
        }
        lines.push(header_line);

        // Header separator
        let mut sep_line = Line::default();
        let mut sep_str = String::from(box_chars.left_t);
        for (i, width) in widths.iter().enumerate() {
            sep_str.push_str(&box_chars.horizontal.repeat(width + 2));
            if i < widths.len() - 1 {
                sep_str.push_str(box_chars.cross);
            }
        }
        sep_str.push_str(box_chars.right_t);
        sep_line.push(unstyled_span(sep_str));
        lines.push(sep_line);

        // Rows
        for row in &self.rows {
            let mut row_line = Line::default();
            row_line.push(unstyled_span(box_chars.vertical.to_string()));
            for (i, width) in widths.iter().enumerate() {
                let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
                let align = self.alignments.get(i).copied().unwrap_or(Align::Left);
                let formatted = self.format_cell(cell, *width, align);
                row_line.push(unstyled_span(" ".to_string()));
                row_line.push(styled_span(style, formatted));
                row_line.push(unstyled_span(format!(" {}", box_chars.vertical)));
            }
            lines.push(row_line);
        }

        // Footer (if present)
        if !self.footers.is_empty() {
            // Footer separator
            let mut footer_sep_line = Line::default();
            let mut footer_sep_str = String::from(box_chars.left_t);
            for (i, width) in widths.iter().enumerate() {
                footer_sep_str.push_str(&box_chars.horizontal.repeat(width + 2));
                if i < widths.len() - 1 {
                    footer_sep_str.push_str(box_chars.cross);
                }
            }
            footer_sep_str.push_str(box_chars.right_t);
            footer_sep_line.push(unstyled_span(footer_sep_str));
            lines.push(footer_sep_line);

            // Footer row
            let mut footer_line = Line::default();
            footer_line.push(unstyled_span(box_chars.vertical.to_string()));
            for (i, width) in widths.iter().enumerate() {
                let cell = self.footers.get(i).map(|s| s.as_str()).unwrap_or("");
                let align = self.alignments.get(i).copied().unwrap_or(Align::Left);
                let formatted = self.format_cell(cell, *width, align);
                footer_line.push(unstyled_span(" ".to_string()));
                footer_line.push(styled_span(header_style, formatted));
                footer_line.push(unstyled_span(format!(" {}", box_chars.vertical)));
            }
            lines.push(footer_line);
        }

        // Bottom border
        let mut bottom_line = Line::default();
        let mut bottom_str = String::from(box_chars.bottom_left);
        for (i, width) in widths.iter().enumerate() {
            bottom_str.push_str(&box_chars.horizontal.repeat(width + 2));
            if i < widths.len() - 1 {
                bottom_str.push_str(box_chars.bottom_t);
            }
        }
        bottom_str.push_str(box_chars.bottom_right);
        bottom_line.push(unstyled_span(bottom_str));
        lines.push(bottom_line);

        lines
    }

    pub fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

impl Component for Table {
    fn render(&self) -> Vec<Line> {
        self.render_impl()
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

    fn render_impl(&self) -> Vec<Line> {
        // Clamp width to minimum of 4 to prevent underflow in border/padding calculations
        let width = self.width.unwrap_or(60).max(4);
        let style = self.variant.style();
        let title_style = ContentStyle {
            foreground_color: style.foreground_color,
            background_color: style.background_color,
            underline_color: style.underline_color,
            attributes: style.attributes.with(Attribute::Bold),
        };

        let box_chars = get_box_chars();
        let mut lines = Vec::new();

        // Top border
        let mut top_line = Line::default();
        top_line.push(unstyled_span(format!(
            "{}{}{}",
            box_chars.top_left,
            box_chars.horizontal.repeat(width - 2),
            box_chars.top_right
        )));
        lines.push(top_line);

        // Title
        if let Some(title) = &self.title {
            let mut title_line = Line::default();
            title_line.push(unstyled_span(format!("{} ", box_chars.vertical)));
            title_line.push(styled_span(
                title_style,
                format!("{:<width$}", title, width = width - 4),
            ));
            title_line.push(unstyled_span(format!(" {}", box_chars.vertical)));
            lines.push(title_line);

            let mut sep_line = Line::default();
            sep_line.push(unstyled_span(format!(
                "{}{}{}",
                box_chars.left_t,
                box_chars.horizontal.repeat(width - 2),
                box_chars.right_t
            )));
            lines.push(sep_line);
        }

        // Body
        for body_line in self.body.lines() {
            let mut line = Line::default();
            line.push(unstyled_span(format!("{} ", box_chars.vertical)));
            line.push(styled_span(
                style,
                format!("{:<width$}", body_line, width = width - 4),
            ));
            line.push(unstyled_span(format!(" {}", box_chars.vertical)));
            lines.push(line);
        }

        // Bottom border
        let mut bottom_line = Line::default();
        bottom_line.push(unstyled_span(format!(
            "{}{}{}",
            box_chars.bottom_left,
            box_chars.horizontal.repeat(width - 2),
            box_chars.bottom_right
        )));
        lines.push(bottom_line);

        lines
    }

    pub fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

impl Component for Card {
    fn render(&self) -> Vec<Line> {
        self.render_impl()
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

    fn render_impl(&self) -> Vec<Line> {
        let mut lines = Vec::new();

        // Print title with variant style
        let title_style = self.variant.style().bold();
        let mut title_line = Line::default();
        title_line.push(styled_span(title_style, self.title.clone()));
        lines.push(title_line);

        if self.bars.is_empty() {
            return lines;
        }

        // Find the maximum value to normalize bar lengths
        let max_value = self.bars.iter().map(|b| b.value).max().unwrap_or(1);

        // Find the longest label for alignment
        let max_label_len = self.bars.iter().map(|b| b.label.len()).max().unwrap_or(0);

        let bar_char = get_histogram_bar_char();

        // Render each bar
        for bar in &self.bars {
            let bar_len = (bar.value * self.bar_width)
                .checked_div(max_value)
                .unwrap_or(0);
            let bar_str = bar_char.to_string().repeat(bar_len);

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

    pub fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

impl Component for Histogram {
    fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

/// An alert/callout component for displaying notices, warnings, and important messages.
///
/// Alerts provide prominent visual feedback with colored borders and titles based on their variant.
/// The border and title use the variant color, while the body text remains in the default style.
///
/// # Examples
///
/// ```rust
/// use spaces_console::typography::{Alert, Variant};
///
/// // Create an info alert
/// let alert = Alert::new("Server restarted successfully")
///     .title("Information")
///     .variant(Variant::Info);
///
/// // Create a warning alert
/// let alert = Alert::new("This action cannot be undone")
///     .title("Warning")
///     .variant(Variant::Warning);
///
/// // Create a danger alert with multi-line content
/// let alert = Alert::new("Connection failed\nPlease check your network settings")
///     .title("Error")
///     .variant(Variant::Danger);
/// ```
pub struct Alert {
    title: Option<String>,
    body: String,
    variant: Variant,
    width: Option<usize>,
}

impl Alert {
    /// Creates a new Alert with the given body text.
    ///
    /// Defaults to the Info variant.
    pub fn new(body: impl Into<String>) -> Self {
        Self {
            title: None,
            body: body.into(),
            variant: Variant::Info,
            width: None,
        }
    }

    /// Sets the title for the alert.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the variant (color scheme) for the alert.
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the width of the alert box.
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Renders the alert as a vector of Lines.
    fn render_impl(&self) -> Vec<Line> {
        // Clamp width to minimum of 4 to prevent underflow in border/padding calculations
        let width = self.width.unwrap_or(60).max(4);
        let border_style = self.variant.style();
        let title_style = ContentStyle {
            foreground_color: border_style.foreground_color,
            background_color: border_style.background_color,
            underline_color: border_style.underline_color,
            attributes: border_style.attributes.with(Attribute::Bold),
        };
        let body_style = default_style();

        let box_chars = get_box_chars();
        let mut lines = Vec::new();

        // Top border (colored)
        let mut top_line = Line::default();
        top_line.push(styled_span(
            border_style,
            format!(
                "{}{}{}",
                box_chars.top_left,
                box_chars.horizontal.repeat(width - 2),
                box_chars.top_right
            ),
        ));
        lines.push(top_line);

        // Title (colored border + colored title text)
        if let Some(title) = &self.title {
            let mut title_line = Line::default();
            title_line.push(styled_span(
                border_style,
                format!("{} ", box_chars.vertical),
            ));
            title_line.push(styled_span(
                title_style,
                format!("{:<width$}", title, width = width - 4),
            ));
            title_line.push(styled_span(
                border_style,
                format!(" {}", box_chars.vertical),
            ));
            lines.push(title_line);

            // Separator (colored)
            let mut sep_line = Line::default();
            sep_line.push(styled_span(
                border_style,
                format!(
                    "{}{}{}",
                    box_chars.left_t,
                    box_chars.horizontal.repeat(width - 2),
                    box_chars.right_t
                ),
            ));
            lines.push(sep_line);
        }

        // Body (colored border + default text)
        for body_line in self.body.lines() {
            let mut line = Line::default();
            line.push(styled_span(
                border_style,
                format!("{} ", box_chars.vertical),
            ));
            line.push(styled_span(
                body_style,
                format!("{:<width$}", body_line, width = width - 4),
            ));
            line.push(styled_span(
                border_style,
                format!(" {}", box_chars.vertical),
            ));
            lines.push(line);
        }

        // Bottom border (colored)
        let mut bottom_line = Line::default();
        bottom_line.push(styled_span(
            border_style,
            format!(
                "{}{}{}",
                box_chars.bottom_left,
                box_chars.horizontal.repeat(width - 2),
                box_chars.bottom_right
            ),
        ));
        lines.push(bottom_line);

        lines
    }

    pub fn render(&self) -> Vec<Line> {
        self.render_impl()
    }
}

impl Component for Alert {
    fn render(&self) -> Vec<Line> {
        self.render_impl()
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

pub fn alert(body: impl Into<String>) -> Vec<Line> {
    Alert::new(body).render()
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
    fn test_nested_list_unordered() {
        let nested = List::unordered().item("Nested 1").item("Nested 2");
        let list = List::unordered().item("First").nested(nested).item("Third");
        let lines = list.render();
        // 1 for "First", 2 for nested items, 1 for "Third" = 4 total
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_nested_list_ordered() {
        let nested = List::ordered().item("Nested A").item("Nested B");
        let list = List::ordered().item("First").nested(nested).item("Second");
        let lines = list.render();
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_nested_list_multiple_levels() {
        let level2 = List::unordered().item("Level 2A").item("Level 2B");
        let level1 = List::unordered().item("Level 1A").nested(level2);
        let list = List::unordered().item("Root").nested(level1).item("End");
        let lines = list.render();
        // Root + Level 1A + Level 2A + Level 2B + End = 5
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn test_align_left() {
        let aligned = align_left("Hello", 10);
        let line = aligned.render();
        let spans: Vec<_> = line.iter().collect();
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_align_center() {
        let aligned = align_center("Hi", 10);
        let line = aligned.render();
        let spans: Vec<_> = line.iter().collect();
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_align_right() {
        let aligned = align_right("Test", 10);
        let line = aligned.render();
        let spans: Vec<_> = line.iter().collect();
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_aligned_text_with_variant() {
        let aligned = AlignedText::new("Success", 20, Align::Center).variant(Variant::Success);
        let line = aligned.render();
        let spans: Vec<_> = line.iter().collect();
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_aligned_text_too_long() {
        // Text longer than width should not be truncated
        let aligned = align_left("This is a very long text", 10);
        let line = aligned.render();
        let spans: Vec<_> = line.iter().collect();
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_link_without_url() {
        let link = Link::new("Click here");
        let line = link.render();
        let spans: Vec<_> = line.iter().collect();
        assert_eq!(spans.len(), 1);
        let span = spans[0];
        assert_eq!(span.content(), "Click here");
        assert!(span.hyperlink.is_none());
    }

    #[test]
    fn test_link_with_url() {
        let link = Link::new("Visit docs").url("https://example.com/docs");
        let line = link.render();
        let spans: Vec<_> = line.iter().collect();
        assert_eq!(spans.len(), 1);
        let span = spans[0];
        assert_eq!(span.content(), "Visit docs");
        assert!(span.hyperlink.is_some());
        if let Some(ref hyperlink) = span.hyperlink {
            assert_eq!(hyperlink.uri(), "https://example.com/docs");
        }
    }

    #[test]
    fn test_link_helper_function() {
        let line = link_with_url("GitHub", "https://github.com");
        let spans: Vec<_> = line.iter().collect();
        assert_eq!(spans.len(), 1);
        let span = spans[0];
        assert_eq!(span.content(), "GitHub");
        assert!(span.hyperlink.is_some());
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
    fn test_table_with_width_padding() {
        // Test that when width is larger than content, columns are padded evenly
        let table = Table::new()
            .headers(vec!["A".to_string(), "B".to_string()])
            .row(vec!["1".to_string(), "2".to_string()])
            .width(40);
        let lines = table.render();
        assert!(!lines.is_empty());
        // First line should be the top border and should be exactly 40 chars
        let first_line_text = lines[0]
            .iter()
            .map(|span| span.content())
            .collect::<String>();
        assert_eq!(first_line_text.chars().count(), 40);
    }

    #[test]
    fn test_table_with_width_truncation() {
        // Test that when width is smaller than content, text is truncated with ellipsis
        let table = Table::new()
            .headers(vec![
                "Long Header Name".to_string(),
                "Another Long Header".to_string(),
            ])
            .row(vec![
                "Very long content".to_string(),
                "More long content".to_string(),
            ])
            .width(30);
        let lines = table.render();
        assert!(!lines.is_empty());
        // First line should be exactly 30 chars
        let first_line_text = lines[0]
            .iter()
            .map(|span| span.content())
            .collect::<String>();
        assert_eq!(first_line_text.chars().count(), 30);
    }

    #[test]
    fn test_table_width_matches_exactly() {
        // Test that all lines have the exact specified width
        let table = Table::new()
            .headers(vec![
                "Name".to_string(),
                "Age".to_string(),
                "City".to_string(),
            ])
            .row(vec![
                "Alice".to_string(),
                "30".to_string(),
                "NYC".to_string(),
            ])
            .row(vec!["Bob".to_string(), "25".to_string(), "LA".to_string()])
            .width(50);
        let lines = table.render();

        for line in lines {
            let line_text = line.iter().map(|span| span.content()).collect::<String>();
            assert_eq!(
                line_text.chars().count(),
                50,
                "Line should be exactly 50 chars: '{}'",
                line_text
            );
        }
    }

    #[test]
    fn test_card() {
        let card = Card::new("This is the body").title("Card Title").width(40);
        let lines = card.render();
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_card_minimum_width() {
        // Test that small widths don't cause underflow panics
        let card1 = Card::new("Test").width(0);
        let lines1 = card1.render();
        assert!(!lines1.is_empty());

        let card2 = Card::new("Test").width(1);
        let lines2 = card2.render();
        assert!(!lines2.is_empty());

        let card3 = Card::new("Test").width(3);
        let lines3 = card3.render();
        assert!(!lines3.is_empty());

        let card4 = Card::new("Test").title("Title").width(4);
        let lines4 = card4.render();
        assert!(!lines4.is_empty());
    }

    #[test]
    fn test_alert() {
        let alert = Alert::new("This is an alert message")
            .title("Alert Title")
            .variant(Variant::Warning)
            .width(40);
        let lines = alert.render();
        assert!(!lines.is_empty());
        // Should have at least: top border, title, separator, body, bottom border
        assert!(lines.len() >= 5);
    }

    #[test]
    fn test_alert_no_title() {
        let alert = Alert::new("Message without title").variant(Variant::Info);
        let lines = alert.render();
        assert!(!lines.is_empty());
        // Should have: top border, body, bottom border
        assert!(lines.len() >= 3);
    }

    #[test]
    fn test_alert_multiline() {
        let alert = Alert::new("Line 1\nLine 2\nLine 3")
            .title("Multi-line Alert")
            .variant(Variant::Danger);
        let lines = alert.render();
        // Should have: top border, title, separator, 3 body lines, bottom border
        assert_eq!(lines.len(), 7);
    }

    #[test]
    fn test_alert_minimum_width() {
        // Test that small widths don't cause underflow panics
        let alert1 = Alert::new("Test").width(0);
        let lines1 = alert1.render();
        assert!(!lines1.is_empty());

        let alert2 = Alert::new("Test").width(1);
        let lines2 = alert2.render();
        assert!(!lines2.is_empty());

        let alert3 = Alert::new("Test").width(3);
        let lines3 = alert3.render();
        assert!(!lines3.is_empty());

        let alert4 = Alert::new("Test").title("Title").width(4);
        let lines4 = alert4.render();
        assert!(!lines4.is_empty());
    }

    #[test]
    fn test_alert_variants() {
        // Test that all variants work with alerts
        for variant in [
            Variant::Primary,
            Variant::Secondary,
            Variant::Success,
            Variant::Danger,
            Variant::Warning,
            Variant::Info,
            Variant::Light,
            Variant::Dark,
            Variant::Default,
        ] {
            let alert = Alert::new("Test message").variant(variant);
            let lines = alert.render();
            assert!(!lines.is_empty());
        }
    }

    #[test]
    fn test_variants() {
        assert!(primary_style().foreground_color.is_some());
        assert!(secondary_style().foreground_color.is_some());
        assert!(success_style().foreground_color.is_some());
        assert!(danger_style().foreground_color.is_some());
        assert!(warning_style().foreground_color.is_some());
        assert!(info_style().foreground_color.is_some());
        assert!(light_style().foreground_color.is_some());
        assert!(dark_style().foreground_color.is_some());
        assert!(default_style().foreground_color.is_some());
    }

    #[test]
    fn test_variant_enum_consistency() {
        // Test that Variant::style() returns the same styles as the standalone functions
        assert_eq!(
            Variant::Primary.style().foreground_color,
            primary_style().foreground_color
        );
        assert_eq!(
            Variant::Secondary.style().foreground_color,
            secondary_style().foreground_color
        );
        assert_eq!(
            Variant::Success.style().foreground_color,
            success_style().foreground_color
        );
        assert_eq!(
            Variant::Danger.style().foreground_color,
            danger_style().foreground_color
        );
        assert_eq!(
            Variant::Warning.style().foreground_color,
            warning_style().foreground_color
        );
        assert_eq!(
            Variant::Info.style().foreground_color,
            info_style().foreground_color
        );
        assert_eq!(
            Variant::Light.style().foreground_color,
            light_style().foreground_color
        );
        assert_eq!(
            Variant::Dark.style().foreground_color,
            dark_style().foreground_color
        );
        assert_eq!(
            Variant::Default.style().foreground_color,
            default_style().foreground_color
        );
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
    fn test_histogram_variant() {
        // Test that the histogram variant is applied without errors
        let histogram = Histogram::new("Test Histogram")
            .variant(Variant::Success)
            .bar(HistogramBar::new("item", 10));
        let lines = histogram.render();

        // The first line should be the title
        assert!(!lines.is_empty());
        assert!(lines.len() >= 1);
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

    #[test]
    fn test_active_progress_truncation_with_small_max_width() {
        // Test that message truncation doesn't overflow when msg_max is very small (0-4)
        // Before the fix, when msg_max was 0-4, the code would:
        // - Take msg_max.saturating_sub(4) characters (could be 0)
        // - Always append "...  " (5 characters)
        // This resulted in output longer than msg_max

        let progress = ActiveProgress::new(
            "test-task".to_string(),
            "B".to_string(), // Short prefix to allow testing various msg_max values
            "This is a very long message that needs to be truncated".to_string(),
            50,
            Some(100),
        );

        // Calculate what the fixed width would be for this progress bar
        // Format: `[H:MM:SS]|<bar>|name: message`
        // With a fresh timer, elapsed_str is around "  0:00:00" (9 chars)
        // But to be safe, let's test with widths that would result in various msg_max values

        // Test edge case widths
        let test_cases = vec![
            (0, true),   // Should not panic even with 0 width
            (1, true),   // Should not panic
            (5, true),   // Should not panic
            (10, true),  // Should not panic
            (20, true),  // Should not panic
            (50, true),  // Should work fine
            (100, true), // Should work fine
        ];

        for (max_width, should_succeed) in test_cases {
            let result = progress.render_bar(max_width);
            if should_succeed {
                assert!(
                    result.is_ok(),
                    "render_bar should succeed for max_width={}",
                    max_width
                );
            }
        }

        // The key invariant: after our fix, the message portion will never exceed msg_max
        // This prevents the overflow issue where "...  " was always appended
    }

    #[test]
    fn test_typography_mode_default() {
        // The default mode should be Unicode
        assert_eq!(typography_mode(), TypographyMode::Unicode);
    }

    #[test]
    fn test_ascii_mode_characters() {
        // This test demonstrates what the ASCII mode characters would be
        // Note: We can't actually set the mode in tests due to OnceLock,
        // but we can verify the character sets exist and are different
        assert_ne!(
            BAR_CHARS_BOUNDED_ASCII.filled,
            BAR_CHARS_BOUNDED_UNICODE.filled
        );
        assert_ne!(SPINNER_FRAMES_ASCII, SPINNER_FRAMES_UNICODE);
        assert_ne!(BOX_CHARS_ASCII.top_left, BOX_CHARS_UNICODE.top_left);
        assert_ne!(DIVIDER_CHARS_ASCII.solid, DIVIDER_CHARS_UNICODE.solid);
        assert_ne!(HISTOGRAM_BAR_ASCII, HISTOGRAM_BAR_UNICODE);
    }

    #[test]
    fn test_ascii_mode_uses_only_ascii() {
        // Verify that ASCII mode characters are all within ASCII range
        assert!(BAR_CHARS_BOUNDED_ASCII.filled.is_ascii());
        assert!(BAR_CHARS_BOUNDED_ASCII.tip.is_ascii());
        assert!(BAR_CHARS_BOUNDED_ASCII.empty.is_ascii());

        for &frame in SPINNER_FRAMES_ASCII {
            assert!(frame.is_ascii());
        }

        assert!(BOX_CHARS_ASCII.top_left.chars().all(|c| c.is_ascii()));
        assert!(BOX_CHARS_ASCII.horizontal.chars().all(|c| c.is_ascii()));
        assert!(BOX_CHARS_ASCII.vertical.chars().all(|c| c.is_ascii()));

        assert!(DIVIDER_CHARS_ASCII.solid.chars().all(|c| c.is_ascii()));
        assert!(HISTOGRAM_BAR_ASCII.is_ascii());
    }

    #[test]
    fn test_description_list() {
        let dl = DescriptionList::new()
            .item("Name", "Zed")
            .item("Version", "0.1.0")
            .item("License", "MIT");
        let lines = dl.render();

        // Should have 3 terms + 3 descriptions + 2 blank lines = 8 lines
        assert_eq!(lines.len(), 8);
    }

    #[test]
    fn test_description_list_compact() {
        let dl = DescriptionList::new()
            .item("Name", "Zed")
            .item("Version", "0.1.0")
            .compact(true);
        let lines = dl.render();

        // Compact mode: 2 terms + 2 descriptions = 4 lines (no blank lines)
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_description_list_variant() {
        let dl = DescriptionList::new()
            .item("Key", "Value")
            .variant(Variant::Success);
        let lines = dl.render();

        assert!(!lines.is_empty());
    }

    #[test]
    fn test_description_list_multiline_description() {
        let dl = DescriptionList::new().item("Description", "Line 1\nLine 2\nLine 3");
        let lines = dl.render();

        // 1 term + 3 description lines = 4 lines
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_description_list_accepts_styled_content() {
        let dl = DescriptionList::new().item("Status", "Ready".green());
        let lines = dl.render();

        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_unstyled().contains("Ready"));
    }

    #[test]
    fn test_description_list_accepts_multiline_styled_content() {
        let dl = DescriptionList::new().item("Status", "Ready\nSet".green());
        let lines = dl.render();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].to_unstyled().contains("Ready"));
        assert!(lines[1].to_unstyled().contains("Set"));
        assert!(lines[0].fmt_for_test().to_string().contains("fg=green"));
        assert!(lines[1].fmt_for_test().to_string().contains("fg=green"));
    }

    #[test]
    fn test_description_list_accepts_styled_line() {
        let mut description_line = Line::default();
        description_line.push(code("npm install"));
        description_line.push(unstyled_span(" --offline".to_string()));

        let dl = DescriptionList::new().item("Command", description_line);
        let lines = dl.render();

        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_unstyled().contains("npm install --offline"));
        assert!(lines[0].fmt_for_test().to_string().contains("fg=magenta"));
    }

    #[test]
    fn test_description_list_accepts_multiline_styled_description() {
        let mut first_line = Line::default();
        first_line.push(mark("First"));

        let mut second_line = Line::default();
        second_line.push(code("Second"));

        let dl = DescriptionList::new().item("Steps", vec![first_line, second_line]);
        let lines = dl.render();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].to_unstyled().contains("First"));
        assert!(lines[1].to_unstyled().contains("Second"));
    }

    #[test]
    fn test_description_list_items_method() {
        let items = vec![("Key1", "Value1"), ("Key2", "Value2")];
        let dl = DescriptionList::new().items(items);
        let lines = dl.render();

        // 2 terms + 2 descriptions + 1 blank line = 5 lines
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn test_code_inline() {
        let span = code("npm install");
        // Verify it returns a Span (compilation test mainly)
        assert!(span.content().len() > 0);
    }

    #[test]
    fn test_mark_inline() {
        let span = mark("important");
        assert!(span.content().len() > 0);
    }

    #[test]
    fn test_small_inline() {
        let span = small("© 2024");
        assert!(span.content().len() > 0);
    }

    #[test]
    fn test_del_inline() {
        let span = del("obsolete");
        assert!(span.content().len() > 0);
    }

    #[test]
    fn test_ins_inline() {
        let span = ins("new feature");
        assert!(span.content().len() > 0);
    }

    #[test]
    fn test_kbd_inline() {
        let span = kbd("Ctrl+C");
        assert!(span.content().len() > 0);
    }

    #[test]
    fn test_var_inline() {
        let span = var("user_name");
        assert!(span.content().len() > 0);
    }

    #[test]
    fn test_samp_inline() {
        let span = samp("Command output");
        assert!(span.content().len() > 0);
    }

    #[test]
    fn test_subscript_conversion() {
        let result = convert_to_subscript("0123456789");
        assert_eq!(result, "₀₁₂₃₄₅₆₇₈₉");

        let result = convert_to_subscript("H2O");
        assert_eq!(result, "H₂O");

        // Note: 'x' and 'n' both have subscript equivalents
        let result = convert_to_subscript("x(n+1)");
        assert_eq!(result, "ₓ₍ₙ₊₁₎");
    }

    #[test]
    fn test_superscript_conversion() {
        let result = convert_to_superscript("0123456789");
        assert_eq!(result, "⁰¹²³⁴⁵⁶⁷⁸⁹");

        let result = convert_to_superscript("E=mc2");
        assert_eq!(result, "E⁼mc²");

        // Note: 'n' has superscript equivalent, but not other letters
        let result = convert_to_superscript("x(n+1)");
        assert_eq!(result, "x⁽ⁿ⁺¹⁾");
    }

    #[test]
    fn test_sub_text_inline() {
        let span = sub_text("2");
        assert_eq!(span.content(), "₂");
    }

    #[test]
    fn test_sup_text_inline() {
        let span = sup_text("2");
        assert_eq!(span.content(), "²");
    }

    #[test]
    fn test_subscript_preserves_unknown_chars() {
        let result = convert_to_subscript("ABC");
        assert_eq!(result, "ABC"); // A, B, C don't have subscript equivalents
    }

    #[test]
    fn test_superscript_preserves_unknown_chars() {
        let result = convert_to_superscript("ABC");
        assert_eq!(result, "ABC"); // A, B, C don't have superscript equivalents (except i and n)
    }

    #[test]
    fn test_icon_functions_unicode_mode() {
        // In default Unicode mode, icons should return non-empty strings
        assert_eq!(icon_success(), "✓");
        assert_eq!(icon_danger(), "✗");
        assert_eq!(icon_warning(), "⚠");
        assert_eq!(icon_info(), "ℹ");
    }

    #[test]
    fn test_icon_functions_not_empty_in_unicode() {
        // Verify icons are not empty in default (Unicode) mode
        assert!(!icon_success().is_empty());
        assert!(!icon_danger().is_empty());
        assert!(!icon_warning().is_empty());
        assert!(!icon_info().is_empty());
    }
}
