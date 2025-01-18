use crate::{executor, label, singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ValueEnum)]
pub enum Phase {
    Checkout,
    PostCheckout,
    Run,
    Evaluate,
    Complete,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RuleType {
    Setup,
    Run,
    Optional,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: Arc<str>,
    pub deps: Option<Vec<Arc<str>>>,
    pub help: Option<Arc<str>>,
    pub inputs: Option<HashSet<Arc<str>>>,
    pub outputs: Option<HashSet<Arc<str>>>,
    pub platforms: Option<Vec<platform::Platform>>,
    #[serde(rename = "type")]
    pub type_: Option<RuleType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Signal {
    ready: bool,
    name: Arc<str>,
}

#[derive(Default, Debug, Clone)]
struct RuleSignal {
    signal: Arc<(Mutex<Signal>, Condvar)>,
}

impl RuleSignal {
    fn new(name: Arc<str>) -> Self {
        RuleSignal {
            signal: Arc::new((Mutex::new(Signal { ready: false, name }), Condvar::new())),
        }
    }

    fn wait_is_ready(&self, duration: std::time::Duration) {
        loop {
            let (lock, cvar) = &*self.signal;
            let signal_access = lock.lock().unwrap();
            if !signal_access.ready {
                let _ = cvar.wait_timeout(signal_access, duration).unwrap();
            } else {
                break;
            }
        }
    }

    fn set_ready_notify_all(&self) {
        let (lock, cvar) = &*self.signal;
        let mut signal_access = lock.lock().unwrap();
        signal_access.ready = true;
        cvar.notify_all();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub executor: executor::Task,
    pub phase: Phase,
    pub rule: Rule,
    pub digest: Arc<str>,
    #[serde(skip)]
    signal: RuleSignal,
    #[serde(skip)]
    deps_signals: Vec<RuleSignal>,
}

impl Task {
    pub fn new(rule: Rule, phase: Phase, executor: executor::Task) -> Self {
        Task {
            executor,
            phase,
            signal: RuleSignal::new(rule.name.clone()),
            deps_signals: Vec::new(),
            rule,
            digest: "".into(),
        }
    }

    pub fn calculate_digest(&self) -> blake3::Hash {
        let mut self_clone = self.clone();
        self_clone.digest = "".into();
        let seed = serde_json::to_string(&self_clone).unwrap();
        let mut digest = blake3::Hasher::new();
        digest.update(seed.as_bytes());
        digest.finalize()
    }

    pub fn update_implicit_dependency(&mut self, other_task: &Task) {
        if let Some(deps) = &self.rule.deps {
            if deps.contains(&other_task.rule.name) {
                return;
            }
        }

        if let Some(inputs) = &self.rule.inputs {
            for input in inputs {
                if let Some(other_outputs) = &other_task.rule.outputs {
                    if other_outputs.contains(input) {
                        if let Some(deps) = self.rule.deps.as_mut() {
                            deps.push(other_task.rule.name.clone());
                        } else {
                            self.rule.deps = Some(vec![other_task.rule.name.clone()]);
                        }
                        return;
                    }
                }
            }
        }
    }

    pub fn execute(
        &self,
        mut progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
    ) -> std::thread::JoinHandle<anyhow::Result<executor::TaskResult>> {
        let name = self.rule.name.clone();
        let executor = self.executor.clone();
        let signal = self.signal.clone();
        let rule = self.rule.clone();
        let deps_signals = self.deps_signals.clone();

        progress.set_message(format!("Waiting for dependencies ({:?})", self.phase).as_str());

        std::thread::spawn(move || -> anyhow::Result<executor::TaskResult> {
            // check inputs/outputs to see if we need to run
            let mut skip_execute_message: Option<Arc<str>> = None;
            if let (Some(platforms), Some(current_platform)) =
                (rule.platforms.as_ref(), platform::Platform::get_platform())
            {
                if !platforms.contains(&current_platform) {
                    skip_execute_message = Some("Skipping: platform not enabled".into());
                }
            }

            logger::Logger::new_progress(&mut progress, name.clone()).debug(
                format!("Skip execute message after platform check? {skip_execute_message:?}")
                    .as_str(),
            );

            let total = deps_signals.len();

            logger::Logger::new_progress(&mut progress, name.clone())
                .trace(format!("{name} has {} dependencies", total).as_str());

            let mut count = 1;
            for deps_rule_signal in deps_signals {
                {
                    let (lock, _) = &*deps_rule_signal.signal;
                    let signal_access = lock.lock().unwrap();
                    logger::Logger::new_progress(&mut progress, name.clone()).debug(
                        format!(
                            "{name} Waiting for dependency {} {count}/{total}",
                            signal_access.name
                        )
                        .as_str(),
                    );
                }

                deps_rule_signal.wait_is_ready(std::time::Duration::from_millis(100));
                count += 1;
            }

            logger::Logger::new_progress(&mut progress, name.clone())
                .debug(format!("{name} All dependencies are done").as_str());

            {
                logger::Logger::new_progress(&mut progress, name.clone())
                    .debug(format!("{name} check for skipping/cancelation").as_str());
                let state = get_state().read();
                let tasks = state.tasks.read();
                let task = tasks
                    .get(name.as_ref())
                    .context(format_context!("Task not found {name}"))?;
                if task.phase == Phase::Cancelled {
                    logger::Logger::new_progress(&mut progress, name.clone())
                        .debug(format!("Skipping {name}: cancelled").as_str());
                    skip_execute_message = Some("Skipping because it was cancelled".into());
                } else if task.rule.type_ == Some(RuleType::Optional) {
                    logger::Logger::new_progress(&mut progress, name.clone())
                        .debug("Skipping because it is optional");
                    skip_execute_message = Some("Skipping: optional".into());
                }
                logger::Logger::new_progress(&mut progress, name.clone())
                    .trace(format!("{name} done checking skip cancellation").as_str());
            }

            let rule_name = rule.name.clone();

            let updated_digest = if let Some(inputs) = &rule.inputs {
                if skip_execute_message.is_some() {
                    None
                } else {
                    logger::Logger::new_progress(&mut progress, name.clone())
                        .trace(format!("{name} update workspace changes").as_str());

                    workspace
                        .write()
                        .update_changes(&mut progress, inputs)
                        .context(format_context!("Failed to update workspace changes"))?;

                    logger::Logger::new_progress(&mut progress, name.clone())
                        .trace(format!("{name} check for new digest").as_str());

                    let seed = serde_json::to_string(&executor)
                        .context(format_context!("Failed to serialize"))?;
                    let digest = workspace
                        .read()
                        .is_rule_inputs_changed(&mut progress, &rule_name, seed.as_str(), inputs)
                        .context(format_context!("Failed to check inputs for {rule_name}"))?;
                    if digest.is_none() {
                        // the digest has not changed - not need to execute
                        skip_execute_message = Some("Skipping: same inputs".into());
                    }
                    logger::Logger::new_progress(&mut progress, name.clone())
                        .debug(format!("New digest for {rule_name}={digest:?}").as_str());
                    digest
                }
            } else {
                None
            };

            if let Some(skip_message) = skip_execute_message.as_ref() {
                logger::Logger::new_progress(&mut progress, name.clone())
                    .info(skip_message.as_ref());
                progress.set_message(skip_message);
            } else {
                progress.set_message("Running");
            }

            // time how long it takes to execute the task
            let start_time = std::time::Instant::now();

            progress.reset_elapsed();
            let task_result = if let Some(message) = skip_execute_message {
                progress.set_ending_message(message.as_ref());
                Ok(executor::TaskResult::new())
            } else {
                executor
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
                let state = get_state().read();
                let mut tasks = state.tasks.write();
                if let Ok(task_result) = &task_result {
                    for enabled_target in task_result.enabled_targets.iter() {
                        let task = tasks
                            .get_mut(enabled_target)
                            .ok_or(format_error!("Task not found {enabled_target}"))
                            .unwrap_or_else(|_| {
                                panic!("Internal Error: Task not found {enabled_target}")
                            });
                        task.rule.type_ = Some(RuleType::Run);
                    }
                } else {
                    // Cancel all pending tasks - exit gracefully
                    for task in tasks.values_mut() {
                        task.phase = Phase::Cancelled;
                    }
                }

                let task = tasks
                    .get_mut(name.as_ref())
                    .context(format_context!("Task not found {name}"))?;
                task.phase = Phase::Complete;
            }

            signal.set_ready_notify_all();

            task_result
        })
    }

    pub fn add_signal_dependency(&mut self, task: &Task) {
        self.deps_signals.push(task.signal.clone());
    }
}

pub fn get_sanitized_rule_name(rule_name: Arc<str>) -> Arc<str> {
    let state = get_state().read();
    state.get_sanitized_rule_name(rule_name)
}

pub fn insert_task(task: Task) -> anyhow::Result<()> {
    let state = get_state().read();
    state.insert_task(task)
}

pub fn set_latest_starlark_module(name: Arc<str>) {
    let mut state = get_state().write();
    state.latest_starlark_module = Some(name.clone());
    state.all_modules.insert(name);
}

pub fn show_tasks(printer: &mut printer::Printer) -> anyhow::Result<()> {
    let state = get_state().read();
    state.show_tasks(printer)
}

pub fn sort_tasks(target: Option<Arc<str>>, phase: Phase) -> anyhow::Result<()> {
    let mut state = get_state().write();
    state.sort_tasks(target, phase)
}

pub fn execute(
    printer: &mut printer::Printer,
    workspace: workspace::WorkspaceArc,
    phase: Phase,
) -> anyhow::Result<executor::TaskResult> {
    let state: std::sync::RwLockReadGuard<'_, State> = get_state().read();
    state.execute(printer, workspace, phase)
}

pub fn debug_sorted_tasks(printer: &mut printer::Printer, phase: Phase) -> anyhow::Result<()> {
    let state = get_state().read();
    for node_index in state.sorted.iter() {
        let task_name = state.graph.get_task(*node_index);
        if let Some(task) = state.tasks.read().get(task_name) {
            if task.phase == phase {
                logger::Logger::new_printer(printer, "phase".into())
                    .debug(format!("Queued task {task_name}").as_str());
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct State {
    pub tasks: lock::StateLock<HashMap<Arc<str>, Task>>,
    pub graph: graph::Graph,
    pub sorted: Vec<petgraph::prelude::NodeIndex>,
    pub latest_starlark_module: Option<Arc<str>>,
    pub all_modules: HashSet<Arc<str>>,
}

impl State {
    pub fn get_sanitized_rule_name(&self, rule_name: Arc<str>) -> Arc<str> {
        label::sanitize_rule(rule_name, self.latest_starlark_module.clone())
    }

    pub fn insert_task(&self, mut task: Task) -> anyhow::Result<()> {
        // update the rule name to have the starlark module name
        let rule_label = label::sanitize_rule(task.rule.name, self.latest_starlark_module.clone());
        task.rule.name = rule_label.clone();

        // update deps that refer to rules in the same starlark module
        if let Some(deps) = task.rule.deps.as_mut() {
            for dep in deps.iter_mut() {
                if label::is_rule_sanitized(dep) {
                    continue;
                }
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

    pub fn sort_tasks(&mut self, target: Option<Arc<str>>, phase: Phase) -> anyhow::Result<()> {
        let mut tasks = self.tasks.write();

        let setup_tasks = tasks
            .values()
            .filter(|task| task.rule.type_ == Some(RuleType::Setup))
            .cloned()
            .collect::<Vec<Task>>();

        self.graph.clear();
        // add all tasks to the graph
        for task in tasks.values() {
            self.graph.add_task(task.rule.name.clone());
        }

        let tasks_copy = tasks.clone();

        for task in tasks.values_mut() {
            // capture implicit dependencies based on inputs/outputs
            for other_task in tasks_copy.values() {
                // can't create a dependency on itself
                if task.rule.name == other_task.rule.name {
                    continue;
                }
                task.update_implicit_dependency(other_task);
            }

            // all non-setup tasks need to depend on the Setup tasks
            if task.rule.type_ != Some(RuleType::Setup) {
                for setup_task in setup_tasks.iter() {
                    if task.phase == setup_task.phase {
                        task.rule
                            .deps
                            .get_or_insert_with(Vec::new)
                            .push(setup_task.rule.name.clone());
                    }
                }
            }

            let task_phase = task.phase;
            if phase == Phase::Checkout && task_phase != Phase::Checkout {
                // skip evaluating non-checkout tasks during checkout
                continue;
            }

            // connect the dependencies
            let mut task_hasher = blake3::Hasher::new();
            task_hasher.update(task.calculate_digest().as_bytes());

            if let Some(deps) = task.rule.deps.clone() {
                for dep in deps {
                    let dep_task = tasks_copy.get(&dep).ok_or(format_error!(
                        "Task Depedency not found: {dep} specified by {}",
                        task.rule.name
                    ))?;

                    match task_phase {
                        Phase::Run => {
                            if dep_task.phase != Phase::Run {
                                return Err(format_error!(
                                    "Run task {} cannot depend on non-run task {}",
                                    task.rule.name,
                                    dep_task.rule.name
                                ));
                            }
                            if task.rule.type_ == Some(RuleType::Setup)
                                && dep_task.rule.type_ != Some(RuleType::Setup)
                            {
                                return Err(format_error!(
                                    "Setup task {} cannot depend on non-setup task {}",
                                    task.rule.name,
                                    dep_task.rule.name
                                ));
                            }
                        }
                        Phase::Checkout => {
                            if dep_task.phase != Phase::Checkout {
                                return Err(format_error!(
                                    "Checkout task {} cannot depend on non-checkout task {}",
                                    task.rule.name,
                                    dep_task.rule.name
                                ));
                            }
                        }
                        _ => {}
                    }

                    task_hasher.update(dep_task.calculate_digest().as_bytes());
                    task.add_signal_dependency(dep_task);
                    self.graph
                        .add_dependency(&task.rule.name, &dep)
                        .context(format_context!(
                            "Failed to add dependency {dep} to task {}",
                            task.rule.name
                        ))?;
                }
            }
            task.digest = task_hasher.finalize().to_string().into();
        }

        let target_is_some = target.is_some();

        self.sorted = self
            .graph
            .get_sorted_tasks(target)
            .context(format_context!("Failed to sort tasks"))?;

        if target_is_some {
            // enable any optional tasks in the graph
            for node_index in self.sorted.iter() {
                let task_name = self.graph.get_task(*node_index);
                let task = tasks
                    .get_mut(task_name)
                    .ok_or(format_error!("Task not found {task_name}"))?;
                if task.rule.type_ == Some(RuleType::Optional) {
                    task.rule.type_ = Some(RuleType::Run);
                }
            }
        }

        Ok(())
    }

    pub fn show_tasks(&self, printer: &mut printer::Printer) -> anyhow::Result<()> {
        let tasks = self.tasks.read();
        let mut task_info_list = std::collections::HashMap::new();
        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);
            let task = tasks
                .get(task_name)
                .ok_or(format_error!("Task not found {task_name}"))?;

            if printer.verbosity.level == printer::Level::Debug {
                printer.debug(task_name, &task)?;
            } else if printer.verbosity.level <= printer::Level::Message || task.rule.help.is_some()
            {
                task_info_list.insert(task.rule.name.clone(), task.rule.help.clone());
            }
        }

        printer.info("targets", &task_info_list)?;

        Ok(())
    }

    pub fn execute(
        &self,
        printer: &mut printer::Printer,
        workspace: workspace::WorkspaceArc,
        phase: Phase,
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
                let message = if task.rule.type_ == Some(RuleType::Optional) {
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
                    task.rule.name.as_ref(),
                    Some(100),
                    Some(message.as_str()),
                );

                logger::Logger::new_progress(&mut progress_bar, task_name.into())
                    .debug(format!("Staging task {}", task.rule.name).as_str());
                handle_list.push(task.execute(progress_bar, workspace.clone()));

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

        workspace
            .read()
            .save_inputs()
            .context(format_context!("Failed to save inputs"))?;
        workspace
            .write()
            .save_changes()
            .context(format_context!("while saving changes"))?;

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
        Err(format_error!("No starlark module set"))
    }
}

pub fn get_path_to_build_checkout(rule_name: Arc<str>) -> anyhow::Result<Arc<str>> {
    let state = get_state().read();
    let rule_name = state.get_sanitized_rule_name(rule_name);
    Ok(format!("build/{}", rule_name).into())
}
