use std::sync::OnceLock;

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
pub(crate) struct BarCharsBounded {
    pub filled: char,
    pub tip: char,
    pub empty: char,
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

pub(crate) fn get_bar_chars_bounded() -> &'static BarCharsBounded {
    match typography_mode() {
        TypographyMode::Ascii => &BAR_CHARS_BOUNDED_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => &BAR_CHARS_BOUNDED_UNICODE,
    }
}

/// Spinner frames for indeterminate progress.
const SPINNER_FRAMES_ASCII: &[char] = &['|', '/', '-', '\\'];
const SPINNER_FRAMES_UNICODE: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub(crate) fn get_spinner_frames() -> &'static [char] {
    match typography_mode() {
        TypographyMode::Ascii => SPINNER_FRAMES_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => SPINNER_FRAMES_UNICODE,
    }
}

/// Box-drawing characters for tables and cards.
pub(crate) struct BoxChars {
    // Corners
    pub top_left: &'static str,
    pub top_right: &'static str,
    pub bottom_left: &'static str,
    pub bottom_right: &'static str,
    // Lines
    pub horizontal: &'static str,
    pub vertical: &'static str,
    // T-junctions
    pub left_t: &'static str,
    pub right_t: &'static str,
    pub top_t: &'static str,
    pub bottom_t: &'static str,
    pub cross: &'static str,
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

pub(crate) fn get_box_chars() -> &'static BoxChars {
    match typography_mode() {
        TypographyMode::Ascii => &BOX_CHARS_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => &BOX_CHARS_UNICODE,
    }
}

/// Characters for divider styles.
pub(crate) struct DividerChars {
    pub solid: &'static str,
    pub dashed: &'static str,
    pub dotted: &'static str,
    pub double: &'static str,
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

pub(crate) fn get_divider_chars() -> &'static DividerChars {
    match typography_mode() {
        TypographyMode::Ascii => &DIVIDER_CHARS_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => &DIVIDER_CHARS_UNICODE,
    }
}

/// Characters for histogram bars.
const HISTOGRAM_BAR_ASCII: char = '#';
const HISTOGRAM_BAR_UNICODE: char = '█';

pub(crate) fn get_histogram_bar_char() -> char {
    match typography_mode() {
        TypographyMode::Ascii => HISTOGRAM_BAR_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => HISTOGRAM_BAR_UNICODE,
    }
}

/// Characters for header underlines.
pub(crate) struct HeaderUnderlineChars {
    pub h1: &'static str,
    pub h2: &'static str,
}

const HEADER_UNDERLINE_ASCII: HeaderUnderlineChars = HeaderUnderlineChars { h1: "=", h2: "-" };

const HEADER_UNDERLINE_UNICODE: HeaderUnderlineChars = HeaderUnderlineChars {
    h1: "═", h2: "─"
};

pub(crate) fn get_header_underline_chars() -> &'static HeaderUnderlineChars {
    match typography_mode() {
        TypographyMode::Ascii => &HEADER_UNDERLINE_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => &HEADER_UNDERLINE_UNICODE,
    }
}

/// Characters for blockquote prefix.
const BLOCKQUOTE_PREFIX_ASCII: &str = "| ";
const BLOCKQUOTE_PREFIX_UNICODE: &str = "▐ ";

pub(crate) fn get_blockquote_prefix() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => BLOCKQUOTE_PREFIX_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => BLOCKQUOTE_PREFIX_UNICODE,
    }
}

/// Icon characters for status indicators.
const ICON_SUCCESS_UNICODE: &str = "✓";
const ICON_DANGER_UNICODE: &str = "✗";
const ICON_WARNING_UNICODE: &str = "⚠";
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

