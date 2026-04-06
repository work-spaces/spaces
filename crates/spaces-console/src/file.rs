use std::fs::File;
use std::io::Write as IoWrite;

use crate::writer::ConsoleWriter;

pub(crate) struct Writer {
    file: File,
}

impl Writer {
    pub(crate) fn new(file: File) -> Self {
        Self { file }
    }
}

impl ConsoleWriter for Writer {
    fn write_str(&mut self, s: &dyn std::fmt::Display) -> anyhow::Result<()> {
        let text = s.to_string();
        let message = text.trim_end_matches('\n');
        if !message.is_empty() {
            writeln!(self.file, "{message}")?;
        }
        Ok(())
    }

    fn emit_line(&mut self, line: superconsole::Line) {
        let text: String = line.iter().map(|span| span.content().to_owned()).collect();
        let _ = writeln!(self.file, "{text}");
    }

    fn add_progress(&mut self, _label: &str, _prefix: &str, _total: Option<u64>) {}

    fn insert_progress(&mut self, _index: usize, _label: &str, _prefix: &str, _total: Option<u64>) {
    }

    fn set_progress_message(&mut self, _label: &str, _message: &str) {}
    fn set_progress_prefix(&mut self, _label: &str, _prefix: &str) {}

    fn update_progress(&mut self, _label: &str, _current: u64, _total: u64) {}

    fn increment_progress(&mut self, _label: &str, _increment: u64) {}

    fn set_progress_total(&mut self, _label: &str, _total: Option<u64>) {}

    fn remove_progress(&mut self, _label: &str) {}

    fn reset_progress_elapsed(&mut self, _label: &str) {}

    fn get_progress_elapsed(&self, _label: &str) -> Option<std::time::Duration> {
        None
    }
}
