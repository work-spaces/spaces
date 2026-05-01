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
    pub prefix: String,
    pub message: String,
    /// Current progress position.
    pub position: u64,
    /// Total steps, or `None` for an indeterminate spinner.
    pub total: Option<u64>,
    pub start_time: std::time::Instant,
}

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

/// Width of the bar fill section in characters.
const BAR_WIDTH: usize = 16;

/// Characters used for a bounded bar: filled, tip, empty.
const BAR_CHARS_BOUNDED: (char, char, char) = ('#', '>', '-');

/// Frames for the indeterminate spinner, cycled by time elapsed.
const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// How many milliseconds each spinner frame is shown.
const SPINNER_FRAME_MS: u128 = 100;

impl ActiveProgress {
    fn render_bar(&self, max_width: usize) -> anyhow::Result<Line> {
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

        let prefix_text = format!("{}: ", self.prefix);
        let prefix_span = Span::new_styled_lossy(crossterm::style::StyledContent::new(
            prefix_style,
            prefix_text,
        ));
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
        let now = std::time::Instant::now();
        let total = self.active_progress.len();

        let mut progress_iter = self.active_progress.iter();
        let first = progress_iter.next();

        let mut hidden = 0;
        for progress in progress_iter {
            let duration = (now - progress.start_time).as_secs();
            if duration > 1 {
                lines.0.push(progress.render_bar(dimensions.width)?);
            } else {
                hidden += 1;
            }
        }

        if let Some(first) = first {
            lines.0.push(first.render_bar(dimensions.width)?);
        }

        if total > 0 {
            let hidden_str = if hidden > 0 {
                format!(" plus {hidden} more")
            } else {
                String::new()
            };
            let header_text = format!("Progress:{hidden_str}");
            let mut header_line = Line::default();
            header_line.push(Span::new_unstyled_lossy(&header_text));
            lines.0.insert(0, header_line);
        }
        Ok(lines)
    }
}
