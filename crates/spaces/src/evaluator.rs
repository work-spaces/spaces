use crate::{executor, info, rules, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::{FrozenModule, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::{AstModule, Dialect};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq)]
enum WithRules {
    No,
    Yes,
}

fn evaluate_module(
    workspace_path: &str,
    name: &str,
    content: String,
    with_rules: WithRules,
) -> anyhow::Result<FrozenModule> {
    {
        let mut state = rules::get_state().write().unwrap();
        if workspace::is_rules_module(name) {
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
        let module_load_path = workspace::get_workspace_path(workspace_path, name, load.module_id);
        if module_load_path.ends_with(workspace::SPACES_MODULE_NAME) {
            return Err(format_error!("Error: Attempting to load module ending with `spaces.star` module. This is a reserved module name."));
        }
        let contents = std::fs::read_to_string(module_load_path.as_str())
            .context(format_context!("Failed to read file {}", module_load_path))?;

        loads.push((
            load.module_id.to_owned(),
            evaluate_module(
                workspace_path,
                module_load_path.as_str(),
                contents,
                with_rules,
            )?,
        ));
    }
    let modules = loads.iter().map(|(a, b)| (a.as_str(), b)).collect();
    let loader = ReturnFileLoader { modules: &modules };

    let globals_builder = GlobalsBuilder::standard()
        .with(starstd::globals)
        .with_struct("fs", starstd::fs::globals)
        .with_struct("json", starstd::json::globals)
        .with_struct("hash", starstd::hash::globals)
        .with_struct("process", starstd::process::globals)
        .with_struct("script", starstd::script::globals)
        .with_struct("info", info::globals);

    let globals_builder = if with_rules == WithRules::Yes {
        globals_builder
            .with_struct("checkout", rules::checkout::globals)
            .with_struct("run", rules::run::globals)
    } else {
        globals_builder
    };

    let globals = globals_builder.build();

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

pub fn sort_tasks(target: Option<String>, phase: rules::Phase) -> anyhow::Result<()> {
    let mut state = rules::get_state().write().unwrap();
    state.sort_tasks(target, phase)
}

fn debug_sorted_tasks(printer: &mut printer::Printer, phase: rules::Phase) -> anyhow::Result<()> {
    let state: std::sync::RwLockReadGuard<'_, rules::State> = rules::get_state().read().unwrap();
    for node_index in state.sorted.iter() {
        let task_name = state.graph.get_task(*node_index);
        if let Some(task) = state.tasks.read().unwrap().get(task_name) {
            if task.phase == phase {
                printer.log(
                    printer::Level::Debug,
                    format!("Queued task {task_name}").as_str(),
                )?;
            }
        }
    }
    Ok(())
}

pub fn run_starlark_modules(
    printer: &mut printer::Printer,
    modules: Vec<(String, String)>,
    phase: rules::Phase,
    target: Option<String>,
) -> anyhow::Result<()> {
    let workspace_path = workspace::absolute_path();
    let mut known_modules = HashSet::new();

    let mut module_queue = std::collections::VecDeque::new();
    module_queue.extend(modules);

    info::set_phase(phase);

    // All modules are evaulated in this loop
    // During checkout additional modules may be added to the queue
    // For Run mode, the env module is processed first and available
    // to subsequent modules
    while !module_queue.is_empty() {
        if let Some((name, content)) = module_queue.pop_front() {
            printer.log(
                printer::Level::Trace,
                format!("Evaluating module {}", name).as_str(),
            )?;
            let _ = evaluate_module(
                workspace_path.as_str(),
                name.as_str(),
                content,
                WithRules::Yes,
            )
            .context(format_context!("Failed to evaluate module {}", name))?;
        }

        // During checkout phase, additional modules may be added to the queue
        // if the repo contains more spaces.star files
        if phase == rules::Phase::Checkout {
            sort_tasks(None, phase).context(format_context!("Failed to sort tasks"))?;
            printer.log(printer::Level::Debug, "--Checkout Phase--")?;
            debug_sorted_tasks(printer, phase)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let state = rules::get_state().read().unwrap();
            let task_result = state
                .execute(printer, phase)
                .context(format_context!("Failed to execute tasks"))?;
            if !task_result.new_modules.is_empty() {
                printer.log(
                    printer::Level::Trace,
                    format!("New Modules:{:?}", task_result.new_modules).as_str(),
                )?;
            }

            let mut new_modules = Vec::new();
            for module in task_result.new_modules {
                let path_to_module = format!("{}/{}", workspace_path, module);
                let content = std::fs::read_to_string(path_to_module.as_str())
                    .context(format_context!("Failed to read file {path_to_module}"))?;

                new_modules.push((module, content));
            }

            // sort new modules by the first item
            new_modules.sort_by(|first, second| first.0.cmp(&second.0));

            for (module, content) in new_modules {
                let hash = blake3::hash(content.as_bytes()).to_string();
                if !known_modules.contains(&hash) {
                    known_modules.insert(hash);
                    module_queue.push_front((module, content));
                }
            }
        }
    }

    match phase {
        rules::Phase::Run => {
            printer.log(printer::Level::Message, "--Run Phase--")?;

            let is_reproducible = info::is_reproducible();
            printer.log(
                if is_reproducible {
                    printer::Level::Message
                } else {
                    printer::Level::Info
                },
                format!("Is Workspace reproducible: {is_reproducible}").as_str(),
            )?;

            sort_tasks(target.clone(), phase).context(format_context!("Failed to sort tasks"))?;

            debug_sorted_tasks(printer, phase)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let state = rules::get_state().read().unwrap();
            let _new_modules = state
                .execute(printer, phase)
                .context(format_context!("Failed to execute tasks"))?;
        }
        rules::Phase::Evaluate => {
            printer.log(printer::Level::Debug, "--Evaluate Phase--")?;
            sort_tasks(target.clone(), phase).context(format_context!("Failed to sort tasks"))?;

            debug_sorted_tasks(printer, rules::Phase::Run)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let state = rules::get_state().read().unwrap();
            state
                .show_tasks(printer)
                .context(format_context!("Failed to show tasks"))?;
        }
        rules::Phase::Checkout => {
            printer.log(printer::Level::Debug, "--Post Checkout Phase--")?;

            // at this point everything should be preset, sort tasks as if in run phase
            sort_tasks(target.clone(), rules::Phase::Run)
                .context(format_context!("Failed to sort tasks"))?;
            debug_sorted_tasks(printer, rules::Phase::PostCheckout)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let state = rules::get_state().read().unwrap();
            state
                .execute(printer, rules::Phase::PostCheckout)
                .context(format_context!("failed to execute post checkout phase"))?;

            // prepend PATH with sysroot/bin if sysroot/bin is not already in the PATH
            let mut env = info::get_env();
            let sysroot_bin = format!("{}/sysroot/bin", workspace::absolute_path());
            if !env.paths.contains(&sysroot_bin) {
                env.paths.insert(0, sysroot_bin);
            }

            if info::is_reproducible() {
                env.vars.insert(
                    workspace::SPACES_ENV_WORKSPACE_DIGEST.to_string(),
                    workspace::get_digest(),
                );
            }

            executor::env::finalize_env(&env).context(format_context!("failed to finalize env"))?;

            let mut workspace_file_content = String::new();
            workspace_file_content.push_str(workspace::WORKSPACE_FILE_HEADER);
            workspace_file_content.push('\n');

            workspace_file_content.push_str("workspace_env = ");

            workspace_file_content.push_str(serde_json::to_string_pretty(&env)?.as_str());
            workspace_file_content.push_str("\n\ninfo.set_env(env = workspace_env) \n");

            let workspace_file_path = format!("{workspace_path}/{}", workspace::ENV_FILE_NAME);
            std::fs::write(workspace_file_path.as_str(), workspace_file_content)
                .context(format_context!("Failed to write workspace file"))?;
        }
        _ => {}
    }

    Ok(())
}

pub fn run_starlark_script(name: &str, script: &str) -> anyhow::Result<()> {
    // load SPACES_WORKSPACE from env
    let workspace = std::env::var("SPACES_WORKSPACE").unwrap_or(".".to_string());

    evaluate_module(workspace.as_str(), name, script.to_string(), WithRules::No)
        .context(format_context!("Failed to evaluate module {}", name))?;

    Ok(())
}
