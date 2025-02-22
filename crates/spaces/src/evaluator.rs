use crate::{builtins, executor, rule, rules, singleton, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::{FrozenModule, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::{AstModule, Dialect};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug)]
struct State {}

static STATE: state::InitCell<lock::StateLock<State>> = state::InitCell::new();

fn get_state() -> &'static lock::StateLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(lock::StateLock::new(State {}));
    STATE.get()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WithRules {
    No,
    Yes,
}

pub fn get_dialect() -> Dialect {
    Dialect {
        enable_top_level_stmt: true,
        ..Default::default()
    }
}

pub fn get_globals(with_rules: WithRules) -> GlobalsBuilder {
    let mut builder = GlobalsBuilder::standard()
        .with(starstd::globals)
        .with_namespace("fs", starstd::fs::globals)
        .with_namespace("json", starstd::json::globals)
        .with_namespace("hash", starstd::hash::globals)
        .with_namespace("process", starstd::process::globals)
        .with_namespace("script", starstd::script::globals)
        .with_namespace("time", starstd::time::globals)
        .with_namespace("info", builtins::info::globals);

    if with_rules == WithRules::Yes {
        builder = builder
            .with_namespace("workspace", builtins::workspace::globals)
            .with_namespace("checkout", builtins::checkout::globals)
            .with_namespace("run", builtins::run::globals);
    }

    builder
}

pub fn evaluate_loads(
    ast: &AstModule,
    name: Arc<str>,
    workspace_path: Arc<str>,
    with_rules: WithRules,
) -> anyhow::Result<Vec<(String, FrozenModule)>> {
    // We can get the loaded modules from `ast.loads`.
    // And ultimately produce a `loader` capable of giving those modules to Starlark.
    let mut loads = Vec::new();
    for load in ast.loads() {
        let module_load_path =
            workspace::get_workspace_path(workspace_path.as_ref(), name.as_ref(), load.module_id);
        if module_load_path.ends_with(workspace::SPACES_MODULE_NAME) {
            return Err(format_error!("Error: Attempting to load module ending with `spaces.star` module. This is a reserved module name."));
        }
        let contents = std::fs::read_to_string(module_load_path.as_ref()).with_context(|| {
            format_context!(
                "error: failed to load {}\n--> {name}:{}\n in workspace `{workspace_path}`",
                load.module_id,
                load.span.file.find_line(load.span.span.begin()) + 1,
            )
        })?;

        loads.push((
            load.module_id.to_owned(),
            evaluate_module(
                workspace_path.clone(),
                module_load_path.clone(),
                contents,
                with_rules,
            )?,
        ));
    }
    Ok(loads)
}

pub fn evaluate_ast(
    ast: AstModule,
    name: Arc<str>,
    workspace_path: Arc<str>,
    with_rules: WithRules,
) -> anyhow::Result<Module> {
    let loads = evaluate_loads(&ast, name.clone(), workspace_path.clone(), with_rules)
        .context(format_context!("Failed to process loads"))?;
    let modules = loads.iter().map(|(a, b)| (a.as_str(), b)).collect();
    let loader = ReturnFileLoader { modules: &modules };

    let globals_builder = get_globals(with_rules);
    let globals = globals_builder.build();
    let module = Module::new();
    {
        let mut eval = Evaluator::new(&module);
        eval.set_loader(&loader);
        eval.eval_module(ast, &globals)
            .map_err(|e| format_error!("{e:?}"))?;
    }

    Ok(module)
}

pub fn evaluate_module(
    workspace_path: Arc<str>,
    name: Arc<str>,
    content: String,
    with_rules: WithRules,
) -> anyhow::Result<FrozenModule> {
    if workspace::is_rules_module(name.as_ref()) {
        rules::set_latest_starlark_module(name.clone());
    }

    let dialect = get_dialect();
    let ast =
        AstModule::parse(name.as_ref(), content, &dialect).map_err(|e| format_error!("{e:?}"))?;
    let module = evaluate_ast(ast, name, workspace_path, with_rules)?;
    Ok(module.freeze()?)

    /*
    // We can get the loaded modules from `ast.loads`.
    // And ultimately produce a `loader` capable of giving those modules to Starlark.
    let loads = process_loads(&ast, name.clone(), workspace_path.clone(), with_rules)
        .context(format_context!("Failed to process loads"))?;
    let modules = loads.iter().map(|(a, b)| (a.as_str(), b)).collect();
    let loader = ReturnFileLoader { modules: &modules };

    let globals_builder = get_globals(with_rules);
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
    Ok(module.freeze()?)
    */
}

fn star_logger(printer: &mut printer::Printer) -> logger::Logger {
    logger::Logger::new_printer(printer, "starlark".into())
}

