pub mod checkout;
pub mod run;

use crate::executor;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::sync::{Arc, Condvar, Mutex};

pub struct State {
    pub tasks: HashMap<String, Task>,
    pub graph: graph::Graph,
    pub sorted: Vec<petgraph::prelude::NodeIndex>,
}

impl State {

    pub fn sort_tasks(&mut self) -> anyhow::Result<()> {
        for task in self.tasks.values() {
            self.graph.add_task(task.name.clone());
        }
        let tasks_copy = self.tasks.clone();
        for task in self.tasks.values_mut() {
            // capture implicit dependencies based on inputs/outputs
            for other_task in tasks_copy.values() {
                if task.name == other_task.name {
                    continue;
                }
                task.update_implicit_dependency(other_task);
            }

            let deps = task.deps.clone();
            for dep in deps {
                let dep_task = tasks_copy
                    .get(&dep)
                    .ok_or(format_error!("Task Depedency not found {dep}"))?;

                task.add_signal_dependency(dep_task);
                self.graph
                    .add_dependency(&task.name, &dep)
                    .context(format_context!(
                        "Failed to add dependency {dep} to task {}",
                        task.name
                    ))?;
            }
        }
        self.sorted = self.graph.get_sorted_tasks();
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
                let progress_bar = multi_progress.add_progress(task.name.as_str(), None, None);
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
    }));
    STATE.get()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Checkout,
    PostCheckout,
    Run,
    Complete
}

#[derive(Debug, Clone)]
pub struct Task {
    pub executor: executor::Task,
    pub phase: Phase,
    pub name: String,
    pub _source: String,
    pub deps: Vec<String>,
    pub _inputs: HashSet<String>,
    pub _outputs: HashSet<String>,
    signal: Arc<(Mutex<bool>, Condvar)>,
    deps_signals: Vec<Arc<(Mutex<bool>, Condvar)>>,
}

impl Task {
    pub fn new(
        name: &str,
        phase: Phase,
        deps: Vec<String>,
        inputs: HashSet<String>,
        outputs: HashSet<String>,
        executor: executor::Task,
    ) -> Self {
        Task {
            executor,
            name: name.to_string(),
            phase,
            _source: "".to_string(),
            signal: Arc::new((Mutex::new(false), Condvar::new())),
            deps_signals: Vec::new(),
            _inputs: HashSet::new(),
            _outputs: HashSet::new(),
            deps,
        }
    }

    pub fn update_implicit_dependency(&mut self, other_task: &Task) {
        if self.deps.contains(&other_task.name) {
            return;
        }
        for input in &self._inputs {
            if other_task._outputs.contains(input) {
                self.deps.push(other_task.name.clone());
                return;
            }
        }
    }

    pub fn execute(
        &self,
        mut progress: printer::MultiProgressBar,
    ) -> std::thread::JoinHandle<anyhow::Result<Vec<String>>> {
        let name = self.name.clone();
        let executor = self.executor.clone();
        let signal = self.signal.clone();
        let deps_signals = self.deps_signals.clone();

        std::thread::spawn(move || -> anyhow::Result<Vec<String>> {
            // check inputs/outputs to see if we need to run

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
                    progress.increment(1);
                }
            }
            progress.set_message("Running");

            let new_modules = executor
                .execute(name.as_str(), progress)
                .context(format_context!("Failed to exec {}", name))?;

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

pub fn list_to_vec(list: Option<&starlark::values::list::ListRef>) -> Vec<String> {
    if let Some(list) = list {
        return list.iter().map(|v| v.to_str().to_string()).collect();
    }
    Vec::new()
}

pub fn list_to_hashset(list: Option<&starlark::values::list::ListRef>) -> HashSet<String> {
    if let Some(list) = list {
        return list.iter().map(|v| v.to_str().to_string()).collect();
    }
    HashSet::new()
}
