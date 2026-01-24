use crate::workspace::WorkspaceArc;
use crate::{builtins, executor, rules, singleton, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::{FrozenModule, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::{AstModule, Dialect};
use std::collections::HashSet;
use std::sync::Arc;
use utils::{lock, logger, rule, ws};

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IsExecuteTasks {
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
    workspace: Option<WorkspaceArc>,
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
                workspace.clone(),
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
    workspace: Option<WorkspaceArc>,
    workspace_path: Arc<str>,
    with_rules: WithRules,
) -> anyhow::Result<Module> {
    let loads = evaluate_loads(
        &ast,
        name.clone(),
        workspace.clone(),
        workspace_path,
        with_rules,
    )
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

    if let Some(workspace) = workspace {
        if singleton::get_inspect_stardoc_path().is_some() {
            let mut workspace = workspace.write();
            let doc_items: Vec<(Arc<str>, _)> = module
                .names()
                .filter_map(|function_name| {
                    let value = module.get(&function_name).unwrap();
                    // The signature is the full path to the function with the file path
                    // filter out values where the signature doesn't start with the module
                    // that is being processed. These are loaded from another module
                    // and don't belong in the docs for this module
                    let signature_starts_with_name = value
                        .parameters_spec()
                        .map(|spec| spec.signature())
                        .map(|signature| signature.starts_with(name.as_ref()))
                        .unwrap_or(false);
                    if signature_starts_with_name {
                        Some((function_name.as_str().into(), value.documentation()))
                    } else {
                        None
                    }
                })
                .collect();
            let relative_name = name
                .strip_prefix(format!("{}/", workspace.get_absolute_path()).as_str())
                .unwrap_or(name.as_ref());
            workspace.stardoc.insert(relative_name.into(), doc_items);
        }
    }
    Ok(module)
}

pub fn evaluate_module(
    workspace: Option<WorkspaceArc>,
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
    let module = evaluate_ast(ast, name, workspace, workspace_path, with_rules)?;
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

fn insert_setup_and_all_rules(
    workspace: workspace::WorkspaceArc,
    target: Option<Arc<str>>,
) -> anyhow::Result<Option<Arc<str>>> {
    // insert the //:setup rule

    rules::add_setup_dep_to_run_rules()
        .context(format_context!("Failed to add setup dep to run rules"))?;

    let setup_rule = rule::Rule {
        name: rule::SETUP_RULE_NAME.into(),
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
    .context(format_context!("Failed to insert task `setup`"))?;

    let mut deps: Vec<Arc<str>> = Vec::new();
    let all_deps = workspace.read().settings.bin.run_all.clone();
    for all_target in all_deps {
        deps.push(all_target.clone());
    }

    deps.push(rule::SETUP_RULE_NAME.into());

    let rule = rule::Rule {
        name: rule::ALL_RULE_NAME.into(),
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
    .context(format_context!("Failed to insert task `//:all`"))?;

    let test_rule = rule::Rule {
        name: rule::TEST_RULE_NAME.into(),
        help: Some("Builtin rule to run tests".into()),
        inputs: None,
        outputs: None,
        type_: Some(rule::RuleType::Test),
        platforms: None,
        deps: Some(rules::get_test_rules()),
    };

    rules::insert_task(task::Task::new(
        test_rule,
        task::Phase::Run,
        executor::Task::Target,
    ))
    .context(format_context!("Failed to insert task `//:test`"))?;

    let pre_commit_rule = rule::Rule {
        name: rule::PRE_COMMIT_RULE_NAME.into(),
        help: Some("Builtin rule to run pre-commit checks".into()),
        inputs: None,
        outputs: None,
        type_: Some(rule::RuleType::PreCommit),
        platforms: None,
        deps: Some(rules::get_pre_commit_rules()),
    };

    rules::insert_task(task::Task::new(
        pre_commit_rule,
        task::Phase::Run,
        executor::Task::Target,
    ))
    .context(format_context!("Failed to insert task `//:pre-commit`"))?;

    let clean_rule = rule::Rule {
        name: rule::CLEAN_RULE_NAME.into(),
        help: Some("Builtin rule to cleanup the workspace".into()),
        inputs: None,
        outputs: None,
        type_: Some(rule::RuleType::Clean),
        platforms: None,
        deps: Some(rules::get_clean_rules()),
    };

    rules::insert_task(task::Task::new(
        clean_rule,
        task::Phase::Run,
        executor::Task::Target,
    ))
    .context(format_context!("Failed to insert task `//:clean`"))?;

    if target.is_none() {
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
                Some(format!("Complete ({name})").as_str()),
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

pub fn evaluate_starlark_modules(
    printer: &mut printer::Printer,
    workspace: workspace::WorkspaceArc,
    modules: Vec<(Arc<str>, Arc<str>)>,
    phase: task::Phase,
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
            star_logger(printer).debug(format!("evaluating {name} from front of queue").as_str());

            let eval_name = name.clone();
            let workspace_arc = workspace.clone();
            let eval_workspace_path = workspace_path.clone();

            let handle = std::thread::spawn(move || -> anyhow::Result<()> {
                let _ = evaluate_module(
                    Some(workspace_arc),
                    eval_workspace_path,
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
            rules::update_depedency_graph(printer, None, phase)
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
                let path_to_module = format!("{workspace_path}/{module}");
                let content = std::fs::read_to_string(path_to_module.as_str())
                    .context(format_context!("Failed to read file {path_to_module}"))?;

                new_modules.push((module, content));
            }

            // sorts the modules lexicographically by the filename from back to front.
            // push_front below will execute the modules in lexicographical order.
            new_modules.sort_by(|first, second| second.0.cmp(&first.0));
            star_logger(printer).debug(
                format!(
                    "Adding new modules: {:?}",
                    new_modules
                        .iter()
                        .map(|(name, _content)| name)
                        .collect::<Vec<_>>()
                )
                .as_str(),
            );

            for (module, content) in new_modules {
                let hash = blake3::hash(content.as_bytes()).to_string();
                if !known_modules.contains(&hash) {
                    star_logger(printer)
                        .debug(format!("Pushing: {module} on front of queue").as_str());
                    known_modules.insert(hash);
                    module_queue.push_front((module, content.into()));
                }
            }
        }
    }
    rules::set_latest_starlark_module("".into());

    if let Some(stardoc) = singleton::get_inspect_stardoc_path() {
        let workspace = workspace.read();
        workspace
            .stardoc
            .generate(stardoc.as_ref())
            .context(format_context!("Failed to generate stardoc"))?;
    }

    if phase == task::Phase::Checkout {
        // check if sysroot/bin/spaces exists
        if !std::path::Path::new("sysroot/bin/spaces").exists() {
            star_logger(printer).warning(
                "sysroot/bin/spaces not found. Add a rule to checkout a compatible version of spaces to the workspace.",
            );
        }
    }

    Ok(())
}

pub fn execute_tasks(
    printer: &mut printer::Printer,
    workspace: workspace::WorkspaceArc,
    phase: task::Phase,
    target: Option<Arc<str>>,
) -> anyhow::Result<()> {
    star_logger(printer).debug("Inserting //:setup, //:all, //:test, //:clean rules");
    let run_target = insert_setup_and_all_rules(workspace.clone(), target.clone())
        .context(format_context!("failed to insert run all"))?;

    let glob_warnings = singleton::get_glob_warnings();
    for warning in glob_warnings {
        star_logger(printer).warning(warning.as_ref());
    }

    if phase == task::Phase::Checkout || singleton::get_is_rescan() || workspace.read().is_dirty {
        star_logger(printer).debug("saving JSON workspace settings");
        workspace
            .read()
            .settings
            .save_json()
            .context(format_context!("Failed to save settings"))?;
    }

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

            rules::update_depedency_graph(printer, run_target.clone(), phase)
                .context(format_context!("Failed to sort tasks"))?;

            rules::debug_sorted_tasks(printer, phase)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let execute_result = rules::execute(printer, workspace.clone(), phase);

            rules::export_log_status(workspace.clone())
                .context(format_context!("Failed to export log status"))?;

            if execute_result.is_err() {
                let read_workspace = workspace.read();
                if read_workspace.is_any_digest_updated {
                    read_workspace
                        .save_bin(printer)
                        .context(format_context!("Failed to save bin settings"))?;
                }
            }

            let _new_modules =
                execute_result.context(format_context!("Failed to execute tasks"))?;
        }
        task::Phase::Inspect => {
            star_logger(printer).message("--Inspect Phase--");

            rules::update_depedency_graph(printer, target.clone(), phase)
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
                globs.insert(format!("+{relative_path}**").into());
                strip_prefix = Some(format!("//{relative_path}").into());
            }

            if let Some(markdown_path) = singleton::get_inspect_markdown_path() {
                rules::export_tasks_as_mardown(&markdown_path)
                    .context(format_context!("Failed to export tasks as markdown"))?;
            } else {
                //only show checkout if log level is message or higher
                if printer.verbosity.level <= printer::Level::Message {
                    rules::show_tasks(
                        printer,
                        workspace.clone(),
                        task::Phase::Checkout,
                        target.clone(),
                        &globs,
                        strip_prefix.clone(),
                    )
                    .context(format_context!("Failed to show tasks"))?;
                }
                rules::show_tasks(
                    printer,
                    workspace.clone(),
                    task::Phase::Run,
                    target.clone(),
                    &globs,
                    strip_prefix,
                )
                .context(format_context!("Failed to show tasks"))?;
            }
        }
        task::Phase::Checkout => {
            star_logger(printer).message("--Post Checkout Phase--");

            // warn if any new branches don't match a git rule
            let new_branches = singleton::get_new_branches();
            for item in new_branches {
                if !rules::is_git_rule(item.as_ref()) {
                    star_logger(printer).warning(
                        format!("Did not create new branch for {item}. Not a git rule").as_str(),
                    );
                }
            }

            rules::update_depedency_graph(printer, None, task::Phase::Checkout)
                .context(format_context!("Failed to sort tasks"))?;
            rules::debug_sorted_tasks(printer, task::Phase::PostCheckout)
                .context(format_context!("Failed to debug sorted tasks"))?;

            rules::execute(printer, workspace.clone(), task::Phase::PostCheckout)
                .context(format_context!("failed to execute post checkout phase"))?;

            // Add command line env values
            let env_args = singleton::get_args_env();
            {
                let mut workspace_write = workspace.write();
                for (key, value) in env_args {
                    workspace_write
                        .env
                        .vars
                        .get_or_insert_default()
                        .insert(key, value);
                }
            }

            // prepend PATH with sysroot/bin if sysroot/bin is not already in the PATH
            let mut env = workspace.read().get_env();
            let sysroot_bin: Arc<str> =
                format!("{}/sysroot/bin", workspace.read().absolute_path).into();
            if !env.paths.as_ref().is_some_and(|e| e.contains(&sysroot_bin)) {
                env.paths.get_or_insert_default().insert(0, sysroot_bin);
            }

            // evaluate the available inherited variables
            let vars = env
                .get_checkout_vars()
                .context(format_context!("Failed to get environment variables"))?;

            env.vars.get_or_insert_default().extend(vars);
            star_logger(printer).debug(format!("env vars: {:?}", env.vars).as_str());

            if workspace.read().is_reproducible() {
                env.vars.get_or_insert_default().insert(
                    workspace::SPACES_ENV_WORKSPACE_DIGEST.into(),
                    workspace.read().digest.clone(),
                );
            }

            let absolute_path = workspace.read().get_absolute_path();
            let workspace_path = std::path::Path::new(absolute_path.as_ref());
            let env_path = workspace_path.join("env");
            env.remove_secret_vars();
            env.create_shell_env(env_path)
                .context(format_context!("failed to finalize env"))?;

            let env_str = serde_json::to_string_pretty(&env)?;

            star_logger(printer).debug("saving workspace env");
            let read_workspace = workspace.read();
            read_workspace
                .save_env_file(env_str.as_str())
                .context(format_context!("Failed to save env file"))?;

            star_logger(printer).debug("saving JSON workspace settings");
            read_workspace
                .settings
                .save_json()
                .context(format_context!("Failed to save settings"))?;

            read_workspace
                .finalize_store()
                .context(format_context!("Failed to finalize store"))?;
        }
        _ => {}
    }

    let is_checkout = phase == task::Phase::Checkout;
    let is_clean_or_checkout = target
        .map(|t| t.as_ref() == rule::CLEAN_RULE_NAME)
        .unwrap_or(is_checkout);

    if is_checkout {
        // Remove files from previous checkout that are no longer needed
        let extraneous_files = {
            let mut workspace_write_lock = workspace.write();
            workspace_write_lock.settings.get_extraneous_files()
        };

        for file in extraneous_files {
            let path = std::path::Path::new(file.as_ref());
            if path.exists() {
                star_logger(printer).warning(format!("Expired, removing: {file}").as_str());
                match std::fs::remove_file(path).context(format_context!("Failed to remove {file}"))
                {
                    Ok(_) => {}
                    Err(err) => star_logger(printer)
                        .warning(format!("Failed to remove: {file} because {err}").as_str()),
                }
            } else {
                star_logger(printer)
                    .warning(format!("Expired file already removed: {file}").as_str())
            }
        }

        workspace
            .read()
            .settings
            .save_checkout()
            .context(format_context!("Failed to save checkout settings"))?;
    }

    if workspace.read().is_bin_dirty || is_clean_or_checkout {
        star_logger(printer).debug("saving BIN workspace settings");
        if is_clean_or_checkout {
            star_logger(printer).debug("cleaning workspace: forgetting inputs");
            workspace.write().settings.bin = ws::BinSettings::default();
        }
        workspace
            .read()
            .save_bin(printer)
            .context(format_context!("Failed to save bin settings"))?;
    }

    Ok(())
}

pub fn run_starlark_modules(
    printer: &mut printer::Printer,
    workspace: workspace::WorkspaceArc,
    modules: Vec<(Arc<str>, Arc<str>)>,
    phase: task::Phase,
    target: Option<Arc<str>>,
    is_execute_tasks: IsExecuteTasks,
) -> anyhow::Result<()> {
    let is_dirty = workspace.read().is_dirty;
    let is_always_evaluate = workspace.read().settings.bin.is_always_evaluate;

    if is_dirty || is_always_evaluate || phase == task::Phase::Checkout {
        if is_always_evaluate {
            star_logger(printer).message("always evaluate modules enabled");
        } else {
            star_logger(printer).message("workspace is dirty");
        }
        evaluate_starlark_modules(printer, workspace.clone(), modules, phase)
            .context(format_context!("evaluating modules"))?;
        rules::update_tasks_digests(printer, workspace.clone())
            .context(format_context!("updating digests"))?;
    } else {
        star_logger(printer).message("workspace is clean");
        rules::import_tasks_from_workspace_settings(workspace.clone())
            .context(format_context!("importing tasks"))?;
        star_logger(printer).trace(format!("tasks {}", rules::get_pretty_tasks()).as_str());
    }

    let secrets = workspace
        .read()
        .env
        .get_secrets()
        .context(format_context!("While running checkout phase"))?;
    printer.secrets = secrets;

    if is_execute_tasks == IsExecuteTasks::Yes {
        execute_tasks(printer, workspace, phase, target)
            .context(format_context!("executing tasks"))?;
    }
    Ok(())
}

pub fn run_starlark_script(name: Arc<str>, script: Arc<str>) -> anyhow::Result<()> {
    // load SPACES_WORKSPACE from env
    let workspace = std::env::var(ws::SPACES_WORKSPACE_ENV_VAR)
        .unwrap_or(".".to_string())
        .into();

    evaluate_module(
        None,
        workspace,
        name.clone(),
        script.to_string(),
        WithRules::No,
    )
    .context(format_context!("Failed to evaluate module {}", name))?;

    Ok(())
}
