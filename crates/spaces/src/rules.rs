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
    pub tasks: HashMap<String, Task>,
    pub graph: graph::Graph,
    pub sorted: Vec<petgraph::prelude::NodeIndex>,
    pub latest_starlark_module: Option<String>,
    pub all_modules: HashSet<String>,
}

impl State {
    pub fn get_sanitized_rule_name(&self, rule_name: &str) -> String {
        label::sanitize_rule(rule_name, self.latest_starlark_module.as_ref())
    }

    pub fn insert_task(&mut self, mut task: Task) -> anyhow::Result<()> {
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

        if self.tasks.get(&rule_label).is_none() {
            self.tasks.insert(rule_label, task);
        }

        Ok(())
    }

    pub fn sort_tasks(&mut self, target: Option<String>) -> anyhow::Result<()> {
        for task in self.tasks.values() {
            self.graph.add_task(task.rule.name.clone());
        }
        let tasks_copy = self.tasks.clone();
        for task in self.tasks.values_mut() {
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
        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);
            let task = self
                .tasks
                .get(task_name)
                .ok_or(format_error!("Task not found {task_name}"))?;

            printer.info(task_name, &task)?;
        }

        Ok(())
    }

    pub fn execute(
        &mut self,
        printer: &mut printer::Printer,
        phase: Phase,
    ) -> anyhow::Result<Vec<String>> {
        let mut new_modules = Vec::new();
        let mut multi_progress = printer::MultiProgress::new(printer);
        let mut handle_list = Vec::new();

        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);
            let task = self
                .tasks
                .get_mut(task_name)
                .ok_or(format_error!("Task not found {task_name}"))?;

            if task.phase == phase {
                let progress_bar = multi_progress.add_progress(task.rule.name.as_str(), Some(200), None);
                handle_list.push(task.execute(progress_bar));
                task.phase = Phase::Complete;

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
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
            }
        }

        for handle in handle_list {
            new_modules.extend(
                handle
                    .join()
                    .unwrap()
                    .context(format_context!("task failed"))?,
            );
        }

        Ok(new_modules)
    }
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

pub fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    STATE.set(RwLock::new(State {
        tasks: HashMap::new(),
        graph: graph::Graph::new(),
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
            .unwrap_or(String::new());
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub deps: Option<Vec<String>>,
    pub inputs: Option<HashSet<String>>,
    pub outputs: Option<HashSet<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub executor: executor::Task,
    pub phase: Phase,
    pub rule: Rule,
    #[serde(skip)]
    signal: Arc<(Mutex<bool>, Condvar)>,
    #[serde(skip)]
    deps_signals: Vec<Arc<(Mutex<bool>, Condvar)>>,
}

impl Task {
    pub fn new(rule: Rule, phase: Phase, executor: executor::Task) -> Self {
        Task {
            executor,
            phase,
            signal: Arc::new((Mutex::new(false), Condvar::new())),
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
    ) -> std::thread::JoinHandle<anyhow::Result<Vec<String>>> {
        let name = self.rule.name.clone();
        let executor = self.executor.clone();
        let signal = self.signal.clone();
        let rule = self.rule.clone();
        let deps_signals = self.deps_signals.clone();

        std::thread::spawn(move || -> anyhow::Result<Vec<String>> {
            // check inputs/outputs to see if we need to run
            let mut is_stale;
            if let Some(inputs) = rule.inputs.as_ref() {
                let mut state = io::get_state().write().unwrap();
                is_stale = inputs.len() == 0;
                for input in inputs {
                    if state
                        .io
                        .update(input.as_str())
                        .context(format_context!("while updating io"))?
                        == io::IsUpdated::Yes
                    {
                        is_stale = true;
                    }
                }
            } else {
                is_stale = true;
            }

            // if there are no inputs, always run

            progress.set_message("Waiting for dependencies");
            for deps_signal in deps_signals {
                loop {
                    let (lock, cvar) = &*deps_signal;
                    let done = lock.lock().unwrap();
                    if !*done {
                        let _ = cvar
                            .wait_timeout(done, std::time::Duration::from_millis(50))
                            .unwrap();
                    } else {
                        break;
                    }
                    progress.increment_with_overflow(1);
                }
            }
            progress.set_message("Running");

            let new_modules = if is_stale {
                executor
                    .execute(name.as_str(), progress)
                    .context(format_context!("Failed to exec {}", name))?
            } else {
                Vec::new()
            };

            let (lock, cvar) = &*signal;
            let mut done = lock.lock().unwrap();
            *done = true;
            cvar.notify_all();
            Ok(new_modules)
        })
    }

    pub fn add_signal_dependency(&mut self, task: &Task) {
        self.deps_signals.push(task.signal.clone());
    }
}
