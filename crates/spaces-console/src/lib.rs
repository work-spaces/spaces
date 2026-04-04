use anyhow::Context;
use anyhow_source_location::format_context;
use std::sync::{Arc, Mutex, RwLock};

mod process;
mod secrets;
pub mod ui;
mod verbosity;

pub use process::ExecuteOptions;
pub use secrets::Secrets;
pub use verbosity::{Level, Verbosity};

trait ConsoleWriter: Send {
    fn write_str(&mut self, s: &dyn std::fmt::Display) -> anyhow::Result<()>;
    fn emit_line(&mut self, line: superconsole::Line);
    fn add_progress(&mut self, label: &str, total: Option<u64>);
    fn set_progress_status(&mut self, label: &str, message: &str);
    fn update_progress(&mut self, label: &str, current: u64, total: u64);
    fn increment_progress(&mut self, label: &str);
    fn remove_progress(&mut self, label: &str);
}

struct SuperConsoleWriter {
    console: Option<superconsole::SuperConsole>,
    component: ui::UiComponent,
}

impl ConsoleWriter for SuperConsoleWriter {
    fn write_str(&mut self, s: &dyn std::fmt::Display) -> anyhow::Result<()> {
        let s = s.to_string();
        let message = s.trim_end_matches('\n');
        if !message.is_empty() {
            let line =
                superconsole::Line::from_iter([superconsole::Span::new_unstyled_lossy(message)]);
            if let Some(console) = self.console.as_mut() {
                console.emit(superconsole::Lines(vec![line]));
            }
        }
        Ok(())
    }

    fn emit_line(&mut self, line: superconsole::Line) {
        if let Some(console) = self.console.as_mut() {
            console.emit(superconsole::Lines(vec![line]));
        }
    }

    fn add_progress(&mut self, label: &str, total: Option<u64>) {
        self.component.active_progress.push(ui::ActiveProgress {
            name: label.to_string(),
            message: String::new(),
            position: 0,
            total,
            start_time: std::time::Instant::now(),
        });
        if let Some(console) = self.console.as_mut() {
            let _ = console.render(&self.component);
        }
    }

    fn set_progress_status(&mut self, label: &str, message: &str) {
        if let Some(entry) = self
            .component
            .active_progress
            .iter_mut()
            .find(|p| p.name == label)
        {
            entry.message = message.to_string();
        }
        if let Some(console) = self.console.as_mut() {
            let _ = console.render(&self.component);
        }
    }

    fn update_progress(&mut self, label: &str, current: u64, total: u64) {
        if let Some(entry) = self
            .component
            .active_progress
            .iter_mut()
            .find(|p| p.name == label)
        {
            entry.position = current;
            entry.total = Some(total);
        }
        if let Some(console) = self.console.as_mut() {
            let _ = console.render(&self.component);
        }
    }

    fn increment_progress(&mut self, label: &str) {
        if let Some(entry) = self
            .component
            .active_progress
            .iter_mut()
            .find(|p| p.name == label)
        {
            entry.position += 1;
        }
        if let Some(console) = self.console.as_mut() {
            let _ = console.render(&self.component);
        }
    }

    fn remove_progress(&mut self, label: &str) {
        self.component.active_progress.retain(|p| p.name != label);
        if let Some(console) = self.console.as_mut() {
            let _ = console.render(&self.component);
        }
    }
}

impl Drop for SuperConsoleWriter {
    fn drop(&mut self) {
        if let Some(console) = self.console.take() {
            let _ = console.finalize(&self.component);
        }
    }
}

mod sealed {
    use super::*;
    pub struct State {
        pub(crate) secrets: Secrets,
        pub(crate) verbosity: Verbosity,
        pub(crate) start_time: std::time::Instant,
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
    pub fn new_stdout(verbosity: Verbosity) -> anyhow::Result<Self> {
        let console = superconsole::SuperConsole::new().context(format_context!(
            "Internal Error: failed to create super console",
        ))?;
        Ok(Self {
            writer: Arc::new(Mutex::new(Box::new(SuperConsoleWriter {
                console: Some(console),
                component: ui::UiComponent {
                    active_progress: Vec::new(),
                },
            }))),
            state: Arc::new(RwLock::new(sealed::State {
                secrets: Secrets::default(),
                verbosity,
                start_time: std::time::Instant::now(),
            })),
        })
    }

