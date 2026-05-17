pub(crate) trait ConsoleWriter: Send {
    fn write_str(&mut self, s: &dyn std::fmt::Display) -> anyhow::Result<()>;
    fn emit_line(&mut self, line: superconsole::Line);
    fn add_progress(&mut self, label: &str, prefix: &str, total: Option<u64>);
    fn insert_progress(&mut self, index: usize, label: &str, prefix: &str, total: Option<u64>);
    fn remove_progress(&mut self, label: &str);
    fn set_progress_message(&mut self, label: &str, message: &str);
    fn set_progress_prefix(&mut self, label: &str, prefix: &str);
    fn update_progress(&mut self, label: &str, current: u64, total: u64);
    fn increment_progress(&mut self, label: &str, increment: u64);
    fn set_progress_total(&mut self, label: &str, total: Option<u64>);
    fn reset_progress_elapsed(&mut self, label: &str);
    fn get_progress_elapsed(&self, label: &str) -> Option<std::time::Duration>;
    fn refresh(&mut self) {}
    fn finalize(&mut self) {}
}
