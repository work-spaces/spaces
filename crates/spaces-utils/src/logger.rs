use crate::lock;
use std::sync::Arc;

pub use console::{FinalType, make_finalize_line};

pub struct Logger {
    console: console::Console,
    label: Arc<str>,
}

static DEFERRED_WARNINGS: state::InitCell<lock::StateLock<Vec<Arc<str>>>> = state::InitCell::new();

fn get_deferred_warnings_state() -> &'static lock::StateLock<Vec<Arc<str>>> {
    if let Some(state) = DEFERRED_WARNINGS.try_get() {
        return state;
    }

    DEFERRED_WARNINGS.set(lock::StateLock::new(Vec::new()));
    DEFERRED_WARNINGS.get()
}

fn push_deferred_warning(warning: Arc<str>) {
    let mut state = get_deferred_warnings_state().write();
    state.push(warning);
}

pub fn push_deprecation_warning<Message: std::fmt::Display>(
    module: Option<Arc<str>>,
    warning: Message,
) {
    if let Ok(warn_deprecation) = std::env::var("SPACES_ENV_WARN_DEPRECATED")
        && warn_deprecation == "0.16"
    {
        let module = module.unwrap_or("unknown".into());
        push_deferred_warning(format!("{module} => {warning}").into());
    }
}

pub fn get_deferred_warnings() -> Vec<Arc<str>> {
    let state = get_deferred_warnings_state().read();
    state.clone()
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
