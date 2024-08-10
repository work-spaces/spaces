mod arguments;
mod git;
mod template;
mod manifest;
mod ledger;
mod workspace;
mod context;
mod archive;
mod platform;


fn main() -> anyhow::Result<()> {
    arguments::execute()
}
