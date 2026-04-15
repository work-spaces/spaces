use anyhow::Context;
use anyhow_source_location::format_context;
use std::sync::{Arc, Mutex, RwLock};

mod file;
mod null;
mod process;
mod secrets;
mod super_console;
pub mod ui;
mod verbosity;
mod writer;

pub use crossterm::style;
pub use process::{ExecuteOptions, ExecuteResult, LogHeader, get_log_divider};
pub use secrets::Secrets;
pub use superconsole::{Line, Span};
pub use ui::format_duration;
pub use verbosity::{Level, Verbosity};
use writer::ConsoleWriter;

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

#[derive(clap::ValueEnum, Debug, Clone, Default)]
pub enum Format {
    #[default]
    Pretty,
    Yaml,
    Json,
}

// ---------------------------------------------------------------------------
// Shared ContentStyle helpers
// ---------------------------------------------------------------------------

pub fn name_style() -> style::ContentStyle {
    style::ContentStyle {
        foreground_color: Some(style::Color::Cyan),
        background_color: None,
        underline_color: None,
        attributes: style::Attributes::from(style::Attribute::Bold),
    }
}

pub fn key_style() -> style::ContentStyle {
    style::ContentStyle {
        foreground_color: Some(style::Color::DarkGrey),
        background_color: None,
        underline_color: None,
        attributes: style::Attributes::default(),
    }
}

pub fn keyword_style() -> style::ContentStyle {
    style::ContentStyle {
        foreground_color: Some(style::Color::DarkYellow),
        background_color: None,
        underline_color: None,
        attributes: style::Attributes::from(style::Attribute::Bold),
    }
}

pub fn warning_style() -> style::ContentStyle {
    style::ContentStyle {
        foreground_color: Some(style::Color::DarkRed),
        background_color: None,
        underline_color: None,
        attributes: style::Attributes::from(style::Attribute::Bold),
    }
}

pub fn total_style() -> style::ContentStyle {
    style::ContentStyle {
        foreground_color: None,
        background_color: None,
        underline_color: None,
        attributes: style::Attributes::from(style::Attribute::Bold),
    }
}

#[derive(strum::Display)]
pub enum FinalType {
    Completed,
    Failed,
    NotRequired,
    NoChanges,
    NotPlatform,
    Cancelled,
    Restored,
    Finished,
}

const FINALIZE_PREFIX_WIDTH: usize = 12;

pub fn make_finalize_line(
    prefix: FinalType,
    duration: Option<std::time::Duration>,
    message: &str,
) -> Vec<Line> {
    let color = match prefix {
        FinalType::Completed => style::Color::Green,
        FinalType::Failed => style::Color::DarkRed,
        FinalType::NotRequired => style::Color::Cyan,
        FinalType::NoChanges => style::Color::Cyan,
        FinalType::NotPlatform => style::Color::Cyan,
        FinalType::Restored => style::Color::DarkGreen,
        FinalType::Cancelled => style::Color::Yellow,
        FinalType::Finished => style::Color::DarkCyan,
    };
    let bold_style = style::ContentStyle {
        foreground_color: Some(color),
        background_color: None,
        underline_color: None,
        attributes: style::Attributes::from(style::Attribute::Bold),
    };
    let padded_prefix = format!(
        "{prefix:>width$}: ",
        width = FINALIZE_PREFIX_WIDTH,
        prefix = prefix.to_string()
    );
    let styled_prefix = style::StyledContent::new(bold_style, padded_prefix);
    let mut line = Line::default();
    line.push(Span::new_styled_lossy(styled_prefix));
    if let Some(duration) = duration {
        let secs = duration.as_secs_f64();
        let duration_str = format!("[{}] ", format_duration(secs));
        line.push(Span::new_unstyled_lossy(&duration_str));
    }
    line.push(Span::new_unstyled_lossy(message));
    vec![line]
}

mod sealed {
    use super::*;
    pub struct State {
        pub(crate) secrets: Secrets,
        pub(crate) verbosity: Verbosity,
        pub(crate) start_time: std::time::Instant,
        pub(crate) shutdown_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
        pub(crate) is_refresh_thread_ready_to_join: Arc<std::sync::atomic::AtomicBool>,
    }
}

