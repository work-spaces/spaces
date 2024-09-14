use crate::rules;

use anyhow::Context;

use anyhow_source_location::{format_context, format_error};
use starlark::environment::{FrozenModule, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::{AstModule, Dialect};

fn evaluate_module(
    workspace_path: &str,
    name: &str,
    content: String,
) -> anyhow::Result<FrozenModule> {

    {
        let mut state = rules::get_state().write().unwrap();
        state.latest_starlark_module = Some(workspace_path.to_string());
    }

    let ast =
        AstModule::parse(name, content, &Dialect::Standard).map_err(|e| format_error!("{e:?}"))?;

    // We can get the loaded modules from `ast.loads`.
    // And ultimately produce a `loader` capable of giving those modules to Starlark.
    let mut loads = Vec::new();
    for load in ast.loads() {
        // load the module using module_id
        let module_path = format!("{workspace_path}/{}", load.module_id);
        let contents = std::fs::read_to_string(module_path.as_str())
            .context(format_context!("Failed to read file {}", module_path))?;

        loads.push((
            load.module_id.to_owned(),
            evaluate_module(workspace_path, load.module_id, contents)?,
        ));
    }
    let modules = loads.iter().map(|(a, b)| (a.as_str(), b)).collect();
    let mut loader = ReturnFileLoader { modules: &modules };

    let globals = GlobalsBuilder::standard()
        .with(starstd::globals)
        .with_struct("checkout", rules::checkout::globals)
        .with_struct("run", rules::run::globals)
        .with_struct("fs", starstd::fs::globals)
        .with_struct("info", crate::info::globals)
        .build();

    let module = Module::new();
    {
        let mut eval = Evaluator::new(&module);
        eval.set_loader(&mut loader);
        eval.eval_module(ast, &globals)
            .map_err(|e| format_error!("{e:?}"))?;
    }
    // After creating a module we freeze it, preventing further mutation.
    // It can now be used as the input for other Starlark modules.
    Ok(module.freeze()?)
}

pub fn run_starlark_file(path: &str, phase: rules::Phase, target: Option<String>) -> anyhow::Result<()> {
    let content =
        std::fs::read_to_string(path).context(format_context!("Failed to read file {}", path))?;
    run_starlark_script(path, content, phase, target)
}

pub fn run_starlark_script(name: &str, content: String, phase: rules::Phase, target: Option<String>) -> anyhow::Result<()> {
    let mut printer = printer::Printer::new_stdout();

    let mut module_queue = std::collections::VecDeque::new();
    module_queue.push_back((name.to_string(), content));

    while !module_queue.is_empty() {

        while !module_queue.is_empty() {
            if let Some((name, content)) = module_queue.pop_front() {
                let _ = evaluate_module(".", name.as_str(), content)
                    .context(format_context!("Failed to evaluate module {}", name))?;  
            }
        }

        let mut state = rules::get_state().write().unwrap();

        state
            .sort_tasks(target.clone())
            .context(format_context!("Failed to sort tasks"))?;

        let new_modules = state
            .execute(&mut printer, phase)
            .context(format_context!("Failed to execute tasks"))?;


        for module in new_modules {
            let content = std::fs::read_to_string(module.as_str())
                .context(format_context!("Failed to read file {module}"))?;
            module_queue.push_back((module, content));
        }
    }

    if phase == rules::Phase::Checkout {
        let mut state = rules::get_state().write().unwrap();
        state
            .execute(&mut printer, rules::Phase::PostCheckout)
            .context(format_context!("failed to execute post checkout phase"))?;

        crate::executor::env::finalize_env().context(format_context!("failed to finalize env"))?;
    }

    Ok(())
}