fn insert_setup_and_all_rules(target: Option<Arc<str>>) -> anyhow::Result<Option<Arc<str>>> {
    // insert the //:setup rule

    rules::add_setup_dep_to_run_rules()
        .context(format_context!("Failed to add setup dep to run rules"))?;

    let setup_rule = rule::Rule {
        name: "setup".into(),
        help: Some("Builtin rule to run setup rules first".into()),
        inputs: None,
        outputs: None,
        type_: Some(rule::RuleType::Run),
        platforms: None,
        deps: Some(rules::get_setup_rules()),
    };

    rules::insert_task(task::Task::new(
        setup_rule,
        task::Phase::Run,
        executor::Task::Target,
    ))
    .context(format_context!("Failed to insert task `all`"))?;

    if target.is_none() {
        let mut deps: Vec<Arc<str>> = Vec::new();
        for all_target in singleton::get_run_all().iter() {
            deps.push(all_target.clone());
        }

        deps.push(rule::SETUP_RULE_NAME.into());

        let rule = rule::Rule {
            name: "all".into(),
            help: Some("Builtin rule to run default targets and dependencies".into()),
            inputs: None,
            outputs: None,
            type_: Some(rule::RuleType::Run),
            platforms: None,
            deps: Some(deps),
        };

        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Run,
            executor::Task::Target,
        ))
        .context(format_context!("Failed to insert task `all`"))?;

        Ok(Some(rule::ALL_RULE_NAME.into()))
    } else {
        Ok(target)
    }
}

fn show_eval_progress(
    printer: &mut printer::Printer,
    name: &str,
    handle: std::thread::JoinHandle<anyhow::Result<()>>,
) -> anyhow::Result<()> {
    // show a progress bar if the evaluation takes more than 100ms
    let mut multi_progress = printer::MultiProgress::new(printer);
    let mut progress_bar = None;
    let mut count = 0;
    loop {
        if handle.is_finished() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        count += 1;
        if count == 10 {
            progress_bar = Some(multi_progress.add_progress(
                "evaluating",
                Some(200),
                Some(format!("Complete ({})", name).as_str()),
            ));
            progress_bar.as_mut().unwrap().set_message(name);
        }
        if let Some(progress_bar) = progress_bar.as_mut() {
            progress_bar.increment_with_overflow(1);
        }
    }

    match handle.join() {
        Ok(result) => {
            result.context(format_context!("Failed to evaluate module {}", name))?;
        }
        Err(_) => {
            return Err(format_error!("Failed to evaluate module {name}"));
        }
    }
    Ok(())
}

