use crate::lock;
use std::sync::Arc;

pub use console::{FinalType, make_finalize_line};

pub struct Logger {
    console: console::Console,
    label: Arc<str>,
}

#[derive(Default, Debug)]
struct DeprecationWarnings {
    deprecation_warnings_enabled: bool,
    warnings: Vec<Arc<str>>,
}

static DEFERRED_WARNINGS: state::InitCell<lock::StateLock<DeprecationWarnings>> =
    state::InitCell::new();

fn get_deferred_warnings_state() -> &'static lock::StateLock<DeprecationWarnings> {
    if let Some(state) = DEFERRED_WARNINGS.try_get() {
        return state;
    }

    DEFERRED_WARNINGS.set(lock::StateLock::new(DeprecationWarnings::default()));
    DEFERRED_WARNINGS.get()
}

fn push_deferred_warning(warning: Arc<str>) {
    let mut state = get_deferred_warnings_state().write();
    state.warnings.push(warning);
}

pub fn enable_deprecation_warnings() {
    let mut state = get_deferred_warnings_state().write();
    state.deprecation_warnings_enabled = true;
}

pub fn push_deprecation_warning<Message: std::fmt::Display>(
    module: Option<Arc<str>>,
    warning: Message,
) {
    let is_deprecation_warning_enabled = {
        let state = get_deferred_warnings_state().read();
        state.deprecation_warnings_enabled
    };
    if is_deprecation_warning_enabled {
        let module = module.unwrap_or("unknown".into());
        push_deferred_warning(format!("{module} => {warning}").into());
    }
}

pub fn get_deferred_warnings() -> Vec<Arc<str>> {
    let state = get_deferred_warnings_state().read();
    state.warnings.clone()
}

impl Logger {
    pub fn new(console: console::Console, label: Arc<str>) -> Logger {
        Logger { console, label }
    }

    pub fn trace(&self, message: &str) {
        self.log(console::Level::Trace, message);
    }

    pub fn debug(&self, message: &str) {
        self.log(console::Level::Debug, message);
    }

    pub fn message(&self, message: &str) {
        self.log(console::Level::Message, message);
    }

    pub fn info(&self, message: &str) {
        self.log(console::Level::Info, message);
    }

    pub fn app(&self, message: &str) {
        self.log(console::Level::App, message);
    }

    pub fn raw(&self, message: &str) {
        let _ = self.console.raw(message);
    }

    pub fn warning(&self, message: &str) {
        let deferred = format!("[{}] {message}", self.label);
        push_deferred_warning(deferred.into());
    }

    pub fn error(&self, message: &str) {
        self.log(console::Level::Error, message);
    }

    fn log(&self, level: console::Level, message: &str) {
        let output = format!("[{}] {message}", self.label);
        let _ = self.console.log(level, &output);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShowBacktrace {
    No,
    Yes,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StripSourceLocation {
    No,
    Yes,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShowFailedBanner {
    No,
    Yes,
}

fn is_source_location_fragment(fragment: &str) -> bool {
    let Some((path, line)) = fragment.rsplit_once(':') else {
        return false;
    };

    !path.is_empty()
        && path.contains('/')
        && line.chars().all(|ch| ch.is_ascii_digit())
        && !line.is_empty()
}

fn strip_source_locations(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut remaining = line;

    while let Some(open_idx) = remaining.find('[') {
        output.push_str(&remaining[..open_idx]);

        let bracket_slice = &remaining[open_idx + 1..];
        let Some(close_rel_idx) = bracket_slice.find(']') else {
            output.push_str(&remaining[open_idx..]);
            remaining = "";
            break;
        };

        let fragment = &bracket_slice[..close_rel_idx];

        if is_source_location_fragment(fragment) {
            remaining = &bracket_slice[close_rel_idx + 1..];
            continue;
        }

        output.push_str(&remaining[open_idx..open_idx + close_rel_idx + 2]);
        remaining = &bracket_slice[close_rel_idx + 1..];
    }

    output.push_str(remaining);
    output.to_string()
}

pub fn show_error(
    console: console::Console,
    input_error_chain: Vec<String>,
    show_backtrace: ShowBacktrace,
    strip_source_location: StripSourceLocation,
    show_failed_banner: ShowFailedBanner,
) {
    let mut container = console::bootstrap::Container::new();
    container.add(console::bootstrap::VerticalSpacer::new(1));

    let mut error_lines = Vec::new();
    let mut error_chain = Vec::new();
    if show_backtrace == ShowBacktrace::Yes {
        for cause in input_error_chain.into_iter().rev() {
            error_chain.push(cause.to_string());
        }
    } else if let Some(last) = input_error_chain.first() {
        error_chain.push(last.to_string());
    }

    for cause in error_chain {
        for line in cause.lines() {
            let line = if strip_source_location == StripSourceLocation::Yes {
                strip_source_locations(line)
            } else {
                line.to_string()
            };

            if line.is_empty() {
                continue;
            }
            error_lines.push(line);
        }
    }

    if show_failed_banner == ShowFailedBanner::Yes {
        container.add(
            console::bootstrap::Banner::new(format!(
                "{} Failed",
                console::bootstrap::icon_danger()
            ))
            .width(console::bootstrap::Width::Large)
            .variant(console::bootstrap::Variant::Danger),
        );

        let mut error_quote =
            console::bootstrap::Blockquote::new().variant(console::bootstrap::Variant::Danger);
        for error_line in error_lines {
            error_quote.push_line(error_line);
        }
        container.add(error_quote);
        container.add(
            console::bootstrap::Divider::new()
                .style(console::bootstrap::DividerStyle::Double)
                .width(console::bootstrap::Width::Large),
        );
    } else {
        for error_line in error_lines {
            container.add(console::bootstrap::Paragraph::new(error_line));
        }
    }

    console.emit_container(&container);
}
