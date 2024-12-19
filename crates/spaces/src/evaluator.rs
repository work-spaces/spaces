use crate::{builtins, rules, singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::{FrozenModule, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::{AstModule, Dialect};
use std::collections::HashSet;

#[derive(Debug)]
struct State {}

static STATE: state::InitCell<state_lock::StateLock<State>> = state::InitCell::new();

fn get_state() -> &'static state_lock::StateLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(state_lock::StateLock::new(State {}));

    STATE.get()
}

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
    if workspace::is_rules_module(name) {
        rules::set_latest_starlark_module(name);
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
        .with_struct("info", builtins::info::globals);

    let globals_builder = if with_rules == WithRules::Yes {
        globals_builder
            .with_struct("checkout", builtins::checkout::globals)
            .with_struct("run", builtins::run::globals)
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

pub fn run_starlark_modules(
    printer: &mut printer::Printer,
    workspace: workspace::WorkspaceArc,
    modules: Vec<(String, String)>,
    phase: rules::Phase,
    target: Option<String>,
) -> anyhow::Result<()> {
    let workspace_path = workspace.read().absolute_path.to_owned();
    let mut known_modules = HashSet::new();

    for (_, content) in modules.iter() {
        let hash = blake3::hash(content.as_bytes()).to_string();
        if !known_modules.contains(&hash) {
            known_modules.insert(hash);
        }
    }

    let mut module_queue = std::collections::VecDeque::new();
    module_queue.extend(modules);

    printer.log(
        printer::Level::Trace,
        format!("Input module queue:{module_queue:?}").as_str(),
    )?;

    // All modules are evaulated in this loop
    // During checkout additional modules may be added to the queue
    // For Run mode, the env module is processed first and available
    // to subsequent modules
    while !module_queue.is_empty() {
        if let Some((name, content)) = module_queue.pop_front() {
            let mut _workspace_lock = get_state().write();
            singleton::set_active_workspace(workspace.clone());
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
            rules::sort_tasks(None, phase).context(format_context!("Failed to sort tasks"))?;
            printer.log(printer::Level::Debug, "--Checkout Phase--")?;
            rules::debug_sorted_tasks(printer, phase)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let task_result = rules::execute(printer, workspace.clone(), phase)
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

            let is_reproducible = workspace.read().is_reproducible();
            printer.log(
                if is_reproducible {
                    printer::Level::Message
                } else {
                    printer::Level::Info
                },
                format!(
                    "Is Workspace reproducible: {is_reproducible} -> {}",
                    workspace.read().digest
                )
                .as_str(),
            )?;

            rules::sort_tasks(target.clone(), phase)
                .context(format_context!("Failed to sort tasks"))?;

            rules::debug_sorted_tasks(printer, phase)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let _new_modules = rules::execute(printer, workspace.clone(), phase)
                .context(format_context!("Failed to execute tasks"))?;
        }
        rules::Phase::Evaluate => {
            printer.log(printer::Level::Debug, "--Evaluate Phase--")?;
            rules::sort_tasks(target.clone(), phase)
                .context(format_context!("Failed to sort tasks"))?;

            rules::debug_sorted_tasks(printer, rules::Phase::Run)
                .context(format_context!("Failed to debug sorted tasks"))?;

            rules::show_tasks(printer).context(format_context!("Failed to show tasks"))?;
        }
        rules::Phase::Checkout => {
            printer.log(printer::Level::Debug, "--Post Checkout Phase--")?;

            // at this point everything should be preset, sort tasks as if in run phase
            rules::sort_tasks(target.clone(), rules::Phase::Run)
                .context(format_context!("Failed to sort tasks"))?;
            rules::debug_sorted_tasks(printer, rules::Phase::PostCheckout)
                .context(format_context!("Failed to debug sorted tasks"))?;

            rules::execute(printer, workspace.clone(), rules::Phase::PostCheckout)
                .context(format_context!("failed to execute post checkout phase"))?;

            // prepend PATH with sysroot/bin if sysroot/bin is not already in the PATH
            let mut env = workspace.read().get_env();
            let sysroot_bin = format!("{}/sysroot/bin", workspace.read().absolute_path);
            if !env.paths.contains(&sysroot_bin) {
                env.paths.insert(0, sysroot_bin);
            }

            if workspace.read().is_reproducible() {
                env.vars.insert(
                    workspace::SPACES_ENV_WORKSPACE_DIGEST.to_string(),
                    workspace.read().digest.clone(),
                );
            }

            let absolute_path = workspace.read().absolute_path.clone();
            let workspace_path = std::path::Path::new(&absolute_path);
            let env_path = workspace_path.join("env");
            env.create_shell_env(env_path)
                .context(format_context!("failed to finalize env"))?;
            let env_str = serde_json::to_string_pretty(&env)?;

            workspace
                .read()
                .save_env_file(env_str.as_str())
                .context(format_context!("Failed to save env file"))?;
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