pub fn run_starlark_modules(
    printer: &mut printer::Printer,
    workspace: workspace::WorkspaceArc,
    modules: Vec<(Arc<str>, Arc<str>)>,
    phase: task::Phase,
    target: Option<Arc<str>>,
) -> anyhow::Result<()> {
    star_logger(printer).message("--Run Starlark Modules--");
    let workspace_path = workspace.read().absolute_path.to_owned();
    let mut known_modules = HashSet::new();

    star_logger(printer).debug("Collect Known Modules");
    for (_, content) in modules.iter() {
        let hash = blake3::hash(content.as_bytes()).to_string();
        if !known_modules.contains(&hash) {
            known_modules.insert(hash);
        }
    }

    if phase == task::Phase::Checkout {
        workspace.write().clear_members();
    }

    let mut module_queue = std::collections::VecDeque::new();
    module_queue.extend(modules);

    star_logger(printer).trace(format!("Input module queue:{module_queue:?}").as_str());

    // All modules are evaulated in this loop
    // During checkout additional modules may be added to the queue
    // For Run mode, the env module is processed first and available
    // to subsequent modules
    while !module_queue.is_empty() {
        if let Some((name, content)) = module_queue.pop_front() {
            let mut _workspace_lock = get_state().write();
            singleton::set_active_workspace(workspace.clone());
            star_logger(printer).message(format!("evaluating {}", name).as_str());

            let eval_workspace_path = workspace_path.clone();
            let eval_name = name.clone();

            let handle = std::thread::spawn(move || -> anyhow::Result<()> {
                let _ = evaluate_module(
                    eval_workspace_path.clone(),
                    eval_name.clone(),
                    content.to_string(),
                    WithRules::Yes,
                )
                .context(format_context!("Failed to evaluate module {}", eval_name))?;
                Ok(())
            });

            show_eval_progress(printer, &name, handle)
                .context(format_context!("Failed to show eval progress"))?;
        }

        // During checkout phase, additional modules may be added to the queue
        // if the repo contains more spaces.star files
        if phase == task::Phase::Checkout {
            rules::sort_tasks(printer, workspace.clone(), None, phase)
                .context(format_context!("Failed to sort tasks"))?;

            star_logger(printer).debug("--Checkout Phase--");
            rules::debug_sorted_tasks(printer, phase)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let task_result = rules::execute(printer, workspace.clone(), phase)
                .context(format_context!("Failed to execute tasks"))?;
            if !task_result.new_modules.is_empty() {
                star_logger(printer)
                    .trace(format!("New Modules:{:?}", task_result.new_modules).as_str());
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
                    module_queue.push_front((module, content.into()));
                }
            }
        }
    }
    rules::set_latest_starlark_module("".into());

    star_logger(printer).debug("Inserting //:setup and //:all");
    let run_target = insert_setup_and_all_rules(target.clone())
        .context(format_context!("failed to insert run all"))?;

    match phase {
        task::Phase::Run => {
            star_logger(printer).message("--Run Phase--");

            let is_reproducible = workspace.read().is_reproducible();
            let repro_message = format!(
                "Is Workspace reproducible: {is_reproducible} -> {}",
                workspace.read().digest
            );
            if is_reproducible {
                star_logger(printer).message(repro_message.as_str());
            } else {
                star_logger(printer).info(repro_message.as_str());
            }

            rules::sort_tasks(printer, workspace.clone(), run_target.clone(), phase)
                .context(format_context!("Failed to sort tasks"))?;

            rules::debug_sorted_tasks(printer, phase)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let _new_modules = rules::execute(printer, workspace.clone(), phase)
                .context(format_context!("Failed to execute tasks"))?;
        }
        task::Phase::Inspect => {
            star_logger(printer).message("--Inspect Phase--");

            rules::sort_tasks(printer, workspace.clone(), target.clone(), phase)
                .context(format_context!("Failed to sort tasks"))?;

            rules::debug_sorted_tasks(printer, task::Phase::Checkout)
                .context(format_context!("Failed to debug sorted tasks"))?;
            rules::debug_sorted_tasks(printer, task::Phase::PostCheckout)
                .context(format_context!("Failed to debug sorted tasks"))?;
            rules::debug_sorted_tasks(printer, task::Phase::Run)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let inspect_globs = singleton::get_inspect_globs();

            // if not filters and called from a relative path, filter on the relative path
            let mut globs = inspect_globs;
            let relative_path = workspace.read().relative_invoked_path.clone();
            let mut strip_prefix = None;
            if globs.is_empty() && !relative_path.is_empty() {
                globs.insert(format!("+{}**", relative_path).into());
                strip_prefix = Some(format!("//{}", relative_path).into());
            }

            //only show checkout if log level is message or higher
            if printer.verbosity.level <= printer::Level::Message {
                rules::show_tasks(
                    printer,
                    task::Phase::Checkout,
                    target.clone(),
                    &globs,
                    strip_prefix.clone(),
                )
                .context(format_context!("Failed to show tasks"))?;
            }
            rules::show_tasks(
                printer,
                task::Phase::Run,
                target.clone(),
                &globs,
                strip_prefix,
            )
            .context(format_context!("Failed to show tasks"))?;
        }
        task::Phase::Checkout => {
            star_logger(printer).message("--Post Checkout Phase--");

            // at this point everything should be set, sort tasks as if in run phase
            rules::sort_tasks(printer, workspace.clone(), None, task::Phase::Run)
                .context(format_context!("Failed to sort tasks"))?;
            rules::debug_sorted_tasks(printer, task::Phase::PostCheckout)
                .context(format_context!("Failed to debug sorted tasks"))?;

            rules::execute(printer, workspace.clone(), task::Phase::PostCheckout)
                .context(format_context!("failed to execute post checkout phase"))?;

            // prepend PATH with sysroot/bin if sysroot/bin is not already in the PATH
            let mut env = workspace.read().get_env();
            let sysroot_bin: Arc<str> =
                format!("{}/sysroot/bin", workspace.read().absolute_path).into();
            if !env.paths.contains(&sysroot_bin) {
                env.paths.insert(0, sysroot_bin);
            }

            if workspace.read().is_reproducible() {
                env.vars.insert(
                    workspace::SPACES_ENV_WORKSPACE_DIGEST.into(),
                    workspace.read().digest.clone(),
                );
            }

            let absolute_path = workspace.read().get_absolute_path();
            let workspace_path = std::path::Path::new(absolute_path.as_ref());
            let env_path = workspace_path.join("env");
            env.create_shell_env(env_path)
                .context(format_context!("failed to finalize env"))?;
            let env_str = serde_json::to_string_pretty(&env)?;

            star_logger(printer).debug("saving workspace env");
            workspace
                .read()
                .save_env_file(env_str.as_str())
                .context(format_context!("Failed to save env file"))?;
        }
        _ => {}
    }

    if phase == task::Phase::Checkout || singleton::get_is_rescan() || workspace.read().is_dirty {
        star_logger(printer).debug("saving workspace setings");
        workspace
            .read()
            .save_settings()
            .context(format_context!("Failed to save settings"))?;
    }

    Ok(())
}

pub fn run_starlark_script(name: Arc<str>, script: Arc<str>) -> anyhow::Result<()> {
    // load SPACES_WORKSPACE from env
    let workspace = std::env::var(ws::SPACES_WORKSPACE_ENV_VAR)
        .unwrap_or(".".to_string())
        .into();

    evaluate_module(workspace, name.clone(), script.to_string(), WithRules::No)
        .context(format_context!("Failed to evaluate module {}", name))?;

    Ok(())
}