pub struct Console {
    writer: Arc<Mutex<Box<dyn ConsoleWriter>>>,
    state: Arc<RwLock<sealed::State>>,
}

impl Clone for Console {
    fn clone(&self) -> Self {
        Self {
            writer: Arc::clone(&self.writer),
            state: Arc::clone(&self.state),
        }
    }
}

impl Console {
    pub fn new_null() -> Self {
        Self {
            writer: Arc::new(Mutex::new(Box::new(null::Writer))),
            state: Arc::new(RwLock::new(sealed::State {
                secrets: Secrets::default(),
                verbosity: Verbosity::default(),
                start_time: std::time::Instant::now(),
                shutdown_flag: None,
                is_refresh_thread_ready_to_join: Arc::new(std::sync::atomic::AtomicBool::new(
                    false,
                )),
            })),
        }
    }

    pub fn new_file(path: &str) -> anyhow::Result<Self> {
        let file =
            std::fs::File::create(path).context(format_context!("Failed to create file {path}"))?;
        Ok(Self {
            writer: Arc::new(Mutex::new(Box::new(file::Writer::new(file)))),
            state: Arc::new(RwLock::new(sealed::State {
                secrets: Secrets::default(),
                verbosity: Verbosity::default(),
                start_time: std::time::Instant::now(),
                shutdown_flag: None,
                is_refresh_thread_ready_to_join: Arc::new(std::sync::atomic::AtomicBool::new(
                    false,
                )),
            })),
        })
    }

    pub fn new_stdout(verbosity: Verbosity) -> anyhow::Result<Self> {
        let super_console = super_console::Writer::new().context(format_context!(
            "Internal Error: failed to create super console",
        ))?;
        Ok(Self {
            writer: Arc::new(Mutex::new(Box::new(super_console))),
            state: Arc::new(RwLock::new(sealed::State {
                secrets: Secrets::default(),
                verbosity,
                start_time: std::time::Instant::now(),
                shutdown_flag: None,
                is_refresh_thread_ready_to_join: Arc::new(std::sync::atomic::AtomicBool::new(
                    false,
                )),
            })),
        })
    }

