use std::sync::Arc;

enum Printer<'a> {
    Printer(&'a mut printer::Printer),
    Progress(&'a mut printer::MultiProgressBar),
}

pub struct Logger<'a> {
    printer: Printer<'a>,
    label: Arc<str>,
}

impl Logger<'_> {
    pub fn new_printer(printer: &mut printer::Printer, label: Arc<str>) -> Logger {
        Logger {
            printer: Printer::Printer(printer),
            label,
        }
    }

    pub fn new_progress(progress: &mut printer::MultiProgressBar, label: Arc<str>) -> Logger {
        Logger {
            printer: Printer::Progress(progress),
            label,
        }
    }

    pub fn trace(&mut self, message: &str) {
        self.log(printer::Level::Trace, message);
    }

    pub fn debug(&mut self, message: &str) {
        self.log(printer::Level::Debug, message);
    }

    pub fn message(&mut self, message: &str) {
        self.log(printer::Level::Message, message);
    }

    pub fn info(&mut self, message: &str) {
        self.log(printer::Level::Info, message);
    }

    pub fn warning(&mut self, message: &str) {
        self.log(printer::Level::Warning, message);
    }

    pub fn error(&mut self, message: &str) {
        self.log(printer::Level::Error, message);
    }

    fn log(&mut self, level: printer::Level, message: &str) {
        let output = format!("[{}] {message}", self.label);
        let _ = match &mut self.printer {
            Printer::Printer(printer) => printer.log(level, output.as_str()),
            Printer::Progress(progress) => {
                progress.log(level, output.as_str());
                Ok(())
            }
        };
    }
}
