use crate::workspace::WorkspaceArc;
use crate::{executor, label, singleton, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn rules_printer_logger(printer: &mut printer::Printer) -> logger::Logger {
    logger::Logger::new_printer(printer, "rules".into())
}

fn _rules_progress_logger(progress: &mut printer::MultiProgressBar) -> logger::Logger {
    logger::Logger::new_progress(progress, "rules".into())
}

fn task_logger(progress: &mut printer::MultiProgressBar, name: Arc<str>) -> logger::Logger {
    logger::Logger::new_progress(progress, name)
}

fn get_task_signal_deps(task: &task::Task) -> anyhow::Result<Vec<task::SignalArc>> {
    let state = get_state().read();
    let tasks = state.tasks.read();

    let mut result = Vec::new();
    if let Some(deps) = task.rule.deps.as_ref() {
        for dep in deps {
            // # tasks * # deps + hashmap access is EXPENSIVE
            // this causes a substantial delay when starting spaces
            let dep_task = tasks.get(dep).ok_or(format_error!(
                "Task Dependency {} not found for {}",
                dep,
                task.rule.name
            ))?;

            if dep_task.phase == task::Phase::Complete || task::Phase::Cancelled == dep_task.phase {
                continue;
            }

            match task.phase {
                task::Phase::Run => {
                    if dep_task.phase != task::Phase::Run {
                        return Err(format_error!(
                            "Run task {} cannot depend on non-run task {} -> {}",
                            task.rule.name,
                            dep_task.rule.name,
                            dep_task.phase
                        ));
                    }
                    if task.rule.type_ == Some(rule::RuleType::Setup)
                        && dep_task.rule.type_ != Some(rule::RuleType::Setup)
                    {
                        return Err(format_error!(
                            "Setup task {} cannot depend on non-setup task {} -> {}",
                            task.rule.name,
                            dep_task.rule.name,
                            dep_task.phase
                        ));
                    }
                }
                task::Phase::Checkout => {
                    if dep_task.phase != task::Phase::Checkout {
                        return Err(format_error!(
                            "Checkout task {} cannot depend on non-checkout task {} -> {}",
                            task.rule.name,
                            dep_task.rule.name,
                            dep_task.phase
                        ));
                    }
                }
                _ => {}
            }

            result.push(dep_task.signal.clone());
        }
    }
    Ok(result)
}

pub fn execute_task(
    mut progress: printer::MultiProgressBar,
    workspace: workspace::WorkspaceArc,
    task: &task::Task,
) -> std::thread::JoinHandle<anyhow::Result<executor::TaskResult>> {
    let task = task.clone();

    struct SignalOnDrop {
        signal: task::SignalArc,
    }

    impl Drop for SignalOnDrop {
        fn drop(&mut self) {
            self.signal.set_ready_notify_all();
        }
    }

    progress.set_message(format!("Waiting for dependencies ({:?})", task.phase).as_str());

    std::thread::spawn(move || -> anyhow::Result<executor::TaskResult> {
        // check inputs/outputs to see if we need to run
        let name = task.rule.name.clone();

        // when this goes out of scope it will notify the dependents
        let _signal_on_drop = SignalOnDrop {
            signal: task.signal.clone(),
        };

        let mut skip_execute_message: Option<Arc<str>> = None;
        if let (Some(platforms), Some(current_platform)) = (
            task.rule.platforms.as_ref(),
            platform::Platform::get_platform(),
        ) {
            if !platforms.contains(&current_platform) {
                skip_execute_message = Some("Skipping: platform not enabled".into());
            }
        }

        task_logger(&mut progress, name.clone()).debug(
            format!("Skip execute message after platform check? {skip_execute_message:?}").as_str(),
        );

        let deps_signals =
            get_task_signal_deps(&task).context(format_context!("Failed to get signal deps"))?;
        let total = deps_signals.len();

        task_logger(&mut progress, name.clone()).trace(format!("{} dependencies", total).as_str());

        let mut count = 1;
        for deps_rule_signal in deps_signals {
            let signal_name = {
                let (lock, _) = &*deps_rule_signal.signal;
                let signal_access = lock.lock().unwrap();
                signal_access.name.clone()
            };

            task_logger(&mut progress, name.clone()).debug(
                format!(
                    "{name} Waiting for dependency {} {count}/{total}",
                    signal_name
                )
                .as_str(),
            );

            deps_rule_signal.wait_is_ready(std::time::Duration::from_millis(100));
            count += 1;
        }

        task_logger(&mut progress, name.clone())
            .debug(format!("{name} All dependencies are done").as_str());

        {
            task_logger(&mut progress, name.clone())
                .debug(format!("{name} check for skipping/cancelation").as_str());
            let state = get_state().read();
            let tasks = state.tasks.read();
            let task = tasks
                .get(name.as_ref())
                .context(format_context!("Task not found {name}"))?;
            if task.phase == task::Phase::Cancelled {
                task_logger(&mut progress, name.clone())
                    .debug(format!("Skipping {name}: cancelled").as_str());
                skip_execute_message = Some("Skipping because it was cancelled".into());
            } else if task.rule.type_ == Some(rule::RuleType::Optional) {
                task_logger(&mut progress, name.clone()).debug("Skipping because it is optional");
                skip_execute_message = Some("Skipping: optional".into());
            }
            task_logger(&mut progress, name.clone())
                .trace(format!("{name} done checking skip cancellation").as_str());
        }

        let rule_name = name.clone();

        let updated_digest = if let Some(inputs) = &task.rule.inputs {
            if skip_execute_message.is_some() {
                None
            } else {
                task_logger(&mut progress, name.clone())
                    .debug(format!("update workspace changes {} inputs", inputs.len()).as_str());

                workspace
                    .write()
                    .update_changes(&mut progress, inputs)
                    .context(format_context!("Failed to update workspace changes"))?;

                task_logger(&mut progress, name.clone()).debug("check for new digest");

                let seed = serde_json::to_string(&task.executor)
                    .context(format_context!("Failed to serialize"))?;
                let digest = workspace
                    .read()
                    .is_rule_inputs_changed(&mut progress, &rule_name, seed.as_str(), inputs)
                    .context(format_context!("Failed to check inputs for {rule_name}"))?;
                if digest.is_none() {
                    // the digest has not changed - not need to execute
                    skip_execute_message = Some("Skipping: same inputs".into());
                }
                task_logger(&mut progress, name.clone())
                    .debug(format!("New digest for {rule_name}={digest:?}").as_str());
                digest
            }
        } else {
            None
        };

        if let Some(skip_message) = skip_execute_message.as_ref() {
            task_logger(&mut progress, name.clone()).info(skip_message.as_ref());
            progress.set_message(skip_message);
        } else {
            task_logger(&mut progress, name.clone()).debug("Running task");
            progress.set_message("Running");
        }

        // time how long it takes to execute the task
        let start_time = std::time::Instant::now();

        progress.reset_elapsed();
        let task_result = if let Some(message) = skip_execute_message.as_ref() {
            progress.set_ending_message(message);
            Ok(executor::TaskResult::new())
        } else {
            task.executor
                .execute(progress, workspace.clone(), &rule_name)
                .context(format_context!("Failed to exec {}", name))
        };

        let elapsed_time = start_time.elapsed();
        workspace
            .write()
            .update_rule_metrics(&rule_name, elapsed_time);

        if task_result.is_ok() {
            if let Some(digest) = updated_digest {
                workspace.write().update_rule_digest(&rule_name, digest);
            }
        }

        // before notifying dependents process the enabled_targets list
        {
            let mut log_status = LogStatus {
                name: rule_name.clone(),
                duration: elapsed_time,
                file: if skip_execute_message.is_some() {
                    "<skipped>".into()
                } else {
                    workspace.read().get_log_file(&rule_name)
                },
                status: executor::exec::Expect::Success,
            };

            let state = get_state().read();
            let mut tasks = state.tasks.write();
            if let Ok(task_result) = &task_result {
                log_status.status = executor::exec::Expect::Success;
                for enabled_target in task_result.enabled_targets.iter() {
                    let task = tasks
                        .get_mut(enabled_target)
                        .ok_or(format_error!("Task not found {enabled_target}"))
                        .unwrap_or_else(|_| {
                            panic!("Internal Error: Task not found {enabled_target}")
                        });
                    task.rule.type_ = Some(rule::RuleType::Run);
                }
            } else {
                log_status.status = executor::exec::Expect::Failure;
                // Cancel all pending tasks - exit gracefully
                for task in tasks.values_mut() {
                    task.phase = task::Phase::Cancelled;
                }
            }

            let task = tasks
                .get_mut(name.as_ref())
                .context(format_context!("Task not found {name}"))?;
            task.phase = task::Phase::Complete;

            state.log_status.write().push(log_status);
        }

        task_result

        // _signal_on_drop.drop() notifies the dependents
    })
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LogStatus {
    pub name: Arc<str>,
    pub status: executor::exec::Expect,
    pub duration: std::time::Duration,
    pub file: Arc<str>,
}

#[derive(Debug)]
pub struct State {
    pub tasks: lock::StateLock<HashMap<Arc<str>, task::Task>>,
    pub graph: graph::Graph,
    pub sorted: Vec<petgraph::prelude::NodeIndex>,
    pub latest_starlark_module: Option<Arc<str>>,
    pub all_modules: HashSet<Arc<str>>,
    pub log_status: lock::StateLock<Vec<LogStatus>>,
}

impl State {
    pub fn get_sanitized_rule_name(&self, rule_name: Arc<str>) -> Arc<str> {
        label::sanitize_rule(rule_name, self.latest_starlark_module.clone())
    }

    pub fn get_sanitized_working_directory(&self, rule_name: Arc<str>) -> Arc<str> {
        label::sanitize_working_directory(rule_name, self.latest_starlark_module.clone())
    }

    pub fn insert_task(&self, mut task: task::Task) -> anyhow::Result<()> {
        // update the rule name to have the starlark module name
        let rule_label = label::sanitize_rule(task.rule.name, self.latest_starlark_module.clone());
        task.rule.name = rule_label.clone();
        task.signal = task::SignalArc::new(rule_label.clone());

        // update deps that refer to rules in the same starlark module
        if let Some(deps) = task.rule.deps.as_mut() {
            for dep in deps.iter_mut() {
                if label::is_rule_sanitized(dep) {
                    continue;
                }
                // sanitize the rule by prepending the current module location
                *dep = label::sanitize_rule(dep.clone(), self.latest_starlark_module.clone());
            }
        }

        let mut tasks = self.tasks.write();

        if let Some(task) = tasks.get(&rule_label) {
            return Err(format_error!(
                "Rule already exists {rule_label} with {task:?}"
            ));
        } else {
            tasks.insert(rule_label, task);
        }

        Ok(())
    }

    pub fn update_dependency_graph(
        &mut self,
        printer: &mut printer::Printer,
        target: Option<Arc<str>>,
        phase: task::Phase,
    ) -> anyhow::Result<()> {
        let mut tasks = self.tasks.write();

        self.graph.clear();
        // add all tasks to the graph
        rules_printer_logger(printer)
            .debug(format!("Adding {} tasks to graph", tasks.len()).as_str());
        for task in tasks.values() {
            self.graph.add_task(task.rule.name.clone());
        }

        rules_printer_logger(printer).debug("Adding deps to graph tasks");

        {
            let mut multiprogress = printer::MultiProgress::new(printer);
            let start_time = std::time::Instant::now();
            let mut progress: Option<printer::MultiProgressBar> = None;
            for task in tasks.values_mut() {
                let now = std::time::Instant::now();
                if now.duration_since(start_time).as_millis() > 100 {
                    if let Some(progress) = progress.as_mut() {
                        progress.increment_with_overflow(1);
                        progress.set_message("populating dependency graph");
                    } else {
                        progress = Some(multiprogress.add_progress(
                            "workspace",
                            Some(200),
                            Some("Populated Graph"),
                        ));
                    }
                }

                let task_phase = task.phase;
                if phase == task::Phase::Checkout && task_phase != task::Phase::Checkout {
                    // skip evaluating non-checkout tasks during checkout
                    continue;
                }

                // connect the dependencies
                if let Some(deps) = task.rule.deps.as_ref() {
                    for dep in deps {
                        self.graph.add_dependency(&task.rule.name, dep).context(
                            format_context!(
                                "Failed to add dependency {dep} to task {}: {}",
                                task.rule.name,
                                self.graph.get_target_not_found(dep.clone())
                            ),
                        )?;
                    }
                }
            }
        }

        let target_is_some = target.is_some();

        rules_printer_logger(printer)
            .debug(format!("sorting graph with for {target:?}...").as_str());
        self.sorted = self
            .graph
            .get_sorted_tasks(target)
            .context(format_context!("Failed to sort tasks"))?;

        rules_printer_logger(printer)
            .debug(format!("done with {} nodes", self.sorted.len()).as_str());

        if target_is_some {
            // enable any optional tasks in the graph
            for node_index in self.sorted.iter() {
                let task_name = self.graph.get_task(*node_index);
                let task = tasks
                    .get_mut(task_name)
                    .ok_or(format_error!("Task not found {task_name}"))?;
                if task.rule.type_ == Some(rule::RuleType::Optional) {
                    task.rule.type_ = Some(rule::RuleType::Run);
                }
            }
        }

        Ok(())
    }

    pub fn import_tasks_from_workspace_settings(
        &mut self,
        workspace: workspace::WorkspaceArc,
    ) -> anyhow::Result<()> {
        {
            let workspace = workspace.read();
            let mut tasks = self.tasks.write();
            *tasks = serde_json::from_str(&workspace.settings.bin.tasks_json)
                .context(format_context!("Failed to parse tasks"))?;

            for task in tasks.values_mut() {
                task.signal = task::SignalArc::new(task.rule.name.clone());
            }
        }
        {
            let mut workspace = workspace.write();
            let env: environment::Environment =
                serde_json::from_str(&workspace.settings.bin.env_json)
                    .context(format_context!("Failed to parse env"))?;
            workspace.set_env(env);
        }
        Ok(())
    }

    pub fn update_tasks_digests(
        &self,
        printer: &mut printer::Printer,
        workspace: WorkspaceArc,
    ) -> anyhow::Result<()> {
        if !workspace.read().is_dirty {
            return Ok(());
        }
        rules_printer_logger(printer).info("sorting and hashing");
        let topo_sorted = self
            .graph
            .get_sorted_tasks(None)
            .context(format_context!("Failed to sort tasks for phase digesting",))?;

        let mut tasks = self.tasks.write();
        for node in topo_sorted.iter() {
            let task_name = self.graph.get_task(*node);
            let task = tasks.get(task_name).cloned();
            if let Some(task) = task {
                let mut task_hasher = blake3::Hasher::new();
                task_hasher.update(task.calculate_digest().as_bytes());
                let mut deps = task.rule.deps.clone().unwrap_or_default();
                deps.sort();
                for dep in deps {
                    if let Some(dep_task) = tasks.get(&dep) {
                        task_hasher.update(dep_task.digest.as_bytes());
                    }
                }
                if let Some(task_mut) = tasks.get_mut(task_name) {
                    task_mut.digest = task_hasher.finalize().to_string().into();
                }
            }
        }

        let serde_tasks = tasks.clone();
        workspace.write().settings.bin.tasks_json = serde_json::to_string(&serde_tasks)
            .map_err(|e| format_error!("Failed to encode {e}"))?
            .into();

        Ok(())
    }

    pub fn show_tasks(
        &self,
        printer: &mut printer::Printer,
        phase: task::Phase,
        _target: Option<Arc<str>>,
        filter: &HashSet<Arc<str>>,
        strip_prefix: Option<Arc<str>>,
    ) -> anyhow::Result<()> {
        let tasks = self.tasks.read();

        #[derive(Serialize)]
        struct TaskInfo {
            help: String,
        }
        let mut task_info_list: HashMap<Arc<str>, _> = std::collections::HashMap::new();
        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);

            if !filter.is_empty()
                && !changes::glob::match_globs(
                    filter,
                    task_name.strip_prefix("//").unwrap_or(task_name),
                )
            {
                logger::Logger::new_printer(printer, "glob".into())
                    .debug(format!("Filtering {task_name} with {filter:?}").as_str());
                continue;
            }

            let task = tasks
                .get(task_name)
                .ok_or(format_error!("Task not found {task_name}"))?;

            if singleton::get_has_help() && task.rule.help.is_none() {
                continue;
            }

            if task.phase == phase {
                if printer.verbosity.level == printer::Level::Debug {
                    printer.debug(task_name, &task)?;
                } else {
                    let help = task
                        .rule
                        .help
                        .clone()
                        .map(|e| e.as_ref().to_string())
                        .unwrap_or("<Not Provided>".to_string());

                    let mut task_name = task.rule.name.as_ref();
                    if let Some(strip_prefix) = strip_prefix.as_ref() {
                        if let Some(stripped) = task_name.strip_prefix(strip_prefix.as_ref()) {
                            task_name = stripped.strip_prefix("/").unwrap_or(stripped);
                        }
                    }

                    task_info_list.insert(task_name.into(), TaskInfo { help });
                }
            }
        }

        printer.info(phase.to_string().as_str(), &task_info_list)?;

        Ok(())
    }

    fn export_tasks_as_mardown(&self, path: &str) -> anyhow::Result<()> {
        let tasks = self.tasks.read();

        let run_rules = tasks
            .values()
            .filter_map(|task| {
                if task.phase == task::Phase::Run {
                    Some(&task.rule)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let mut printer = printer::Printer::new_file(path)
            .context(format_context!("Failed to create file {path}"))?;
        let mut md_printer = printer::markdown::Markdown::new(&mut printer);
        let md = &mut md_printer;
        rule::Rule::print_markdown_header(md)?;
        rule::Rule::print_markdown_section_heading(md, "Run Rules", &run_rules)?;
        rule::Rule::print_markdown_section_body(md, "Run Rules", &run_rules)?;
        Ok(())
    }

    fn execute(
        &self,
        printer: &mut printer::Printer,
        workspace: workspace::WorkspaceArc,
        phase: task::Phase,
    ) -> anyhow::Result<executor::TaskResult> {
        let mut task_result = executor::TaskResult::new();
        let mut multi_progress = printer::MultiProgress::new(printer);
        let mut handle_list = Vec::new();

        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);
            let task = {
                let tasks = self.tasks.read();
                tasks
                    .get(task_name)
                    .ok_or(format_error!("Task not found {task_name}"))?
                    .clone()
            };

            if task.phase == phase {
                let message = if task.rule.type_ == Some(rule::RuleType::Optional) {
                    "Skipped (Optional)".to_string()
                } else {
                    let message = if let Some(rule_type) = task.rule.type_ {
                        format!("{:?}", rule_type)
                    } else {
                        format!("{:?}", phase)
                    };
                    format!("Complete ({message})")
                };

                let mut progress_bar = multi_progress.add_progress(
                    &label::sanitize_rule_for_display(task.rule.name.clone()),
                    Some(100),
                    Some(message.as_str()),
                );

                task_logger(&mut progress_bar, task_name.into())
                    .debug(format!("Staging task {}", task.rule.name).as_str());

                handle_list.push(execute_task(progress_bar, workspace.clone(), &task));

                loop {
                    let mut number_running = 0;
                    for handle in handle_list.iter() {
                        if !handle.is_finished() {
                            number_running += 1;
                        }
                    }

                    // this could be configured with a another global starlark function
                    if number_running < singleton::get_max_queue_count() {
                        break;
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        }

        let mut first_error = None;
        for handle in handle_list {
            let handle_join_result = handle.join();
            match handle_join_result {
                Ok(handle_result) => match handle_result {
                    Ok(handle_task_result) => {
                        task_result.extend(handle_task_result);
                    }
                    Err(err) => {
                        let err_message = err.to_string();
                        singleton::process_anyhow_error(err);
                        first_error = Some(format_error!("Task failed: {err_message}"));
                    }
                },
                Err(err) => {
                    let message = format!("Failed to join thread: {err:?}");
                    singleton::process_error(message);
                    first_error = Some(format_error!("Failed to join thread: {err:?}"));
                }
            }
        }

        if let Some(err) = first_error {
            return Err(err);
        }

        Ok(task_result)
    }
}

static STATE: state::InitCell<lock::StateLock<State>> = state::InitCell::new();

fn get_state() -> &'static lock::StateLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    STATE.set(lock::StateLock::new(State {
        tasks: lock::StateLock::new(HashMap::new()),
        graph: graph::Graph::default(),
        sorted: Vec::new(),
        latest_starlark_module: None,
        all_modules: HashSet::new(),
        log_status: lock::StateLock::new(Vec::new()),
    }));
    STATE.get()
}

