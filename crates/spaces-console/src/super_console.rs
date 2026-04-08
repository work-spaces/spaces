use crate::ui;
use crate::writer::ConsoleWriter;

pub(crate) fn string_to_lines(message: Option<&str>) -> Vec<superconsole::Line> {
    match message {
        None => vec![],
        Some(s) => s
            .split('\n')
            .map(|l| superconsole::Line::from_iter([superconsole::Span::new_unstyled_lossy(l)]))
            .collect(),
    }
}

pub(crate) struct Writer {
    console: Option<superconsole::SuperConsole>,
    component: ui::UiComponent,
}

impl Writer {
    pub(crate) fn new() -> anyhow::Result<Self> {
        Ok(Self {
            console: superconsole::SuperConsole::new(),
            component: ui::UiComponent {
                active_progress: Vec::new(),
            },
        })
    }
}

impl ConsoleWriter for Writer {
    fn write_str(&mut self, s: &dyn std::fmt::Display) -> anyhow::Result<()> {
        let s = s.to_string();
        let message = s.trim_end_matches('\n');
        if !message.is_empty() {
            if let Some(console) = self.console.as_mut() {
                let lines: Vec<superconsole::Line> = message
                    .split('\n')
                    .map(|l| {
                        superconsole::Line::from_iter([superconsole::Span::new_unstyled_lossy(l)])
                    })
                    .collect();
                console.emit(superconsole::Lines(lines));
            } else {
                println!("{}", message);
            }
        }
        Ok(())
    }

    fn emit_line(&mut self, line: superconsole::Line) {
        if let Some(console) = self.console.as_mut() {
            console.emit(superconsole::Lines(vec![line]));
        }
    }

    fn add_progress(&mut self, label: &str, prefix: &str, total: Option<u64>) {
        self.component.active_progress.push(ui::ActiveProgress {
            name: label.to_string(),
            prefix: prefix.to_string(),
            message: String::new(),
            position: 0,
            total,
            start_time: std::time::Instant::now(),
        });
    }

    fn insert_progress(&mut self, index: usize, label: &str, prefix: &str, total: Option<u64>) {
        let index = index.min(self.component.active_progress.len());
        self.component.active_progress.insert(
            index,
            ui::ActiveProgress {
                name: label.to_string(),
                prefix: prefix.to_string(),
                message: String::new(),
                position: 0,
                total,
                start_time: std::time::Instant::now(),
            },
        );
    }

    fn set_progress_message(&mut self, label: &str, message: &str) {
        if let Some(entry) = self
            .component
            .active_progress
            .iter_mut()
            .find(|p| p.name == label)
        {
            entry.message = message.trim().to_string();
        }
    }

    fn set_progress_prefix(&mut self, label: &str, prefix: &str) {
        if let Some(entry) = self
            .component
            .active_progress
            .iter_mut()
            .find(|p| p.name == label)
        {
            entry.prefix = prefix.to_string();
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
    }

    fn increment_progress(&mut self, label: &str, increment: u64) {
        if let Some(entry) = self
            .component
            .active_progress
            .iter_mut()
            .find(|p| p.name == label)
        {
            entry.position += increment;
        }
    }

    fn set_progress_total(&mut self, label: &str, total: Option<u64>) {
        if let Some(entry) = self
            .component
            .active_progress
            .iter_mut()
            .find(|p| p.name == label)
        {
            entry.total = total;
        }
    }

    fn remove_progress(&mut self, label: &str) {
        self.component.active_progress.retain(|p| p.name != label);
    }

    fn reset_progress_elapsed(&mut self, label: &str) {
        if let Some(entry) = self
            .component
            .active_progress
            .iter_mut()
            .find(|p| p.name == label)
        {
            entry.start_time = std::time::Instant::now();
        }
    }

    fn get_progress_elapsed(&self, label: &str) -> Option<std::time::Duration> {
        self.component
            .active_progress
            .iter()
            .find(|p| p.name == label)
            .map(|entry| entry.start_time.elapsed())
    }

    fn refresh(&mut self) {
        if let Some(console) = self.console.as_mut() {
            let _ = console.render(&self.component);
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.component.active_progress.clear();
        if let Some(console) = self.console.take() {
            let _ = console.finalize(&self.component);
        }
    }
}
