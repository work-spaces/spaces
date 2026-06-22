use crossterm::style::{self, Attribute, Attributes, Color, ContentStyle};
use superconsole::Span;
use termwiz::cell::Hyperlink;

// ---------------------------------------------------------------------------
// Helper functions for creating styled spans
// ---------------------------------------------------------------------------

pub(crate) fn styled_span(style: ContentStyle, text: String) -> Span {
    Span::new_styled_lossy(style::StyledContent::new(style, text))
}

pub(crate) fn unstyled_span(text: String) -> Span {
    Span::new_unstyled_lossy(text)
}

pub(crate) fn hyperlinked_span(style: ContentStyle, text: String, url: String) -> Span {
    let span = Span::new_styled_lossy(style::StyledContent::new(style, text));
    span.with_hyperlink(Some(Hyperlink::new(url)))
}

// ---------------------------------------------------------------------------
// Inline Text Styling
// ---------------------------------------------------------------------------

pub fn plain_text(text: impl Into<String>) -> Span {
    unstyled_span(text.into())
}

/// Inline code text (monospace).
///
/// # Examples
///
/// ```rust
/// use spaces_console::components::code;
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
/// use spaces_console::components::mark;
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
/// use spaces_console::components::small;
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
/// use spaces_console::components::del;
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
/// use spaces_console::components::ins;
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
/// use spaces_console::components::sub_text;
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
/// use spaces_console::components::sup_text;
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
/// use spaces_console::components::kbd;
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
/// use spaces_console::components::var;
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
/// use spaces_console::components::samp;
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
pub(super) fn convert_to_subscript(text: &str) -> String {
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

pub(super) fn convert_to_superscript(text: &str) -> String {
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