pub fn get_checkout_path() -> anyhow::Result<Arc<str>> {
    let state = get_state().read();
    if let Some(latest) = state.latest_starlark_module.as_ref() {
        let path = std::path::Path::new(latest.as_ref());
        let parent = path
            .parent()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(parent.into())
    } else {
        Err(format_error!("Internal Error: No starlark module set"))
    }
}

pub fn get_path_to_build_checkout(rule_name: Arc<str>) -> anyhow::Result<Arc<str>> {
    let state = get_state().read();
    let rule_name = state.get_sanitized_rule_name(rule_name);
    Ok(format!("build/{}", rule_name).into())
}

pub fn get_sanitized_rule_name(rule_name: Arc<str>) -> Arc<str> {
    let state = get_state().read();
    state.get_sanitized_rule_name(rule_name)
}

pub fn get_sanitized_working_directory(rule_name: Arc<str>) -> Arc<str> {
    let state = get_state().read();
    state.get_sanitized_working_directory(rule_name)
}

pub fn insert_task(task: task::Task) -> anyhow::Result<()> {
    let state = get_state().read();
    state.insert_task(task)
}

pub fn set_latest_starlark_module(name: Arc<str>) {
    let mut state = get_state().write();
    state.latest_starlark_module = Some(name.clone());
    state.all_modules.insert(name);
}

