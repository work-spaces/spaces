

mod arguments;
mod action;
mod git;
mod manifest;
mod ledger;
mod workspace;
mod context;
mod archive;


fn main() -> anyhow::Result<()> {
    arguments::execute()
}
