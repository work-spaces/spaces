#[macro_use]
extern crate starlark;

mod arguments;
mod docs;
mod evaluator;
mod executor;
mod builtins;
mod label;
#[cfg(feature = "lsp")]
mod lsp_context;
mod rule;
mod rules;
mod task;
mod tools;
mod runner;
mod workspace;
mod singleton;

fn main() -> anyhow::Result<()> {
    match arguments::execute() {
        Ok(_) => Ok(()),    
        Err(error) => {
            singleton::process_anyhow_error(error);
            singleton::show_error_chain();
            Err(anyhow::anyhow!("Execution Failed"))
        }
    }
}
