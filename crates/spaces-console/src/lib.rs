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
    fn add_progress(&mut self, label: &str);
    fn set_progress_status(&mut self, label: &str, message: &str);
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

    fn add_progress(&mut self, label: &str) {
        self.component.active_progress.push(ui::ActiveProgress {
            name: label.to_string(),
            message: String::new(),
            position: 0,
            total: None,
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

    pub fn add_progress(&self, label: &str) {
        self.writer.lock().unwrap().add_progress(label);
    }

    pub fn set_progress_status(&self, label: &str, message: &str) {
        self.writer
            .lock()
            .unwrap()
            .set_progress_status(label, message);
    }

    pub fn remove_progress(&self, label: &str) {
        self.writer.lock().unwrap().remove_progress(label);
    }

    pub fn trace<Type: std::fmt::Display>(&self, name: &str, value: &Type) -> anyhow::Result<()> {
        if self.state.read().unwrap().verbosity.level <= Level::Trace {
            self.write(format!("{name}: {value}\n").as_str())
        } else {
            Ok(())
        }
    }

    pub fn debug<Type: std::fmt::Display>(&self, name: &str, value: &Type) -> anyhow::Result<()> {
        if self.state.read().unwrap().verbosity.level <= Level::Debug {
            self.write(format!("{name}: {value}\n").as_str())
        } else {
            Ok(())
        }
    }

    pub fn message<Type: std::fmt::Display>(&self, name: &str, value: &Type) -> anyhow::Result<()> {
        if self.state.read().unwrap().verbosity.level <= Level::Message {
            self.write(format!("{name}: {value}\n").as_str())
        } else {
            Ok(())
        }
    }

    pub fn info<Type: std::fmt::Display>(&self, name: &str, value: &Type) -> anyhow::Result<()> {
        if self.state.read().unwrap().verbosity.level <= Level::Info {
            self.write(format!("{name}: {value}\n").as_str())
        } else {
            Ok(())
        }
    }

    pub fn warning<Type: std::fmt::Display>(&self, name: &str, value: &Type) -> anyhow::Result<()> {
        if self.state.read().unwrap().verbosity.level <= Level::Warning {
            self.write(format!("{name}: {value}\n").as_str())
        } else {
            Ok(())
        }
    }

    pub fn error<Type: std::fmt::Display>(&self, name: &str, value: &Type) -> anyhow::Result<()> {
        if self.state.read().unwrap().verbosity.level <= Level::Error {
            self.write(format!("{name}: {value}\n").as_str())
        } else {
            Ok(())
        }
    }

    pub fn log(&self, level: Level, message: &str) -> anyhow::Result<()> {
        let state = self.state.read().unwrap();
        if state.verbosity.level <= level {
            let line = verbosity::format_log(
                level,
                message,
                state.verbosity.is_show_elapsed_time,
                state.start_time,
            );
            drop(state);
            self.emit_line(line);
            Ok(())
        } else {
            Ok(())
        }
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
        let writer = Arc::clone(&self.writer);

        writer.lock().unwrap().add_progress(&label);

        let label_clone = label.clone();
        let status_thread = std::thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                writer
                    .lock()
                    .unwrap()
                    .set_progress_status(&label_clone, &message);
            }
            writer.lock().unwrap().remove_progress(&label_clone);
        });

        let secrets = self.state.read().unwrap().secrets.clone();
        let result = process::monitor_process(command, child, &tx, &options, &secrets);
        drop(tx);
        let _ = status_thread.join();
        result
    }
}
