#[macro_use]
extern crate starlark;

mod arguments;
mod docs;
mod evaluator;
mod environment;
mod executor;
mod builtins;
mod label;
mod ledger;
mod rules;
mod tools;
mod workspace;

fn main() -> anyhow::Result<()> {
    arguments::execute()
}
