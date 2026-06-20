use superconsole::{Component, Dimensions, DrawMode, Line, Lines, Span};

use crate::verbosity::Level;

// Re-export ActiveProgress and Variant so existing code can still use ui::ActiveProgress and ui::Variant
pub use crate::typography::{ActiveProgress, Variant};

/// A single pending log line to be emitted above the progress bars.
pub struct LogLine {
    pub level: Level,
    pub message: String,
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
