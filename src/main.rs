

mod arguments;
mod git;
mod manifest;
mod workspace;
mod context;
mod archive;


fn main() -> anyhow::Result<()> {
    arguments::execute()
}