    pub fn raw<Type: std::fmt::Display>(&self, message: Type) -> anyhow::Result<()> {
        self.writer.lock().unwrap().write_str(&message)
    }

    pub(crate) fn write(&self, message: &str) -> anyhow::Result<()> {
        let redacted = self.state.read().unwrap().secrets.redact(message.into());
        self.writer.lock().unwrap().write_str(&redacted.as_ref())
    }

    pub(crate) fn emit_line(&self, line: superconsole::Line) {
        self.writer.lock().unwrap().emit_line(line);
    }

    fn add_progress(&self, label: &str, total: Option<u64>) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer.lock().unwrap().add_progress(label, total);
        }
    }

    fn set_progress_status(&self, label: &str, message: &str) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer
                .lock()
                .unwrap()
                .set_progress_status(label, message);
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

    fn increment_progress(&self, label: &str) {
        if self.state.read().unwrap().verbosity.is_show_progress_bars {
            self.writer.lock().unwrap().increment_progress(label);
        }
    }

    fn remove_progress(&self, label: &str) {
        self.writer.lock().unwrap().remove_progress(label);
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
            let line =
                verbosity::format_log(&state.verbosity, level, None, message, state.start_time);
            drop(state);
            self.emit_line(line);
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

    pub fn execute_process(
        &self,
        command: &str,
        options: ExecuteOptions,
    ) -> anyhow::Result<Option<String>> {
        use std::sync::mpsc;

        let child = options
            .spawn(command)
            .context(format_context!("Failed to spawn command {command}"))?;
        let (tx, rx) = mpsc::channel::<String>();
        let label = options.label.clone();

        self.writer.lock().unwrap().add_progress(&label, None);

        let label_clone = label.clone();
        let command_clone = command.to_string();
        let log_level = options.log_level.clone();
        let verbosity = self.state.read().unwrap().verbosity.clone();

        let console = self.clone();
        let status_thread = std::thread::spawn(move || {
            let start_time = std::time::Instant::now();

            let is_app = log_level.is_some_and(|level| level == Level::App);
            let is_passhrough = log_level.is_some_and(|level| level == Level::Passthrough);
            while let Ok(message) = rx.recv() {
                let mut writer = console.writer.lock().unwrap();
                writer.set_progress_status(&label_clone, &message);
                if (is_passhrough || is_app)
                    && let Some(level) = log_level.as_ref()
                {
                    let line = verbosity::format_log(
                        &verbosity,
                        *level,
                        Some(command_clone.as_ref()),
                        message.as_str(),
                        start_time,
                    );
                    writer.emit_line(line);
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
}

pub struct Progress {
    pub console: Console,
    label: Arc<str>,
    finalize: Option<Arc<str>>,
}

impl Progress {
    pub fn new(console: Console, label: Arc<str>, total: Option<u64>) -> Self {
        console.add_progress(label.as_ref(), total);
        Self {
            console,
            label,
            finalize: None,
        }
    }

    pub fn set_finalize(&mut self, finalize: Arc<str>) {
        self.finalize = Some(finalize);
    }

    pub fn set_progress_status(&self, message: &str) {
        self.console
            .set_progress_status(self.label.as_ref(), message);
    }

    pub fn update_progress(&self, current: u64, total: u64) {
        self.console
            .update_progress(self.label.as_ref(), current, total);
    }

    pub fn increment_progress(&self) {
        self.console.increment_progress(self.label.as_ref());
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.console.remove_progress(self.label.as_ref());
        if let Some(finalize) = self.finalize.as_ref() {
            let _ = self.console.write(finalize);
        }
    }
}
