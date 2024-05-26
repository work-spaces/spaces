

mod arguments;
mod git;
mod manifest;
mod workspace;
mod config;
mod archive;


fn main() -> anyhow::Result<()> {
    arguments::execute()
}
