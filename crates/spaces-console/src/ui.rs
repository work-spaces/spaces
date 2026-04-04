use crossterm::style::{Attribute, Attributes, Color, ContentStyle};
use superconsole::{Component, Dimensions, DrawMode, Line, Lines, Span};

use crate::verbosity::Level;

/// A single pending log line to be emitted above the progress bars.
pub struct LogLine {
    pub level: Level,
    pub message: String,
}

/// State for a single actively running process shown as a progress bar.
/// Mirrors the indicatif template: `{elapsed_precise}|{bar:.cyan/blue}|{name}: {message}`
pub struct ActiveProgress {
    pub name: String,
    pub message: String,
    /// Current progress position.
    pub position: u64,
    /// Total steps, or `None` for an indeterminate spinner.
    pub total: Option<u64>,
    pub start_time: std::time::Instant,
}

/// Width of the bar fill section in characters.
const BAR_WIDTH: usize = 20;

/// Characters used for a bounded bar: filled, tip, empty.
const BAR_CHARS_BOUNDED: (char, char, char) = ('#', '>', '-');

/// Characters used for an indeterminate bar: fill, tip, empty.
const BAR_CHARS_SPINNER: (char, char, char) = ('*', '>', '-');

impl ActiveProgress {
    fn render_bar(&self, max_width: usize) -> anyhow::Result<Line> {
        let elapsed = self.start_time.elapsed();
        let secs = elapsed.as_secs();
        let elapsed_str = format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        );

        let (filled_char, tip_char, empty_char) = if self.total.is_some() {
            BAR_CHARS_BOUNDED
        } else {
            BAR_CHARS_SPINNER
        };

        let filled_count = if let Some(total) = self.total {
            if total == 0 {
                0
            } else {
                (self.position as usize * BAR_WIDTH / total as usize) % BAR_WIDTH
            }
        } else {
            // Spinner: animate position within the bar width
            (self.position as usize) % BAR_WIDTH
        };

        let bar_str: String = {
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
        };

        let bar_style = ContentStyle {
            foreground_color: Some(Color::Cyan),
            background_color: None,
            underline_color: None,
            attributes: Attributes::default(),
        };

        let prefix_style = ContentStyle {
            foreground_color: None,
            background_color: None,
            underline_color: None,
            attributes: Attributes::from(Attribute::Bold),
        };

        // Format: `[H:MM:SS]|<bar>|name: message`
        let mut line = Line::default();
        line.push(Span::new_unstyled_lossy(&elapsed_str));
        line.push(Span::new_unstyled_lossy("|"));

        let bar_span =
            Span::new_styled_lossy(crossterm::style::StyledContent::new(bar_style, bar_str));
        line.push(bar_span);

        line.push(Span::new_unstyled_lossy("|"));

        let prefix_text = format!("{}: ", self.name);
        let prefix_span = Span::new_styled_lossy(crossterm::style::StyledContent::new(
            prefix_style,
            prefix_text,
        ));
        line.push(prefix_span);

        // Truncate message to fit remaining width
        let fixed_width = elapsed_str.len() + 1 + BAR_WIDTH + 1 + self.name.len() + 2;
        let msg_max = max_width.saturating_sub(fixed_width);
        let message: String = self.message.chars().take(msg_max).collect();
        line.push(Span::new_unstyled_lossy(&message));

        Ok(line)
    }
}

/// The root superconsole component. Renders one progress bar per active process
/// in the canvas (scratch) area at the bottom. Log lines are emitted above via
/// `SuperConsole::emit`.
pub struct UiComponent {
    pub active_progress: Vec<ActiveProgress>,
}

impl Component for UiComponent {
    type Error = anyhow::Error;

    fn draw_unchecked(
        &self,
        dimensions: Dimensions,
        _mode: DrawMode,
    ) -> Result<Lines, Self::Error> {
        let mut lines = Lines::default();
        for process in &self.active_progress {
            lines.0.push(process.render_bar(dimensions.width)?);
        }
        Ok(lines)
    }
}
