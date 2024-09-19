use crate::{executor, info, rules, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
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

pub fn run_starlark_file(
    printer: &mut printer::Printer,
    path: &str,
    phase: rules::Phase,
    target: Option<String>,
) -> anyhow::Result<()> {
    let content =
        std::fs::read_to_string(path).context(format_context!("Failed to read file {}", path))?;
    run_starlark_modules(printer, vec![(path.to_string(), content)], phase, target)
}

pub fn run_starlark_modules(
    printer: &mut printer::Printer,
    modules: Vec<(String, String)>,
    phase: rules::Phase,
    target: Option<String>,
) -> anyhow::Result<()> {
    let workspace_path = workspace::get_workspace_path().context(format_context!(
        "Internal Error: Failed to get workspace path"
    ))?;

    let mut module_queue = std::collections::VecDeque::new();
    module_queue.extend(modules);
    let mut known_modules = HashSet::new();

    for (_, content) in module_queue.iter() {
        known_modules.insert(blake3::hash(content.as_bytes()).to_string());
    }

    while !module_queue.is_empty() {
        while !module_queue.is_empty() {
            if let Some((name, content)) = module_queue.pop_front() {
                let _ = evaluate_module(workspace_path.as_str(), name.as_str(), content)
                    .context(format_context!("Failed to evaluate module {}", name))?;
            }
        }

        let mut state = rules::get_state().write().unwrap();

        if phase == rules::Phase::Checkout {
            state
                .sort_tasks(target.clone())
                .context(format_context!("Failed to sort tasks"))?;

            let new_modules = state
                .execute(printer, phase)
                .context(format_context!("Failed to execute tasks"))?;

            for module in new_modules {
                let content = std::fs::read_to_string(module.as_str())
                    .context(format_context!("Failed to read file {module}"))?;
                let hash = blake3::hash(content.as_bytes()).to_string();
                if !known_modules.contains(&hash) {
                    known_modules.insert(hash);
                    module_queue.push_back((module, content));
                }
            }
        }
    }

    let mut state = rules::get_state().write().unwrap();
    match phase {
        rules::Phase::Run => {
            state
                .sort_tasks(target.clone())
                .context(format_context!("Failed to sort tasks"))?;

            let _new_modules = state
                .execute(printer, phase)
                .context(format_context!("Failed to execute tasks"))?;
        }
        rules::Phase::Evaluate => {
            state
                .sort_tasks(target.clone())
                .context(format_context!("Failed to sort tasks"))?;

            printer.info("Evaluate", &state.tasks)?;
        }
        rules::Phase::Checkout => {
            state
                .sort_tasks(target.clone())
                .context(format_context!("Failed to sort tasks"))?;

            state
                .execute(printer, rules::Phase::PostCheckout)
                .context(format_context!("failed to execute post checkout phase"))?;

            executor::env::finalize_env().context(format_context!("failed to finalize env"))?;

            let mut workspace_file_content = String::new();
            workspace_file_content.push_str(workspace::WORKSPACE_FILE_HEADER);
            workspace_file_content.push_str("\n");

            workspace_file_content.push_str("workspace_env = ");
            workspace_file_content
                .push_str(format!("{}", serde_json::to_string_pretty(&info::get_env())?).as_str());
            workspace_file_content.push_str("\n\ninfo.set_env(env = workspace_env) \n");

            let workspace_file_path =
                format!("{workspace_path}/{}", workspace::WORKSPACE_FILE_NAME);
            std::fs::write(workspace_file_path.as_str(), workspace_file_content)
                .context(format_context!("Failed to write workspace file"))?;
        }
        _ => {}
    }

    let io_path = workspace::get_workspace_io_path().context(format_context!(
        "Internal Error: Failed to get workspace io path"
    ))?;

    {
        let io_state = rules::io::get_state().read().unwrap();
        io_state
            .io
            .save(io_path.as_str())
            .context(format_context!("Failed to save io"))?;
    }

    Ok(())
}

pub fn run_starlark_workspace(
    printer: &mut printer::Printer,
    phase: rules::Phase,
    target: Option<String>,
) -> anyhow::Result<()> {
    let current_working_directory = std::env::current_dir()
        .context(format_context!("Failed to get current working directory"))?
        .to_string_lossy()
        .to_string();

    let workspace = {
        let mut multi_progress = printer::MultiProgress::new(printer);
        let progress = multi_progress.add_progress("loading workspace", Some(100), None);
        workspace::Workspace::new(progress, current_working_directory.as_str())
            .context(format_context!("while running workspace"))?
    };

    let (workspace_path, modules) = (workspace.absolute_path, workspace.modules);
    info::set_workspace_path(workspace_path)
        .context(format_context!("while setting workspace path"))?;

    run_starlark_modules(printer, modules, phase, target)
}
