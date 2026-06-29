#[macro_use]
extern crate starlark;

mod about;
mod arguments;
mod builtins;
mod co;
mod completions;
mod docs;
mod evaluation_profile;
mod evaluator;
mod executor;
mod label;
mod lsp_context;
mod prelude;
mod rules;
mod runner;
mod singleton;
mod stardoc;
mod sync;
mod task;
mod tools;
mod workspace;

fn main() {
    if arguments::execute().is_err() {
        std::process::exit(1);
    }
}
