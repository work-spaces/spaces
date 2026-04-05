pub(crate) trait ConsoleWriter: Send {
    fn write_str(&mut self, s: &dyn std::fmt::Display) -> anyhow::Result<()>;
    fn emit_line(&mut self, line: superconsole::Line);
    fn add_progress(&mut self, label: &str, total: Option<u64>);
    fn set_progress_status(&mut self, label: &str, message: &str);
    fn update_progress(&mut self, label: &str, current: u64, total: u64);
    fn increment_progress(&mut self, label: &str, increment: u64);
    fn set_progress_total(&mut self, label: &str, total: Option<u64>);
    fn remove_progress(&mut self, label: &str);
    fn reset_progress_elapsed(&mut self, label: &str);
}
