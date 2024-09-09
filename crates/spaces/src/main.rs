#[macro_use]
extern crate starlark;

mod arguments;
mod ledger;
mod rules;
mod evaluator;
mod info;
mod executor;


fn main() -> anyhow::Result<()> {
    arguments::execute()
}