    pub fn start_refresh_thread(&self) -> std::thread::JoinHandle<()> {
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdown_flag = Arc::clone(&shutdown);
        let is_ready_to_join =
            Arc::clone(&self.state.read().unwrap().is_refresh_thread_ready_to_join);
        is_ready_to_join.store(false, std::sync::atomic::Ordering::Relaxed);
        self.state.write().unwrap().shutdown_flag = Some(shutdown_flag.clone());
        {
            let refresh_console = self.clone();
            std::thread::spawn(move || {
                while !shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    refresh_console.refresh();
                }
                is_ready_to_join.store(true, std::sync::atomic::Ordering::Relaxed);
            })
        }
    }

    pub fn is_refresh_thread_ready_to_join(&self) -> bool {
        self.state
            .read()
            .unwrap()
            .is_refresh_thread_ready_to_join
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn shutdown_refresh_thread(&self) {
        let state = self.state.read().unwrap();
        if let Some(shutdown_flag) = state.shutdown_flag.as_ref() {
            shutdown_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn raw<Type: std::fmt::Display>(&self, message: Type) -> anyhow::Result<()> {
        self.writer.lock().unwrap().write_str(&message)
    }

    pub fn write(&self, message: &str) -> anyhow::Result<()> {
        let redacted = self.state.read().unwrap().secrets.redact(message.into());
        self.writer.lock().unwrap().write_str(&redacted.as_ref())
    }

    pub fn emit_line(&self, line: superconsole::Line) {
        self.writer.lock().unwrap().emit_line(line);
    }

    pub(crate) fn emit_progress_line(&self, line: superconsole::Line) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer.lock().unwrap().emit_line(line);
        }
    }

    fn add_progress(&self, label: &str, prefix: &str, total: Option<u64>) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer
                .lock()
                .unwrap()
                .add_progress(label, prefix, total);
        }
    }

    fn insert_progress(&self, index: usize, label: &str, prefix: &str, total: Option<u64>) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer
                .lock()
                .unwrap()
                .insert_progress(index, label, prefix, total);
        }
    }

    fn set_progress_total(&self, label: &str, total: Option<u64>) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer.lock().unwrap().set_progress_total(label, total);
        }
    }

    fn set_progress_status(&self, label: &str, message: &str) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer
                .lock()
                .unwrap()
                .set_progress_message(label, message);
        }
    }

    fn set_progress_prefix(&self, label: &str, prefix: &str) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer
                .lock()
                .unwrap()
                .set_progress_prefix(label, prefix);
        }
    }

    fn update_progress(&self, label: &str, current: u64, total: u64) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer
                .lock()
                .unwrap()
                .update_progress(label, current, total);
        }
    }

    fn increment_progress(&self, label: &str, increment: u64) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer
                .lock()
                .unwrap()
                .increment_progress(label, increment);
        }
    }

    fn remove_progress(&self, label: &str) {
        self.writer.lock().unwrap().remove_progress(label);
    }

    fn reset_progress_elapsed(&self, label: &str) {
        self.writer.lock().unwrap().reset_progress_elapsed(label);
    }

    fn get_progress_elapsed(&self, label: &str) -> Option<std::time::Duration> {
        self.writer.lock().unwrap().get_progress_elapsed(label)
    }

    pub fn refresh(&self) {
        self.writer.lock().unwrap().refresh();
    }

    pub fn trace<Type: std::fmt::Display>(&self, name: &str, value: Type) -> anyhow::Result<()> {
        self.log(Level::Trace, &format!("{name}: {value}"))
    }

    pub fn debug<Type: std::fmt::Display>(&self, name: &str, value: Type) -> anyhow::Result<()> {
        self.log(Level::Debug, &format!("{name}: {value}"))
    }

    pub fn message<Type: std::fmt::Display>(&self, name: &str, value: Type) -> anyhow::Result<()> {
        self.log(Level::Message, &format!("{name}: {value}"))
    }

    pub fn info<Type: std::fmt::Display>(&self, name: &str, value: Type) -> anyhow::Result<()> {
        self.log(Level::Info, &format!("{name}: {value}"))
    }

    pub fn warning<Type: std::fmt::Display>(&self, name: &str, value: Type) -> anyhow::Result<()> {
        self.log(Level::Warning, &format!("{name}: {value}"))
    }

    pub fn error<Type: std::fmt::Display>(&self, name: &str, value: Type) -> anyhow::Result<()> {
        self.log(Level::Error, &format!("{name}: {value}"))
    }

    pub fn log(&self, level: Level, message: &str) -> anyhow::Result<()> {
        let state = self.state.read().unwrap();
        if state.verbosity.level <= level {
            let lines =
                verbosity::format_log(&state.verbosity, level, None, message, state.start_time);
            drop(state);
            for line in lines {
                self.emit_line(line);
            }
            Ok(())
        } else {
            Ok(())
        }
    }

    pub fn get_level(&self) -> Level {
        self.state.read().unwrap().verbosity.level
    }

    pub fn set_level(&self, level: Level) {
        self.state.write().unwrap().verbosity.level = level;
    }

    pub fn set_secrets(&self, secrets: Vec<Arc<str>>) {
        self.state.write().unwrap().secrets.secrets = secrets;
    }

    pub fn is_show_progress_bars(&self) -> bool {
        self.state.read().unwrap().verbosity.is_show_progress_bars
    }

    pub fn set_is_show_progress_bars(&self, value: bool) {
        self.state.write().unwrap().verbosity.is_show_progress_bars = value;
    }

    pub fn is_show_elapsed_time(&self) -> bool {
        self.state.read().unwrap().verbosity.is_show_elapsed_time
    }

    pub fn set_is_show_elapsed_time(&self, value: bool) {
        self.state.write().unwrap().verbosity.is_show_elapsed_time = value;
    }

    pub fn is_tty(&self) -> bool {
        self.state.read().unwrap().verbosity.is_tty
    }

    pub fn set_is_tty(&self, value: bool) {
        self.state.write().unwrap().verbosity.is_tty = value;
    }

    fn execute_process_with_progress_label(
        &self,
        command: &str,
        options: ExecuteOptions,
        label: &str,
    ) -> anyhow::Result<ExecuteResult> {
        use std::sync::mpsc;

        let child = options
            .spawn(command)
            .context(format_context!("Failed to spawn command {command}"))?;
        let (tx, rx) = mpsc::channel::<String>();

        let label_clone = label.to_string();
        let command_clone = command.to_string();
        let log_level = options.log_level;
        let verbosity = self.state.read().unwrap().verbosity;

        let console = self.clone();
        let status_thread = std::thread::spawn(move || {
            let start_time = std::time::Instant::now();

            let is_app = log_level.is_some_and(|level| level == Level::App);
            let is_passhrough = log_level.is_some_and(|level| level == Level::Passthrough);
            while let Ok(message) = rx.recv() {
                let mut writer = console.writer.lock().unwrap();
                writer.set_progress_message(&label_clone, &message);
                if (is_passhrough || is_app)
                    && let Some(level) = log_level.as_ref()
                {
                    let lines = verbosity::format_log(
                        &verbosity,
                        *level,
                        Some(command_clone.as_ref()),
                        message.as_str(),
                        start_time,
                    );
                    for line in lines {
                        writer.emit_line(line);
                    }
                }
            }
            console.writer.lock().unwrap().remove_progress(&label_clone);
        });

        let secrets = self.state.read().unwrap().secrets.clone();
        let result = process::monitor_process(command, child, &tx, &options, &secrets);
        drop(tx);
        let _ = status_thread.join();
        result
    }

    pub fn execute_process(
        &self,
        command: &str,
        options: ExecuteOptions,
    ) -> anyhow::Result<ExecuteResult> {
        let label = options.label.clone();
        self.execute_process_with_progress_label(command, options, &label)
    }
}

