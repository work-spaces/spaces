use crate::{executor, info, rules, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use printer::Level;
use starlark::environment::{FrozenModule, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::{AstModule, Dialect};
use std::collections::HashSet;

fn evaluate_module(
    workspace_path: &str,
    name: &str,
    content: String,
) -> anyhow::Result<FrozenModule> {
    {
        let mut state = rules::get_state().write().unwrap();
        if name.ends_with(workspace::SPACES_MODULE_NAME) {
            state.latest_starlark_module = Some(name.to_string());
            state.all_modules.insert(name.to_string());
        }
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
    let loader = ReturnFileLoader { modules: &modules };

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
        eval.set_loader(&loader);
        eval.eval_module(ast, &globals)
            .map_err(|e| format_error!("{e:?}"))?;
    }
    // After creating a module we freeze it, preventing further mutation.
    // It can now be used as the input for other Starlark modules.
    module.freeze()
}

pub fn sort_tasks(target: Option<String>) -> anyhow::Result<()> {
    let mut state = rules::get_state().write().unwrap();
    state.sort_tasks(target)
}

pub fn run_starlark_modules(
    printer: &mut printer::Printer,
    modules: Vec<(String, String)>,
    phase: rules::Phase,
    target: Option<String>,
) -> anyhow::Result<()> {
    let workspace_path = workspace::absolute_path();

    let mut module_queue = std::collections::VecDeque::new();
    module_queue.extend(modules);
    let mut known_modules = HashSet::new();

    for (_, content) in module_queue.iter() {
        known_modules.insert(blake3::hash(content.as_bytes()).to_string());
    }

    while !module_queue.is_empty() {
        while !module_queue.is_empty() {
            if let Some((name, content)) = module_queue.pop_front() {
                printer.log(Level::Trace, format!("Evaluating module {}", name).as_str())?;
                let _ = evaluate_module(workspace_path.as_str(), name.as_str(), content)
                    .context(format_context!("Failed to evaluate module {}", name))?;
            }
        }

        if phase == rules::Phase::Checkout {
            sort_tasks(None).context(format_context!("Failed to sort tasks"))?;

            let state = rules::get_state().read().unwrap();
            let task_result = state
                .execute(printer, phase)
                .context(format_context!("Failed to execute tasks"))?;
            if !task_result.new_modules.is_empty() {
                printer.log(
                    Level::Trace,
                    format!("New Modules:{:?}", task_result.new_modules).as_str(),
                )?;
            }

            for module in task_result.new_modules {
                let path_to_module = format!("{}/{}", workspace_path, module);
                let content = std::fs::read_to_string(path_to_module.as_str())
                    .context(format_context!("Failed to read file {path_to_module}"))?;
                let hash = blake3::hash(content.as_bytes()).to_string();
                if !known_modules.contains(&hash) {
                    known_modules.insert(hash);
                    module_queue.push_back((module, content));
                }
            }
        }
    }

    match phase {
        rules::Phase::Run => {
            printer.log(Level::Trace, "Run Phase")?;
            sort_tasks(target.clone()).context(format_context!("Failed to sort tasks"))?;

            let state = rules::get_state().read().unwrap();
            let _new_modules = state
                .execute(printer, phase)
                .context(format_context!("Failed to execute tasks"))?;
        }
        rules::Phase::Evaluate => {
            printer.log(Level::Trace, "Evaluate Phase")?;

            sort_tasks(target.clone()).context(format_context!("Failed to sort tasks"))?;

            let state = rules::get_state().read().unwrap();
            state
                .show_tasks(printer)
                .context(format_context!("Failed to show tasks"))?;
        }
        rules::Phase::Checkout => {
            printer.log(Level::Trace, "Checkout Phase")?;

            sort_tasks(target.clone())
                .context(format_context!("Failed to sort tasks"))?;

            let state = rules::get_state().read().unwrap();
            state
                .execute(printer, rules::Phase::PostCheckout)
                .context(format_context!("failed to execute post checkout phase"))?;

            executor::env::finalize_env().context(format_context!("failed to finalize env"))?;

            let mut workspace_file_content = String::new();
            workspace_file_content.push_str(workspace::WORKSPACE_FILE_HEADER);
            workspace_file_content.push('\n');

            workspace_file_content.push_str("workspace_env = ");
            workspace_file_content
                .push_str(serde_json::to_string_pretty(&info::get_env())?.as_str());
            workspace_file_content.push_str("\n\ninfo.set_env(env = workspace_env) \n");

            let workspace_file_path =
                format!("{workspace_path}/{}", workspace::WORKSPACE_FILE_NAME);
            std::fs::write(workspace_file_path.as_str(), workspace_file_content)
                .context(format_context!("Failed to write workspace file"))?;
        }
        _ => {}
    }

    let io_path = workspace::get_io_path();

    {
        let io_state = rules::io::get_state().read().unwrap();
        io_state
            .io
            .save(io_path)
            .context(format_context!("Failed to save io"))?;
    }

    Ok(())
}