/// Logical/comparison operator symbols.
const OP_LESS_EQUAL_ASCII: &str = "<=";
const OP_LESS_EQUAL_UNICODE: &str = "≤";
const OP_GREATER_EQUAL_ASCII: &str = ">=";
const OP_GREATER_EQUAL_UNICODE: &str = "≥";
const OP_IMPLIES_ASCII: &str = "=>";
const OP_IMPLIES_UNICODE: &str = "⇒";
const OP_EQUIVALENT_ASCII: &str = "<=>";
const OP_EQUIVALENT_UNICODE: &str = "⇔";
const OP_ARROW_RIGHT_ASCII: &str = "->";
const OP_ARROW_RIGHT_UNICODE: &str = "→";
const OP_ARROW_LEFT_ASCII: &str = "<-";
const OP_ARROW_LEFT_UNICODE: &str = "←";
const OP_ARROW_BIDIRECTIONAL_ASCII: &str = "<->";
const OP_ARROW_BIDIRECTIONAL_UNICODE: &str = "↔";
const OP_DOUBLE_EQUAL_ASCII: &str = "==";
// No direct Unicode equivalent is universally used for "==", so keep ASCII form.
const OP_DOUBLE_EQUAL_UNICODE: &str = "==";

/// Returns a less-than-or-equal operator (`<=` in ASCII, `≤` in Unicode).
pub fn op_less_equal() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => OP_LESS_EQUAL_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => OP_LESS_EQUAL_UNICODE,
    }
}

/// Returns a greater-than-or-equal operator (`>=` in ASCII, `≥` in Unicode).
pub fn op_greater_equal() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => OP_GREATER_EQUAL_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => OP_GREATER_EQUAL_UNICODE,
    }
}

/// Returns an implication operator (`=>` in ASCII, `⇒` in Unicode).
pub fn op_implies() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => OP_IMPLIES_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => OP_IMPLIES_UNICODE,
    }
}

/// Returns an equivalence operator (`<=>` in ASCII, `⇔` in Unicode).
pub fn op_equivalent() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => OP_EQUIVALENT_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => OP_EQUIVALENT_UNICODE,
    }
}

/// Returns a right arrow operator (`->` in ASCII, `→` in Unicode).
pub fn op_arrow_right() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => OP_ARROW_RIGHT_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => OP_ARROW_RIGHT_UNICODE,
    }
}

/// Returns a left arrow operator (`<-` in ASCII, `←` in Unicode).
pub fn op_arrow_left() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => OP_ARROW_LEFT_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => OP_ARROW_LEFT_UNICODE,
    }
}

/// Returns a bidirectional arrow operator (`<->` in ASCII, `↔` in Unicode).
pub fn op_arrow_bidirectional() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => OP_ARROW_BIDIRECTIONAL_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => OP_ARROW_BIDIRECTIONAL_UNICODE,
    }
}

/// Returns an equality operator (`==` in both ASCII and Unicode modes).
pub fn op_double_equal() -> &'static str {
    match typography_mode() {
        TypographyMode::Ascii => OP_DOUBLE_EQUAL_ASCII,
        TypographyMode::Unicode | TypographyMode::NerdFonts => OP_DOUBLE_EQUAL_UNICODE,
    }
}

/// Replaces ASCII typography sequences with their mode-appropriate equivalents.
///
/// In `Ascii` mode, returns the input unchanged.
/// In `Unicode` and `NerdFonts` modes, replaces common ASCII operators and
/// punctuation with their Unicode glyph equivalents.
pub fn replace_ascii_with_typography(input: &str) -> String {
    replace_ascii_with_typography_for_mode(input, typography_mode())
}