pub fn show_tasks(
    printer: &mut printer::Printer,
    phase: task::Phase,
    target: Option<Arc<str>>,
    filter: &HashSet<Arc<str>>,
    strip_prefix: Option<Arc<str>>,
) -> anyhow::Result<()> {
    let state = get_state().read();
    state.show_tasks(printer, phase, target, filter, strip_prefix)
}

pub fn export_tasks_as_mardown(path: &str) -> anyhow::Result<()> {
    let state = get_state().read();
    state.export_tasks_as_mardown(path)
}

pub fn update_tasks_digests(
    printer: &mut printer::Printer,
    workspace: workspace::WorkspaceArc,
) -> anyhow::Result<()> {
    let state = get_state().read();
    state.update_tasks_digests(printer, workspace)
}

pub fn update_depedency_graph(
    printer: &mut printer::Printer,
    target: Option<Arc<str>>,
    phase: task::Phase,
) -> anyhow::Result<()> {
    let mut state = get_state().write();
    state.update_dependency_graph(printer, target, phase)
}

pub fn import_tasks_from_workspace_settings(
    workspace: workspace::WorkspaceArc,
) -> anyhow::Result<()> {
    let mut state = get_state().write();
    state.import_tasks_from_workspace_settings(workspace)
}

pub fn get_pretty_tasks() -> String {
    let state = get_state().read();
    let tasks = state.tasks.read();
    let tasks = tasks.clone();
    serde_json::to_string_pretty(&tasks).unwrap()
}

