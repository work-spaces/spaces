#[macro_use]
extern crate starlark;

pub mod args;
pub mod env;
pub mod fs;
pub mod hash;
pub mod json;
pub mod log;
pub mod path;
pub mod process;
pub mod script;
pub mod sh;
pub mod string;
pub mod sys;
pub mod text;
pub mod time;
pub mod tmp;
pub mod toml;
pub mod yaml;

use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

pub(crate) struct State {
    is_lsp_mode: AtomicBool,
}

static STATE: OnceLock<State> = OnceLock::new();

pub(crate) fn state() -> &'static State {
    STATE.get_or_init(|| State {
        is_lsp_mode: AtomicBool::new(false),
    })
}

pub(crate) fn is_lsp_mode() -> bool {
    state()
        .is_lsp_mode
        .load(std::sync::atomic::Ordering::Relaxed)
}

pub fn enable_lsp_mode() {
    state()
        .is_lsp_mode
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Prints a string to standard output, followed by a newline.
    ///
    /// ```python
    /// print("hello, world")
    /// ```
    fn print(content: &str) -> anyhow::Result<NoneType> {
        if !is_lsp_mode() {
            println!("{content}");
        }
        Ok(NoneType)
    }

    /// Prints a string to standard error, followed by a newline.
    ///
    /// ```python
    /// eprint("error: something went wrong")
    /// ```
    fn eprint(content: &str) -> anyhow::Result<NoneType> {
        if !is_lsp_mode() {
            eprintln!("{content}");
        }
        Ok(NoneType)
    }

    fn debug(content: starlark::values::Value) -> anyhow::Result<NoneType> {
        if !is_lsp_mode() {
            println!("{content}");
        }
        Ok(NoneType)
    }

    /// Exits the process with exit status 1 if condition is false.
    ///
    /// An optional `message` can be provided to describe the failure.
    /// When omitted, the error message defaults to `"assertion failed"`.
    ///
    /// ```python
    /// assert(1 + 1 == 2)
    /// assert(x > 0, "x must be positive")
    /// ```
    fn assert_on(condition: bool, message: Option<&str>) -> anyhow::Result<NoneType> {
        if !condition && !is_lsp_mode() {
            let msg = message.unwrap_or("assertion failed");
            eprintln!("{msg}");
            std::process::exit(1);
        }
        Ok(NoneType)
    }
}
