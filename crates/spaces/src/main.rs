#[macro_use]
extern crate starlark;

mod arguments;
mod builtins;
mod co;
mod completions;
mod docs;
mod evaluator;
mod executor;
mod label;
mod lsp_context;
mod rules;
mod runner;
mod singleton;
mod stardoc;
mod task;
mod tools;
mod workspace;

fn main() -> anyhow::Result<()> {
    match arguments::execute() {
        Ok(_) => Ok(()),
        Err(error) => {
            if !singleton::has_rule_failure() {
                singleton::process_anyhow_error(error);
                singleton::show_error_chain();
                Err(anyhow::anyhow!("Execution Failed"))
            } else {
                let logs = singleton::get_logs_for_failed_rules();
                Err(anyhow::anyhow!(
                    "Rule(s) Execution Failed:\n  {}",
                    logs.join("\n  ")
                ))
            }
        }
    }
}
