#[macro_use]
extern crate starlark;

mod arguments;
mod docs;
mod evaluator;
mod executor;
mod builtins;
mod label;
mod ledger;
mod rules;
mod tools;
mod runner;
mod workspace;

fn main() -> anyhow::Result<()> {
    arguments::execute()
}
