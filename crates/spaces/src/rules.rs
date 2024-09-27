pub mod checkout;
pub mod io;
pub mod run;

use crate::{executor, label};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::sync::{Arc, Condvar, Mutex};

pub struct State {
    pub tasks: RwLock<HashMap<String, Task>>,
    pub graph: graph::Graph,
    pub sorted: Vec<petgraph::prelude::NodeIndex>,
    pub latest_starlark_module: Option<String>,
    pub all_modules: HashSet<String>,
}

impl State {
    pub fn get_sanitized_rule_name(&self, rule_name: &str) -> String {
        label::sanitize_rule(rule_name, self.latest_starlark_module.as_ref())
    }

    pub fn insert_task(&self, mut task: Task) -> anyhow::Result<()> {
        // update the rule name to have the starlark module name
        let rule_label = label::sanitize_rule(
            task.rule.name.as_str(),
            self.latest_starlark_module.as_ref(),
        );
        task.rule.name = rule_label.clone();

        // update deps that refer to rules in the same starlark module
        if let Some(deps) = task.rule.deps.as_mut() {
            for dep in deps.iter_mut() {
                if label::is_rule_sanitized(dep) {
                    continue;
                }
                *dep = label::sanitize_rule(dep.as_str(), self.latest_starlark_module.as_ref());
            }
        }

        let mut tasks = self.tasks.write().unwrap();
        if tasks.get(&rule_label).is_none() {
            tasks.insert(rule_label, task);
        }

        Ok(())
    }

    pub fn sort_tasks(&mut self, target: Option<String>) -> anyhow::Result<()> {
        let mut tasks = self.tasks.write().unwrap();

        for task in tasks.values() {
            self.graph.add_task(task.rule.name.clone());
        }
        let tasks_copy = tasks.clone();
        for task in tasks.values_mut() {
            // capture implicit dependencies based on inputs/outputs
            for other_task in tasks_copy.values() {
                if task.rule.name == other_task.rule.name {
                    continue;
                }
                task.update_implicit_dependency(other_task);
            }

            if let Some(deps) = task.rule.deps.clone() {
                for dep in deps {
                    let dep_task = tasks_copy
                        .get(&dep)
                        .ok_or(format_error!("Task Depedency not found {dep}"))?;

                    task.add_signal_dependency(dep_task);
                    self.graph
                        .add_dependency(&task.rule.name, &dep)
                        .context(format_context!(
                            "Failed to add dependency {dep} to task {}",
                            task.rule.name
                        ))?;
                }
            }
        }
        self.sorted = self
            .graph
            .get_sorted_tasks(target)
            .context(format_context!("Failed to sort tasks"))?;
        Ok(())
    }

    pub fn show_tasks(&self, printer: &mut printer::Printer) -> anyhow::Result<()> {
        let tasks = self.tasks.read().unwrap();
        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);
            let task = tasks
                .get(task_name)
                .ok_or(format_error!("Task not found {task_name}"))?;