pub struct Progress {
    pub console: Console,
    label: String,
    final_message: Vec<superconsole::Line>,
}

impl Progress {
    pub fn new<LabelType: std::fmt::Display>(
        console: Console,
        label: LabelType,
        total: Option<u64>,
        final_message: Option<String>,
    ) -> Self {
        let label = label.to_string();
        console.add_progress(label.as_str(), label.as_str(), total);
        Self {
            console,
            label,
            final_message: super_console::string_to_lines(final_message.as_deref()),
        }
    }

    pub fn new_insert<LabelType: std::fmt::Display>(
        console: Console,
        index: usize,
        label: LabelType,
        total: Option<u64>,
        final_message: Option<String>,
    ) -> Self {
        let label = label.to_string();
        console.insert_progress(index, label.as_str(), label.as_str(), total);
        Self {
            console,
            label,
            final_message: super_console::string_to_lines(final_message.as_deref()),
        }
    }

    pub fn set_finalize_lines(&mut self, lines: Vec<Line>) {
        self.final_message = lines;
    }

    pub fn set_finalize_none(&mut self) {
        self.final_message = vec![];
    }

    pub fn set_finalize<FinalMessageType: std::fmt::Display>(
        &mut self,
        final_message: FinalMessageType,
    ) {
        self.final_message = super_console::string_to_lines(Some(&final_message.to_string()));
    }

    pub fn set_total(&self, total: Option<u64>) {
        self.console.set_progress_total(self.label.as_ref(), total);
    }

    pub fn set_prefix(&self, message: &str) {
        self.console
            .set_progress_prefix(self.label.as_ref(), message);
    }

    pub fn set_message(&self, message: &str) {
        self.console
            .set_progress_status(self.label.as_ref(), message);
    }

    pub fn update_progress(&self, current: u64, total: u64) {
        self.console
            .update_progress(self.label.as_ref(), current, total);
    }

    pub fn increment_progress(&self) {
        self.console.increment_progress(self.label.as_ref(), 1);
    }

    pub fn increment(&self, increment: u64) {
        self.console
            .increment_progress(self.label.as_ref(), increment);
    }

    pub fn increment_with_overflow(&self, increment: u64) {
        self.console
            .increment_progress(self.label.as_ref(), increment);
    }

    pub fn reset_elapsed(&self) {
        self.console.reset_progress_elapsed(self.label.as_ref());
    }

    pub fn elapsed(&self) -> Option<std::time::Duration> {
        self.console.get_progress_elapsed(self.label.as_ref())
    }

    pub fn execute_process(
        &self,
        command: &str,
        options: ExecuteOptions,
    ) -> anyhow::Result<ExecuteResult> {
        self.console
            .execute_process_with_progress_label(command, options, self.label.as_ref())
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.console.remove_progress(self.label.as_ref());
        for line in self.final_message.drain(..) {
            self.console.emit_progress_line(line);
        }
    }
}