fn replace_ascii_with_typography_for_mode(input: &str, mode: TypographyMode) -> String {
    if mode == TypographyMode::Ascii {
        return input.to_string();
    }

    // Order matters: replace longer multi-character sequences before shorter ones.
    let replacements = [
        (OP_EQUIVALENT_ASCII, OP_EQUIVALENT_UNICODE),
        (OP_ARROW_BIDIRECTIONAL_ASCII, OP_ARROW_BIDIRECTIONAL_UNICODE),
        (OP_LESS_EQUAL_ASCII, OP_LESS_EQUAL_UNICODE),
        (OP_GREATER_EQUAL_ASCII, OP_GREATER_EQUAL_UNICODE),
        (OP_IMPLIES_ASCII, OP_IMPLIES_UNICODE),
        (OP_ARROW_RIGHT_ASCII, OP_ARROW_RIGHT_UNICODE),
        (OP_ARROW_LEFT_ASCII, OP_ARROW_LEFT_UNICODE),
        ("...", "…"),
    ];

    let mut output = input.to_string();
    for (ascii, replacement) in replacements {
        output = output.replace(ascii, replacement);
    }

    output
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_operator_functions_unicode_mode() {
        // In default Unicode mode, operators should use Unicode glyphs where available
        assert_eq!(op_less_equal(), "≤");
        assert_eq!(op_greater_equal(), "≥");
        assert_eq!(op_implies(), "⇒");
        assert_eq!(op_equivalent(), "⇔");
        assert_eq!(op_arrow_right(), "→");
        assert_eq!(op_arrow_left(), "←");
        assert_eq!(op_arrow_bidirectional(), "↔");
        assert_eq!(op_double_equal(), "==");
    }

    #[test]
    fn test_operator_ascii_variants_are_ascii() {
        assert!(OP_LESS_EQUAL_ASCII.chars().all(|c| c.is_ascii()));
        assert!(OP_GREATER_EQUAL_ASCII.chars().all(|c| c.is_ascii()));
        assert!(OP_IMPLIES_ASCII.chars().all(|c| c.is_ascii()));
        assert!(OP_EQUIVALENT_ASCII.chars().all(|c| c.is_ascii()));
        assert!(OP_ARROW_RIGHT_ASCII.chars().all(|c| c.is_ascii()));
        assert!(OP_ARROW_LEFT_ASCII.chars().all(|c| c.is_ascii()));
        assert!(OP_ARROW_BIDIRECTIONAL_ASCII.chars().all(|c| c.is_ascii()));
        assert!(OP_DOUBLE_EQUAL_ASCII.chars().all(|c| c.is_ascii()));

        assert!(!OP_LESS_EQUAL_UNICODE.chars().all(|c| c.is_ascii()));
        assert!(!OP_GREATER_EQUAL_UNICODE.chars().all(|c| c.is_ascii()));
        assert!(!OP_IMPLIES_UNICODE.chars().all(|c| c.is_ascii()));
        assert!(!OP_EQUIVALENT_UNICODE.chars().all(|c| c.is_ascii()));
        assert!(!OP_ARROW_RIGHT_UNICODE.chars().all(|c| c.is_ascii()));
        assert!(!OP_ARROW_LEFT_UNICODE.chars().all(|c| c.is_ascii()));
        assert!(!OP_ARROW_BIDIRECTIONAL_UNICODE.chars().all(|c| c.is_ascii()));
        assert!(OP_DOUBLE_EQUAL_UNICODE.chars().all(|c| c.is_ascii()));
    }

    #[test]
    fn test_replace_ascii_with_typography_for_ascii_mode() {
        let input = "a <= b >= c => d <=> e -> f <- g <-> h ... == i";
        assert_eq!(
            replace_ascii_with_typography_for_mode(input, TypographyMode::Ascii),
            input
        );
    }

    #[test]
    fn test_replace_ascii_with_typography_for_unicode_mode() {
        let input = "a <= b >= c => d <=> e -> f <- g <-> h ... == i";
        assert_eq!(
            replace_ascii_with_typography_for_mode(input, TypographyMode::Unicode),
            "a ≤ b ≥ c ⇒ d ⇔ e → f ← g ↔ h … == i"
        );
    }

    #[test]
    fn test_replace_ascii_with_typography_for_nerd_fonts_mode() {
        let input = "a <= b >= c => d <=> e -> f <- g <-> h ... == i";
        assert_eq!(
            replace_ascii_with_typography_for_mode(input, TypographyMode::NerdFonts),
            "a ≤ b ≥ c ⇒ d ⇔ e → f ← g ↔ h … == i"
        );
    }

    #[test]
    fn test_replace_ascii_with_typography_replacement_order() {
        assert_eq!(
            replace_ascii_with_typography_for_mode(
                "<=> <-> <= >= => -> <-",
                TypographyMode::Unicode
            ),
            "⇔ ↔ ≤ ≥ ⇒ → ←"
        );
    }
}
