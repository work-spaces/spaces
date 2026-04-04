use crossterm::style::{Attribute, Attributes, ContentStyle};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    strum::Display,
    Default,
    Serialize,
    Deserialize,
)]
pub enum Level {
    Trace,
    Debug,
    Message,
    #[default]
    Info,
    App,
    Passthrough,
    Warning,
    Error,
    Silent,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Verbosity {
    pub level: Level,
    pub is_show_progress_bars: bool,
    pub is_show_elapsed_time: bool,
    pub is_tty: bool,
}

pub(crate) fn format_log(
    level: Level,
    message: &str,
    is_show_elapsed_time: bool,
    start_time: std::time::Instant,
) -> superconsole::Line {
    let timestamp: Arc<str> = if is_show_elapsed_time {
        let elapsed = std::time::Instant::now() - start_time;
        format!("[{:.3}] ", elapsed.as_secs_f64()).into()
    } else {
        "".into()
    };

    let mut line = superconsole::Line::default();

    if level == Level::Passthrough {
        line.push(superconsole::Span::new_unstyled_lossy(&format!(
            "{timestamp}{message}"
        )));
    } else {
        let bold_style = ContentStyle {
            foreground_color: None,
            background_color: None,
            underline_color: None,
            attributes: Attributes::from(Attribute::Bold),
        };
        let level_label = format!("::{}", level.to_string().to_lowercase());
        line.push(superconsole::Span::new_styled_lossy(
            crossterm::style::StyledContent::new(bold_style, level_label),
        ));
        line.push(superconsole::Span::new_unstyled_lossy(&format!(
            "::{timestamp}{message}"
        )));
    }

    line
}
