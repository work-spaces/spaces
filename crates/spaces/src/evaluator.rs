use crate::builtins::eval_context::EvalContext;
use crate::workspace::WorkspaceArc;
use crate::{builtins, executor, rules, singleton, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::{FrozenModule, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, ReturnFileLoader};
use starlark::syntax::{AstModule, Dialect};
use std::collections::HashSet;
use std::sync::Arc;
use utils::{inspect, labels, logger, query, rule, ws};

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

#[derive(Debug, Clone, Copy, PartialEq)]
enum IsSaveBin {
    No,
    Yes,
}

pub fn get_dialect() -> Dialect {
    let is_enforce_types =
        std::env::var("SPACES_ENV_IS_ENFORCE_TYPES").unwrap_or("ON".to_string()) == "ON";
    let enable_types = if is_enforce_types {
        starlark_syntax::dialect::DialectTypes::Enable
    } else {
        starlark_syntax::dialect::DialectTypes::ParseOnly
    };
    Dialect {
        enable_top_level_stmt: true,
        enable_types,
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
) -> starlark::Result<Vec<(String, FrozenModule)>> {
    // We can get the loaded modules from `ast.loads`.
    // And ultimately produce a `loader` capable of giving those modules to Starlark.
    let mut loads = Vec::new();
    for load in ast.loads() {
        let module_load_path =
            workspace::get_workspace_path(workspace_path.as_ref(), name.as_ref(), load.module_id);
        if module_load_path.ends_with(workspace::SPACES_MODULE_NAME) {
            return Err(format_error!("Error: Attempting to load module ending with `spaces.star` module. This is a reserved module name.").into());
        }
        let contents = std::fs::read_to_string(module_load_path.as_ref()).map_err(|e| {
            use starlark::{Error, ErrorKind};
            Error::new_spanned(
                ErrorKind::Fail(format_error!("Failed to load {module_load_path} -> {e}")),
                load.span.span,
                &load.span.file,
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
    eval_context: Option<EvalContext>,
) -> starlark::Result<FrozenModule> {
    let loads = evaluate_loads(
        &ast,
        name.clone(),
        workspace.clone(),
        workspace_path,
        with_rules,
    )?;

    let modules = loads.iter().map(|(a, b)| (a.as_str(), b)).collect();
    let loader = ReturnFileLoader { modules: &modules };
    let globals_builder = get_globals(with_rules);
    let globals = globals_builder.build();

    Module::with_temp_heap(move |module| {
        // `eval_context` is owned by the closure; `eval` borrows it for its
        // lifetime, which is strictly shorter than the closure body.
        let mut eval_context = eval_context;
        {
            let mut eval = Evaluator::new(&module);
            if let Some(ref mut ctx) = eval_context {
                eval.extra_mut = Some(ctx);
            }
            eval.set_loader(&loader);
            eval.eval_module(ast, &globals)?;
        }

        if let Some(workspace) = workspace
            && singleton::get_inspect_options().stardoc.is_some()
        {
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

        let frozen_module = module.freeze()?;

        Ok(frozen_module)
    })
}

pub fn evaluate_module(
    workspace: Option<WorkspaceArc>,
    workspace_path: Arc<str>,
    name: Arc<str>,
    content: String,
    with_rules: WithRules,
) -> starlark::Result<FrozenModule> {
    // Register the module name so that the global task-graph machinery can
    // track which modules exist, without writing to `latest_starlark_module`
    // (which would race with parallel evaluations).
    if workspace::is_rules_module(name.as_ref()) {
        rules::register_module(name.clone());
    }

    // Build a per-evaluation context that builtins access via `eval.extra_mut`
    // instead of through the global singleton.
    let eval_context = workspace
        .as_ref()
        .map(|w| EvalContext::new(Some(w.clone()), name.clone()));

    let dialect = get_dialect();
    let ast = AstModule::parse(name.as_ref(), content, &dialect)?;
    let module = evaluate_ast(
        ast,
        name,
        workspace,
        workspace_path,
        with_rules,
        eval_context,
    )?;
    Ok(module)
}

fn star_logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "starlark".into())
}

fn insert_setup_and_all_rules(
    workspace: workspace::WorkspaceArc,
    target: Option<Arc<str>>,
) -> anyhow::Result<Arc<str>> {
    // insert the //:setup rule

    rules::add_setup_dep_to_run_rules()
        .context(format_context!("Failed to add setup dep to run rules"))?;

    let setup_rule = rule::Rule {
        name: rule::SETUP_RULE_NAME.into(),
        help: Some("Builtin rule to run setup rules first".into()),
        inputs: None,
        outputs: None,
        targets: None,
        type_: Some(rule::RuleType::Run),
        platforms: None,
        deps: Some(rules::get_setup_rules()),
        visibility: Some(rule::Visibility::Public),
    };

    rules::insert_task(task::Task::new(
        setup_rule,
        task::Phase::Run,
        executor::Task::Target,
    ))
    .context(format_context!("Failed to insert task `setup`"))?;

    let mut deps: Vec<rule::AnyDep> = Vec::new();
    let all_deps = workspace.read().settings.bin.run_all.clone();
    for all_target in all_deps {
        deps.push(rule::AnyDep::Rule(all_target.clone()));
    }

    deps.push(rule::AnyDep::Rule(rule::SETUP_RULE_NAME.into()));

    let rule = rule::Rule {
        name: rule::ALL_RULE_NAME.into(),
        help: Some("Builtin rule to run default targets and dependencies".into()),
        inputs: None,
        outputs: None,
        targets: None,
        type_: Some(rule::RuleType::Run),
        platforms: None,
        deps: Some(rule::Deps::Any(deps)),
        visibility: Some(rule::Visibility::Public),
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
        targets: None,
        type_: Some(rule::RuleType::Test),
        platforms: None,
        deps: Some(rules::get_test_rules()),
        visibility: Some(rule::Visibility::Public),
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
        targets: None,
        type_: Some(rule::RuleType::PreCommit),
        platforms: None,
        deps: Some(rules::get_pre_commit_rules()),
        visibility: Some(rule::Visibility::Public),
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
        targets: None,
        type_: Some(rule::RuleType::Clean),
        platforms: None,
        deps: Some(rules::get_clean_rules()),
        visibility: Some(rule::Visibility::Public),
    };

    rules::insert_task(task::Task::new(
        clean_rule,
        task::Phase::Run,
        executor::Task::Target,
    ))
    .context(format_context!("Failed to insert task `//:clean`"))?;

    Ok(target.unwrap_or(rule::ALL_RULE_NAME.into()))
}

fn show_eval_progress(
    console: console::Console,
    name: &str,
    handle: std::thread::JoinHandle<anyhow::Result<()>>,
) -> anyhow::Result<()> {
    // show a progress bar if the evaluation takes more than 100ms
    let mut progress_bar = None;
    let mut count = 0;
    loop {
        if handle.is_finished() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        count += 1;
        if count == 10 {
            progress_bar = Some(console::Progress::new(
                console.clone(),
                format!("evaluating [{name}]"),
                Some(200),
                None,
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
    console: console::Console,
    workspace: workspace::WorkspaceArc,
    modules: &[(Arc<str>, Arc<str>)],
    phase: task::Phase,
) -> anyhow::Result<()> {
    let logger = star_logger(console.clone());
    logger.message("--Run Starlark Modules--");
    let workspace_path = workspace.read().absolute_path.to_owned();
    let mut known_modules = HashSet::new();

    let mut eval_progress =
        console::Progress::new(console.clone(), "top-level-eval-progress", None, None);

    eval_progress.set_prefix("evaluate-starlark");

    logger.debug("Collect Known Modules");
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
    module_queue.extend(modules.iter().cloned());
    let mut total_modules = module_queue.len();

    logger.trace(format!("Input module queue:{module_queue:?}").as_str());

    // first module is the env module. It is always evaluated first.
    // It can't be evaluated in parallel with other modules.
    if phase != task::Phase::Checkout
        && let Some((name, content)) = module_queue.pop_front()
    {
        eval_progress.set_message("env.spaces.star (first module)");
        let _ = evaluate_module(
            Some(workspace.clone()),
            workspace_path.clone(),
            name,
            content.to_string(),
            WithRules::Yes,
        )
        .map_err(|e| format_error!("Failed to evaluate module {:?}", e))?;
    }

    let mut progress = None;

    // All modules are evaulated in this loop
    // During checkout additional modules may be added to the queue
    // For Run mode, the env module is processed first and available
    // to subsequent modules
    const MAX_CONCURRENT_EVALS: usize = 8;
    let mut eval_handles: Vec<(Arc<str>, std::thread::JoinHandle<anyhow::Result<()>>)> = Vec::new();
    while !module_queue.is_empty() {
        if let Some((name, content)) = module_queue.pop_front() {
            logger.debug(format!("evaluating {name} from front of queue").as_str());
            eval_progress.set_message(name.as_ref());
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
                .map_err(|e| format_error!("Failed to evaluate module {:?}", e))?;
                Ok(())
            });

            if phase != task::Phase::Checkout {
                // Drain any already-finished handles before checking the limit.

                let mut i = 0;
                while i < eval_handles.len() {
                    if eval_handles[i].1.is_finished() {
                        let (n, h) = eval_handles.remove(i);
                        h.join()
                            .map_err(|_| format_error!("eval thread panicked for {n}"))?
                            .context(format_context!("Failed to evaluate module {n}"))?;
                    } else {
                        i += 1;
                    }
                }
                // If still at the concurrency limit, wait with progress on the oldest handle.
                if eval_handles.len() >= MAX_CONCURRENT_EVALS {
                    let (n, h) = eval_handles.remove(0);
                    show_eval_progress(console.clone(), &n, h)
                        .context(format_context!("Failed to evaluate module {n}"))?;
                }
                eval_handles.push((name, handle));
            } else {
                // During checkout phase, additional modules may be added to the queue
                // if the repo contains more spaces.star files

                if progress.is_none() {
                    progress = Some(console::Progress::new_insert(
                        console.clone(),
                        0,
                        "Executing tasks",
                        Some(0),
                        None,
                    ));
                }

                let progress = progress.as_mut().unwrap();
                let module_name = utils::labels::get_path_label_from_rule_label(name.as_ref());
                let _ = console.write(module_name);
                let rule_name = utils::labels::get_rule_name_from_label(name.as_ref());
                progress.set_prefix(format!("[{rule_name}]").as_str());

                show_eval_progress(console.clone(), &name, handle)
                    .context(format_context!("Failed to show eval progress"))?;

                logger.debug("--Checkout Phase--");

                rules::update_depedency_graph(console.clone(), None, phase)
                    .context(format_context!("Failed to evaluate dependency graph"))?;

                rules::update_target_dependency_graph(console.clone(), None).context(
                    format_context!("Failed to update run target dependency graph during checkout"),
                )?;

                rules::debug_sorted_tasks(console.clone(), phase)
                    .context(format_context!("Failed to debug sorted tasks"))?;

                progress.set_message(format!("Executing {phase} rules").as_str());

                let task_result = rules::execute(progress, workspace.clone(), phase)
                    .context(format_context!("Failed to execute tasks"))?;

                update_secrets(console.clone(), workspace.clone())
                    .context(format_context!("while running checkout tasks"))?;

                if !task_result.new_modules.is_empty() {
                    logger.trace(format!("New Modules:{:?}", task_result.new_modules).as_str());
                }

                {
                    let mut workspace_write = workspace.write();
                    workspace_write
                        .get_env_mut()
                        .repopulate_inherited_vars()
                        .context(format_context!("While populating required inherited vars"))?;
                }

                let mut new_modules = Vec::new();
                total_modules += task_result.new_modules.len();
                for module in task_result.new_modules {
                    let path_to_module = format!("{workspace_path}/{module}");
                    let content = std::fs::read_to_string(path_to_module.as_str())
                        .context(format_context!("Failed to read file {path_to_module}"))?;

                    new_modules.push((module, content));
                }

                // sorts the modules lexicographically by the filename from back to front.
                // push_front below will execute the modules in lexicographical order.
                new_modules.sort_by(|first, second| second.0.cmp(&first.0));
                logger.debug(
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
                        logger.debug(format!("Pushing: {module} on front of queue").as_str());
                        known_modules.insert(hash);
                        module_queue.push_front((module, content.into()));
                    }
                }
            }
            if let Some(progress) = progress.as_mut() {
                progress.set_finalize_lines(logger::make_finalize_line(
                    logger::FinalType::Completed,
                    progress.elapsed(),
                    "Checkout rules",
                ));
            }
        }
    }
    // Join any remaining parallel eval threads (non-checkout phase).
    for (n, h) in eval_handles.drain(..) {
        h.join()
            .map_err(|_| format_error!("eval thread panicked for {n}"))?
            .context(format_context!("Failed to evaluate module {n}"))?;
    }

    if let Some(stardoc) = singleton::get_inspect_options().stardoc {
        let workspace = workspace.read();
        workspace
            .stardoc
            .generate(stardoc.as_ref())
            .context(format_context!("Failed to generate stardoc"))?;
    }

    let final_message = if phase == task::Phase::Checkout {
        // check if sysroot/bin/spaces exists
        if !std::path::Path::new("sysroot/bin/spaces").exists() {
            logger.warning(
                "sysroot/bin/spaces not found. Add a rule to checkout a compatible version of spaces to the workspace.",
            );
        }
        let workspace_folder = std::path::Path::new(workspace_path.as_ref())
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(workspace_path.as_ref());
        format!("created {workspace_folder}")
    } else {
        format!("evaluated {total_modules} modules")
    };

    eval_progress.set_finalize_lines(logger::make_finalize_line(
        logger::FinalType::Finished,
        eval_progress.elapsed(),
        final_message.as_str(),
    ));

    Ok(())
}

fn update_secrets(
    console: console::Console,
    workspace: workspace::WorkspaceArc,
) -> anyhow::Result<()> {
    let secrets = {
        let read_workspace = workspace.read();
        read_workspace
            .get_secret_values()
            .context(format_context!("while getting secrets for checkout phase"))?
    };
    console.set_secrets(secrets);
    Ok(())
}

fn execute_tasks(
    console: console::Console,
    workspace: workspace::WorkspaceArc,
    phase: task::Phase,
    target: Option<Arc<str>>,
    run_target: Arc<str>,
    modules: &[(Arc<str>, Arc<str>)],
) -> anyhow::Result<IsSaveBin> {
    let logger = star_logger(console.clone());
    if phase == task::Phase::Checkout || singleton::get_is_rescan() || workspace.read().is_dirty {
        logger.debug("saving JSON workspace settings");
        workspace
            .read()
            .settings
            .save_json()
            .context(format_context!("Failed to save settings"))?;

        workspace
            .read()
            .settings
            .save_bin()
            .context(format_context!("Failed to save settings"))?;
    }

    match phase {
        task::Phase::Run => {
            update_secrets(console.clone(), workspace.clone())
                .context(format_context!("while entering run phase"))?;
            rules::update_target_dependency_graph(console.clone(), Some(run_target.clone()))
                .context(format_context!(
                    "Failed to update run target dependency graph for {run_target}"
                ))?;

            {
                // apply args_env to workspace
                let args_env = singleton::get_args_env();
                workspace
                    .write()
                    .get_env_mut()
                    .insert_assign_from_args(&args_env);
            }

            logger.message("--Run Phase--");

            let is_reproducible = workspace.read().is_reproducible();
            let repro_message = format!(
                "Is Workspace reproducible: {is_reproducible} -> {}",
                workspace
                    .read()
                    .settings
                    .json
                    .digest
                    .clone()
                    .unwrap_or_default()
            );
            if is_reproducible {
                logger.message(repro_message.as_str());
            } else {
                logger.info(repro_message.as_str());
            }

            rules::debug_sorted_tasks(console.clone(), phase)
                .context(format_context!("Failed to debug sorted tasks"))?;

            let mut progress =
                console::Progress::new(console.clone(), "Executing tasks", Some(0), None);

            let execute_result = rules::execute(&mut progress, workspace.clone(), phase);

            rules::export_log_status(workspace.clone())
                .context(format_context!("Failed to export log status"))?;

            if execute_result.is_err() {
                let read_workspace = workspace.read();
                if read_workspace.is_any_digest_updated {
                    read_workspace
                        .save_bin(console.clone())
                        .context(format_context!("Failed to save bin settings"))?;
                }
            }

            let _new_modules =
                execute_result.context(format_context!("Failed to execute tasks"))?;
        }
        task::Phase::Inspect => {
            update_secrets(console.clone(), workspace.clone())
                .context(format_context!("while entering inspect phase"))?;
            logger.message("--Inspect Phase--");

            rules::update_target_dependency_graph(console.clone(), target.clone()).context(
                format_context!("Failed to update target dependency graph for {target:?}"),
            )?;

            rules::debug_sorted_tasks(console.clone(), task::Phase::Checkout)
                .context(format_context!("Failed to debug sorted tasks"))?;
            rules::debug_sorted_tasks(console.clone(), task::Phase::Run)
                .context(format_context!("Failed to debug sorted tasks"))?;

            if let Some(query_command) = singleton::get_query_command() {
                let config = query_command.required_config();
                let ctx = build_query_context(console.clone(), workspace.clone(), &config)
                    .context(format_context!("Failed to build query context"))?;
                singleton::set_query_context(ctx);
                return Ok(IsSaveBin::No);
            }

            // if not filters and called from a relative path, filter on the relative path
            let inspect_options = singleton::get_inspect_options();
            let mut globs = inspect_options.filter_globs.clone();
            let relative_path = workspace.read().relative_invoked_path.clone();
            let mut strip_prefix = None;
            if globs.is_empty() && !relative_path.is_empty() {
                globs.insert(format!("+{relative_path}**").into());
                strip_prefix = Some(format!("//{relative_path}").into());
            }

            if let Some(markdown_path) = inspect_options.markdown {
                rules::export_tasks_as_mardown(&markdown_path)
                    .context(format_context!("Failed to export tasks as markdown"))?;
            } else if inspect_options.details {
                if let Some(target) = inspect_options.target {
                    let task = rules::get_task(target.as_ref())
                        .context(format_context!("Failed to get task {target}"))?;
                    let output = if inspect_options.json {
                        let mut json = serde_json::to_string_pretty(&task)
                            .context(format_context!("Failed to serialize task as JSON"))?;
                        json.push('\n');
                        json
                    } else {
                        serde_yaml::to_string(&task)
                            .context(format_context!("Failed to serialize task as YAML"))?
                    };
                    console.clone().raw(output.as_str())?;
                } else {
                    return Err(format_error!(
                        "Internal Error: details requires a rule to be specified"
                    ));
                }
            } else if inspect_options.checkout {
                let checkout_rules = rules::get_checkout_rules();
                let inspect_checkout_git_rules: Vec<_> = checkout_rules
                    .into_iter()
                    .filter_map(|e| {
                        if let executor::Task::Git(git_task) = e.executor {
                            Some(inspect::GitTask {
                                url: git_task.url.clone(),
                                rule_name: e.rule.name.clone(),
                                spaces_key: git_task.spaces_key.clone(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

                inspect_options
                    .execute_inspect_checkout(
                        console.clone(),
                        inspect_checkout_git_rules.as_slice(),
                    )
                    .context(format_context!("while inspecting checkout rules"))?;
            } else {
                //only show checkout if log level is message or higher
                let fuzzy_query_ref = inspect_options.fuzzy.as_deref();
                if console.get_level() <= console::Level::Message {
                    rules::show_tasks(
                        console.clone(),
                        workspace.clone(),
                        task::Phase::Checkout,
                        &globs,
                        strip_prefix.clone(),
                        fuzzy_query_ref,
                    )
                    .context(format_context!("Failed to show tasks"))?;
                }
                rules::show_tasks(
                    console.clone(),
                    workspace.clone(),
                    task::Phase::Run,
                    &globs,
                    strip_prefix,
                    fuzzy_query_ref,
                )
                .context(format_context!("Failed to show tasks"))?;
            }
        }
        task::Phase::Checkout => {
            update_secrets(console.clone(), workspace.clone())
                .context(format_context!("while entering post checkout phase"))?;
            logger.message("--Post Checkout Phase--");

            rules::export_log_status(workspace.clone())
                .context(format_context!("Failed to export log status"))?;

            rules::update_target_dependency_graph(console.clone(), None).context(
                format_context!("Failed to update target dependency graph for {target:?}"),
            )?;

            // warn if any new branches don't match a git rule
            let new_branches = singleton::get_new_branches();
            for item in new_branches {
                if !rules::is_git_rule(item.as_ref()) {
                    logger.warning(
                        format!("Did not create new branch for {item}. Not a git rule").as_str(),
                    );
                }
            }

            {
                let mut workspace_write = workspace.write();
                let minimum_version = workspace_write.minimum_version.to_string();
                workspace_write.settings.json.minimum_version = Some(minimum_version.into());
            }

            workspace
                .write()
                .save_env_file(modules)
                .context(format_context!("Failed to save env file"))?;

            let read_workspace = workspace.read();
            logger.debug("saving JSON workspace settings");
            read_workspace
                .settings
                .save_json()
                .context(format_context!("Failed to save json settings"))?;

            logger.debug("saving BIN workspace settings");
            read_workspace
                .settings
                .save_bin()
                .context(format_context!("Failed to save bin settings"))?;

            read_workspace
                .finalize_store()
                .context(format_context!("Failed to finalize store"))?;

            logger.debug("saving checkout store");
            read_workspace
                .settings
                .save_checkout_store()
                .context(format_context!("Failed to save checkout store"))?;
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
                logger.warning(format!("Expired, removing: {file}").as_str());
                match std::fs::remove_file(path).context(format_context!("Failed to remove {file}"))
                {
                    Ok(_) => {}
                    Err(err) => {
                        logger.warning(format!("Failed to remove: {file} because {err}").as_str())
                    }
                }
            } else {
                logger.warning(format!("Expired file already removed: {file}").as_str())
            }
        }

        workspace
            .read()
            .settings
            .save_checkout()
            .context(format_context!("Failed to save checkout settings"))?;
    }

    let is_save_bin = if workspace.read().is_bin_dirty || is_clean_or_checkout {
        if is_clean_or_checkout {
            logger.debug("cleaning workspace: forgetting inputs");
            workspace.write().settings.bin = ws::BinSettings::default();
        }
        IsSaveBin::Yes
    } else {
        IsSaveBin::No
    };

    Ok(is_save_bin)
}

fn build_query_rule(
    console: console::Console,
    workspace: &WorkspaceArc,
    task: &task::Task,
    config: &query::QueryContextConfig,
) -> anyhow::Result<query::QueryRule> {
    // Only compute expanded_deps if the config requires it
    let expanded_deps = if config.compute_expanded_deps {
        let mut progress = console::Progress::new(console.clone(), "query-deps", None, None);
        let mut dep_strings: Vec<Arc<str>> = task.collect_rule_deps();
        let glob_deps = rules::collect_task_glob_deps(task);
        let files = workspace
            .read()
            .inspect_inputs(
                &mut progress,
                &glob_deps,
                utils::changes::IsAllowNoEntries::Yes,
            )
            .context(format_context!("Failed to inspect deps globs for query"))?;
        dep_strings.extend(files.into_iter().map(|e| format!("//{e}").into()));
        Some(dep_strings)
    } else {
        None
    };

    // Only compute serialization if the config requires it
    let (serialized_yaml, serialized_json) = if config.compute_serialization {
        let json = {
            let mut s = serde_json::to_string_pretty(task).context(format_context!(
                "Internal Error: failed to serialize task for query"
            ))?;
            s.push('\n');
            s
        };
        // Derive YAML from the JSON value to avoid silent serialization failures
        let yaml = {
            let json_val: serde_json::Value =
                serde_json::from_str(json.trim()).context(format_context!(
                    "Internal Error: failed to deserialize task for query (to json from str)"
                ))?;
            serde_yaml::to_string(&json_val).context(format_context!(
                "Internal Error: failed to serialize task for query (str to yaml)"
            ))?
        };
        (Some(yaml), Some(json))
    } else {
        (None, None)
    };

    Ok(query::QueryRule {
        rule: task.rule.clone(),
        source: labels::get_source_from_label(task.rule.name.as_ref()),
        expanded_deps,
        executor_markdown: task.executor.to_markdown(),
        serialized_yaml,
        serialized_json,
    })
}

fn build_query_context(
    console: console::Console,
    workspace: WorkspaceArc,
    config: &query::QueryContextConfig,
) -> anyhow::Result<query::QueryContext> {
    let checkout_tasks = rules::get_checkout_rules();
    let run_tasks = rules::get_run_rules();

    let mut checkout_rules = Vec::new();
    let mut checkout_git_tasks = Vec::new();

    for task in &checkout_tasks {
        checkout_rules.push(
            build_query_rule(console.clone(), &workspace, task, config).context(
                format_context!("Failed to build QueryRule for {}", task.rule.name),
            )?,
        );
        if let executor::Task::Git(git_task) = &task.executor {
            checkout_git_tasks.push(inspect::GitTask {
                url: git_task.url.clone(),
                rule_name: task.rule.name.clone(),
                spaces_key: git_task.spaces_key.clone(),
            });
        }
    }

    let mut run_rules = Vec::new();
    for task in &run_tasks {
        run_rules.push(
            build_query_rule(console.clone(), &workspace, task, config).context(
                format_context!("Failed to build QueryRule for {}", task.rule.name),
            )?,
        );
    }

    let relative_invoked_path = workspace.read().relative_invoked_path.clone();

    Ok(query::QueryContext {
        checkout_rules,
        run_rules,
        checkout_git_tasks,
        relative_invoked_path,
    })
}

pub fn run_starlark_modules(
    console: console::Console,
    workspace: workspace::WorkspaceArc,
    modules: Vec<(Arc<str>, Arc<str>)>,
    phase: task::Phase,
    target: Option<Arc<str>>,
    is_execute_tasks: IsExecuteTasks,
) -> anyhow::Result<()> {
    let logger = star_logger(console.clone());
    let is_dirty = workspace.read().is_dirty;
    let is_always_evaluate = workspace.read().settings.bin.is_always_evaluate;

    let (run_target, is_save_bin) =
        if is_dirty || is_always_evaluate || phase == task::Phase::Checkout {
            if is_always_evaluate {
                logger.message("always evaluate modules enabled");
            } else if is_dirty {
                logger.message("workspace is dirty");
            } else {
                logger.message("always evaluate during checkout/sync");
            }
            evaluate_starlark_modules(console.clone(), workspace.clone(), &modules, phase)
                .context(format_context!("evaluating modules"))?;

            logger.message("Inserting //:setup, //:all, //:test, //:clean rules");
            let run_target = insert_setup_and_all_rules(workspace.clone(), target.clone())
                .context(format_context!("failed to insert run all"))?;

            // after checkout, the dependencies need to be inserted for run rules
            rules::update_depedency_graph(console.clone(), Some(workspace.clone()), phase)
                .context(format_context!("Failed to update dependency graph"))?;

            rules::update_tasks_digests(console.clone(), workspace.clone())
                .context(format_context!("updating digests"))?;

            (run_target, IsSaveBin::Yes)
        } else {
            logger.message("workspace is clean");
            let needs_graph = match is_execute_tasks {
                IsExecuteTasks::No => rules::NeedsGraph::No,
                IsExecuteTasks::Yes => rules::NeedsGraph::Yes(phase),
            };
            rules::import_tasks_from_workspace_settings(
                console.clone(),
                workspace.clone(),
                needs_graph,
            )
            .context(format_context!("importing tasks"))?;
            logger.trace(format!("tasks {}", rules::get_pretty_tasks()).as_str());
            (
                target.clone().unwrap_or(rule::ALL_RULE_NAME.into()),
                IsSaveBin::No,
            )
        };

    let is_save_bin = if is_execute_tasks == IsExecuteTasks::Yes {
        execute_tasks(
            console.clone(),
            workspace.clone(),
            phase,
            target,
            run_target,
            modules.as_slice(),
        )
        .context(format_context!("executing tasks"))?
    } else {
        is_save_bin
    };

    if is_save_bin == IsSaveBin::Yes {
        logger.debug("saving BIN workspace settings");
        workspace
            .read()
            .save_bin(console.clone())
            .context(format_context!("Failed to save bin settings"))?;
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
    .map_err(|e| format_error!("Failed to evaluate module {name}: {e}"))?;

    Ok(())
}
