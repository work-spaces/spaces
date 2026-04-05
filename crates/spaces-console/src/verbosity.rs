use crossterm::style::{Attribute, Attributes, Color, ContentStyle};
use serde::{Deserialize, Serialize};

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
    verbosity: &Verbosity,
    level: Level,
    app_message: Option<&str>,
    message: &str,
    start_time: std::time::Instant,
) -> Vec<superconsole::Line> {
    let timestamp = if verbosity.is_show_elapsed_time {
        let elapsed = std::time::Instant::now() - start_time;
        format!("[{:.3}] ", elapsed.as_secs_f64())
    } else {
        String::new()
    };

    let mut message_lines = message.split('\n');
    let first_line = message_lines.next().unwrap_or("");
    let rest_lines: Vec<&str> = message_lines.collect();

    let mut first = superconsole::Line::default();

    if level == Level::Passthrough {
        first.push(superconsole::Span::new_unstyled_lossy(format!(
            "{timestamp}{first_line}"
        )));
    } else if verbosity.is_tty {
        let foreground_color = match level {
            Level::Error => Some(Color::Red),
            Level::Warning => Some(Color::Yellow),
            Level::Trace => Some(Color::Blue),
            Level::Debug => Some(Color::Cyan),
            _ => None,
        };
        let bold_style = ContentStyle {
            foreground_color,
            background_color: None,
            underline_color: None,
            attributes: Attributes::from(Attribute::Bold),
        };
        if !timestamp.is_empty() {
            first.push(superconsole::Span::new_unstyled_lossy(&timestamp));
        }
        if let Some(app_message) = app_message {
            first.push(superconsole::Span::new_unstyled_lossy(app_message));
        } else {
            let level_label = level.to_string().to_lowercase();
            first.push(superconsole::Span::new_styled_lossy(
                crossterm::style::StyledContent::new(bold_style, level_label),
            ));
        }
        first.push(superconsole::Span::new_unstyled_lossy(format!(
            ":{first_line}"
        )));
    } else {
        let level_label = level.to_string().to_lowercase();
        first.push(superconsole::Span::new_unstyled_lossy(format!(
            "::{level_label}::{timestamp}{first_line}"
        )));
    }

    let mut result = vec![first];
    for line in rest_lines {
        result.push(superconsole::Line::from_iter([
            superconsole::Span::new_unstyled_lossy(line),
        ]));
    }
    result
}
