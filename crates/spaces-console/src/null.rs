use crate::writer::ConsoleWriter;

pub(crate) struct Writer;

impl ConsoleWriter for Writer {
    fn write_str(&mut self, _s: &dyn std::fmt::Display) -> anyhow::Result<()> {
        Ok(())
    }

    fn emit_line(&mut self, _line: superconsole::Line) {}

    fn add_progress(&mut self, _label: &str, _prefix: &str, _total: Option<u64>) {}

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