            printer.info(task_name, &task)?;
        }

        Ok(())
    }

    pub fn execute(
        &self,
        printer: &mut printer::Printer,
        phase: Phase,
    ) -> anyhow::Result<executor::TaskResult> {
        let mut task_result = executor::TaskResult::new();
        let mut multi_progress = printer::MultiProgress::new(printer);
        let mut handle_list = Vec::new();

        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);
            let task = {
                let tasks = self.tasks.read().unwrap();

                tasks
                    .get(task_name)
                    .ok_or(format_error!("Task not found {task_name}"))?
                    .clone()
            };

            if task.phase == phase {
                let mut progress_bar = multi_progress.add_progress(
                    task.rule.name.as_str(),
                    Some(100),
                    Some("Complete"),
                );

                progress_bar.log(
                    printer::Level::Trace,
                    format!("Running task {}", task.rule.name).as_str(),
                );
                handle_list.push(task.execute(progress_bar));

                loop {
                    let mut number_running = 0;
                    for handle in handle_list.iter() {
                        if !handle.is_finished() {
                            number_running += 1;
                        }
                    }

                    // this can be configured with a another global starlark function
                    if number_running < 10 {
                        break;
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        }

        let mut first_error = None;
        for handle in handle_list {
            let handle_result = handle.join().unwrap();
            match handle_result {
                Ok(handle_task_result) => {
                    task_result.extend(handle_task_result);
                }
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }

        if let Some(err) = first_error {
            return Err(err);
        }

        Ok(task_result)
    }
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

pub fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    STATE.set(RwLock::new(State {
        tasks: RwLock::new(HashMap::new()),
        graph: graph::Graph::default(),
        sorted: Vec::new(),
        latest_starlark_module: None,
        all_modules: HashSet::new(),
    }));
    STATE.get()
}

pub fn get_checkout_path() -> anyhow::Result<String> {
    let state = get_state().read().unwrap();
    if let Some(latest) = state.latest_starlark_module.as_ref() {
        let path = std::path::Path::new(latest.as_str());
        let parent = path
            .parent()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(parent)
    } else {
        Err(format_error!("No starlark module set"))
    }
}

pub fn get_path_to_build_checkout(rule_name: &str) -> anyhow::Result<String> {
    let state = get_state().read().unwrap();
    let rule_name = state.get_sanitized_rule_name(rule_name);
    Ok(format!("build/{}", rule_name))
}

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
pub struct Rule {
    pub name: String,
    pub deps: Option<Vec<String>>,
    pub inputs: Option<HashSet<String>>,
    pub outputs: Option<HashSet<String>>,
    pub platforms: Option<Vec<platform::Platform>>,
    #[serde(rename = "type")]
    pub type_: Option<RuleType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Signal {
    ready: bool,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub executor: executor::Task,
    pub phase: Phase,
    pub rule: Rule,
    #[serde(skip)]
    signal: Arc<(Mutex<Signal>, Condvar)>,
    #[serde(skip)]
    deps_signals: Vec<Arc<(Mutex<Signal>, Condvar)>>,
}

impl Task {
    pub fn new(rule: Rule, phase: Phase, executor: executor::Task) -> Self {
        Task {
            executor,
            phase,
            signal: Arc::new((
                Mutex::new(Signal {
                    ready: false,
                    name: rule.name.clone(),
                }),
                Condvar::new(),
            )),
            deps_signals: Vec::new(),
            rule,
        }
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
    ) -> std::thread::JoinHandle<anyhow::Result<executor::TaskResult>> {
        let name = self.rule.name.clone();
        let executor = self.executor.clone();
        let signal = self.signal.clone();
        let rule = self.rule.clone();
        let deps_signals = self.deps_signals.clone();

        progress.set_message("Waiting for dependencies");

        std::thread::spawn(move || -> anyhow::Result<executor::TaskResult> {
            // check inputs/outputs to see if we need to run
            let mut is_execute = true;
            if let (Some(platforms), Some(current_platform)) =
                (rule.platforms.as_ref(), platform::Platform::get_platform())
            {
                is_execute = platforms.contains(&current_platform);
            }

            progress.log(
                printer::Level::Trace,
                format!("Execute {name} after platform check? {}", is_execute).as_str(),
            );

            let total = deps_signals.len();

            progress.log(
                printer::Level::Trace,
                format!("{name} has {} dependencies", total).as_str(),
            );

            let mut count = 1;
            for deps_signal in deps_signals {
                {
                    let (lock, _) = &*deps_signal;
                    let signal_access = lock.lock().unwrap();
                    progress.log(
                        printer::Level::Trace,
                        format!(
                            "{name} Waiting for dependency {} {count}/{total}",
                            signal_access.name
                        )
                        .as_str(),
                    );
                }
                loop {
                    let (lock, cvar) = &*deps_signal;
                    let signal_access = lock.lock().unwrap();
                    if !signal_access.ready {
                        let _ = cvar
                            .wait_timeout(signal_access, std::time::Duration::from_millis(100))
                            .unwrap();
                    } else {
                        break;
                    }
                    progress.increment_with_overflow(1);
                }
                count += 1;
            }

            progress.log(
                printer::Level::Trace,
                format!("{name} All dependencies are done").as_str(),
            );

            let is_optional_or_cancelled = {
                let state = get_state().read().unwrap();
                let tasks = state.tasks.read().unwrap();
                let task = tasks
                    .get(&name)
                    .context(format_context!("Task not found {name}"))?;
                if task.phase == Phase::Cancelled {
                    progress.log(
                        printer::Level::Message,
                        format!("Skipping {name} because it was cancelled").as_str(),
                    );
                    true
                } else if task.rule.type_ == Some(RuleType::Optional) {
                    progress.log(
                        printer::Level::Message,
                        format!("Skipping {name} because it is optional").as_str(),
                    );
                    true
                } else {
                    false
                }
            };

            if is_optional_or_cancelled {
                is_execute = false;
            }

            if !is_execute {
                progress.log(printer::Level::Message, format!("Skipping {name}").as_str());
            }

            progress.set_message(if is_execute { "Running" } else { "Skipping" });

            let task_result = if is_execute {
                executor
                    .execute(name.as_str(), progress)
                    .context(format_context!("Failed to exec {}", name))
            } else {
                Ok(executor::TaskResult::new())
            };

            // before notifying dependents process the enabled_targets list
            {
                let state = get_state().read().unwrap();
                let mut tasks = state.tasks.write().unwrap();
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
                    .get_mut(&name)
                    .context(format_context!("Task not found {name}"))?;
                task.phase = Phase::Complete;
            }

            let (lock, cvar) = &*signal;
            let mut signal_access = lock.lock().unwrap();
            signal_access.ready = true;
            cvar.notify_all();

            task_result
        })
    }

    pub fn add_signal_dependency(&mut self, task: &Task) {
        self.deps_signals.push(task.signal.clone());
    }
}
