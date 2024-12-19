#[macro_use]
extern crate starlark;

mod arguments;
mod docs;
mod evaluator;
mod executor;
mod builtins;
mod label;
mod inputs;
mod rules;
mod tools;
mod runner;
mod workspace;
mod singleton;

fn main() -> anyhow::Result<()> {
    arguments::execute()
}