pub fn execute(
    printer: &mut printer::Printer,
    workspace: workspace::WorkspaceArc,
    phase: task::Phase,
) -> anyhow::Result<executor::TaskResult> {
    let state: std::sync::RwLockReadGuard<'_, State> = get_state().read();
    state.execute(printer, workspace, phase)
}

pub fn add_setup_dep_to_run_rules() -> anyhow::Result<()> {
    let state = get_state().read();
    let mut tasks = state.tasks.write();
    for task in tasks.values_mut() {
        if task.rule.type_ != Some(rule::RuleType::Setup) && task.phase == task::Phase::Run {
            task.rule
                .deps
                .get_or_insert_with(Vec::new)
                .push(rule::SETUP_RULE_NAME.into());
        }
    }
    Ok(())
}

pub fn get_setup_rules() -> Vec<Arc<str>> {
    let state = get_state().read();
    let tasks = state.tasks.read();
    tasks
        .values()
        .filter(|task| task.rule.type_ == Some(rule::RuleType::Setup))
        .map(|task| task.rule.name.clone())
        .collect()
}

pub fn export_log_status(workspace: WorkspaceArc) -> anyhow::Result<()> {
    let state = get_state().read();
    let log_status = state.log_status.read().clone();
    let log_output_folder = workspace.read().log_directory.clone();
    let log_status_file_output = format!("{}/log_status.json", log_output_folder);
    let content = serde_json::to_string_pretty(&log_status)
        .context(format_context!("Failed to serialize log status"))?;
    std::fs::write(log_status_file_output.as_str(), content).context(format_context!(
        "Failed to write log status to {log_status_file_output}"
    ))?;
    Ok(())
}

pub fn debug_sorted_tasks(
    printer: &mut printer::Printer,
    phase: task::Phase,
) -> anyhow::Result<()> {
    let state = get_state().read();
    for node_index in state.sorted.iter() {
        let task_name = state.graph.get_task(*node_index);
        if let Some(task) = state.tasks.read().get(task_name) {
            if task.phase == phase {
                rules_printer_logger(printer).debug(format!("Queued task {task_name}").as_str());
            }
        }
    }
    Ok(())
}
