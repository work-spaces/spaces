use crate::label::IsAnnotated;
use crate::workspace::WorkspaceArc;
use crate::{executor, label, singleton, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use utils::changes::glob;
use utils::rule::Visibility;
use utils::{environment, graph, lock, logger, platform, rule};

#[derive(Debug, Clone, PartialEq)]
pub enum HasHelp {
    No,
    Yes,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NeedsGraph {
    No,
    Yes(task::Phase),
}

fn rules_printer_logger(printer: &mut printer::Printer) -> logger::Logger<'_> {
    logger::Logger::new_printer(printer, "rules".into())
}

fn _rules_progress_logger(progress: &mut printer::MultiProgressBar) -> logger::Logger<'_> {
    logger::Logger::new_progress(progress, "rules".into())
}

fn task_logger(progress: &mut printer::MultiProgressBar, name: Arc<str>) -> logger::Logger<'_> {
    logger::Logger::new_progress(progress, name)
}

fn get_task_signal_deps(task: &task::Task) -> anyhow::Result<Vec<task::SignalArc>> {
    let state = get_state().read();
    let tasks = state.tasks.read();

    let mut result = Vec::new();
    if let Some(deps) = task.rule.deps.as_ref() {
        let all_rules = deps.collect_all_rules();
        for dep in all_rules.iter() {
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
        ) && !platforms.contains(&current_platform)
        {
            skip_execute_message = Some("Skipping: platform not enabled".into());
        }

        task_logger(&mut progress, name.clone()).debug(
            format!("Skip execute message after platform check? {skip_execute_message:?}").as_str(),
        );

        let deps_signals =
            get_task_signal_deps(&task).context(format_context!("Failed to get signal deps"))?;
        let total = deps_signals.len();

        task_logger(&mut progress, name.clone()).trace(format!("{total} dependencies").as_str());

        let mut count = 1;
        for deps_rule_signal in deps_signals {
            let signal_name = {
                let (lock, _) = &*deps_rule_signal.signal;
                let signal_access = lock.lock().unwrap();
                signal_access.name.clone()
            };

            task_logger(&mut progress, name.clone()).debug(
                format!("{name} Waiting for dependency {signal_name} {count}/{total}").as_str(),
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

        let has_dep_globs = task.rule.deps.as_ref().is_some_and(|d| d.has_globs());
        let updated_digest = if has_dep_globs {
            if skip_execute_message.is_some() {
                None
            } else {
                let dep_globs = task.rule.deps.as_ref().unwrap().collect_globs();

                task_logger(&mut progress, name.clone())
                    .debug("update workspace changes with deps globs");

                workspace
                    .write()
                    .update_changes(&mut progress, &dep_globs)
                    .context(format_context!("Failed to update workspace changes"))?;

                task_logger(&mut progress, name.clone()).debug("check for new digest");

                let seed = serde_json::to_string(&task.executor)
                    .context(format_context!("Failed to serialize"))?;
                let digest = workspace
                    .read()
                    .is_rule_inputs_changed(
                        &mut progress,
                        &rule_name,
                        seed.as_str(),
                        &dep_globs[..],
                    )
                    .context(format_context!(
                        "Failed to check deps globs for {rule_name}"
                    ))?;
                if digest.is_none() {
                    // the digest has not changed - not need to execute
                    skip_execute_message = Some("skipping: same deps globs".into());
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
            if task.rule.type_ == Some(rule::RuleType::Setup) {
                progress.set_ending_message_none();
            } else {
                progress.set_ending_message(message);
            }
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

        if task_result.is_ok()
            && let Some(digest) = updated_digest
        {
            workspace.write().update_rule_digest(&rule_name, digest);
        }

        {
            let mut log_status = LogStatus {
                name: rule_name.clone(),
                duration: elapsed_time,
                file: if skip_execute_message.is_some() {
                    "<skipped>".into()
                } else if singleton::get_is_logging_disabled() {
                    "<logging disabled>".into()
                } else {
                    workspace.read().get_log_file(&rule_name)
                },
                status: executor::exec::Expect::Success,
            };

            let state = get_state().read();
            let mut tasks = state.tasks.write();
            if task_result.is_ok() {
                log_status.status = executor::exec::Expect::Success;
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
    pub workspace_destinations: lock::StateLock<HashMap<Arc<str>, Arc<str>>>,
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

    fn sanitize_glob_hash_set(
        hash_set: &mut HashSet<Arc<str>>,
        is_annotated: IsAnnotated,
        rule_label: Arc<str>,
        latest_starlark_module: Option<Arc<str>>,
    ) -> anyhow::Result<()> {
        *hash_set = hash_set
            .drain()
            .map(|item| {
                label::sanitize_glob_value(
                    item.as_ref(),
                    is_annotated,
                    rule_label.as_ref(),
                    latest_starlark_module.clone(),
                )
                .context(format_context!("Failed to sanitize deps glob: {item}"))
            })
            .collect::<anyhow::Result<HashSet<Arc<str>>>>()?;

        Ok(())
    }

    pub fn insert_task(&self, mut task_to_insert: task::Task) -> anyhow::Result<()> {
        // update the rule name to have the starlark module name

        // don't insert tasks in lsp mode
        if singleton::is_lsp_mode() {
            return Ok(());
        }

        // check to see if the task will cause a conflict
        let workspace_destination = match &task_to_insert.executor {
            executor::Task::Git(repo) => Some(repo.spaces_key.clone()),
            executor::Task::AddAsset(asset) => Some(asset.destination.clone()),
            executor::Task::AddHardLink(asset) => Some(asset.destination.clone().into()),
            executor::Task::AddSoftLink(asset) => Some(asset.destination.clone().into()),
            executor::Task::AddWhichAsset(asset) => Some(asset.destination.clone().into()),
            // UpdateAsset by design will edit the same destination
            _ => None,
        };

        let rule_label = label::sanitize_rule(
            task_to_insert.rule.name,
            self.latest_starlark_module.clone(),
        );

        if let Some(ws_dest) = workspace_destination {
            if let Some(rule_name) = self.workspace_destinations.read().get(&ws_dest) {
                return Err(format_error!(
                    "The workspace destination `{ws_dest}` is already being used by rule `{rule_name}`"
                ));
            }
            let _ = self
                .workspace_destinations
                .write()
                .insert(ws_dest, rule_label.clone());
        }

        task_to_insert.rule.name = rule_label.clone();
        task_to_insert.signal = task::SignalArc::new(rule_label.clone());

        // Migrate any inputs into deps as Deps::Any with AnyDep::Glob
        task_to_insert.rule.sanitize();

        // update deps: sanitize rule names and glob hash sets
        if let Some(deps) = task_to_insert.rule.deps.as_mut() {
            match deps {
                rule::Deps::Rules(rules) => {
                    for dep in rules.iter_mut() {
                        if label::is_rule_sanitized(dep) {
                            continue;
                        }
                        *dep =
                            label::sanitize_rule(dep.clone(), self.latest_starlark_module.clone());
                    }
                }
                rule::Deps::Any(any_list) => {
                    for any_entry in any_list.iter_mut() {
                        match any_entry {
                            rule::AnyDep::Rule(dep) => {
                                if !label::is_rule_sanitized(dep) {
                                    *dep = label::sanitize_rule(
                                        dep.clone(),
                                        self.latest_starlark_module.clone(),
                                    );
                                }
                            }
                            rule::AnyDep::Glob(glob) => match glob {
                                rule::Globs::Includes(set) => {
                                    Self::sanitize_glob_hash_set(
                                        set,
                                        label::IsAnnotated::No,
                                        rule_label.clone(),
                                        self.latest_starlark_module.clone(),
                                    )?;
                                }
                                rule::Globs::Excludes(set) => {
                                    Self::sanitize_glob_hash_set(
                                        set,
                                        label::IsAnnotated::No,
                                        rule_label.clone(),
                                        self.latest_starlark_module.clone(),
                                    )?;
                                }
                            },
                        }
                    }
                }
            }
        }

        if let Some(Visibility::Rules(list)) = task_to_insert.rule.visibility.as_mut() {
            for vis_rule in list.iter_mut() {
                *vis_rule =
                    label::sanitize_rule(vis_rule.clone(), self.latest_starlark_module.clone());
            }
        }

        let mut tasks = self.tasks.write();

        if let Some(task) = tasks.get(&rule_label) {
            return Err(format_error!(
                "Rule already exists {rule_label} with {task:?}"
            ));
        } else {
            tasks.insert(rule_label, task_to_insert);
        }

        Ok(())
    }

    fn check_task_deps_visibility(&self, task: &task::Task) -> anyhow::Result<()> {
        let tasks = self.tasks.read();

        if let Some(deps) = task.rule.deps.as_ref() {
            let all_rules = deps.collect_all_rules();
            let task_path = label::get_path_from_label(task.rule.name.as_ref());
            for dep in all_rules.iter() {
                if let Some(dep_task) = tasks.get(dep) {
                    match dep_task.rule.visibility.as_ref() {
                        None | Some(rule::Visibility::Public) => {
                            // Do nothing if the dependency is public
                        }
                        Some(rule::Visibility::Rules(list)) => {
                            // are task and dep in the same repository
                            let mut is_match = false;
                            for prefix in list.iter() {
                                if task.rule.name.starts_with(prefix.as_ref()) {
                                    is_match = true;
                                    break;
                                }
                            }
                            if !is_match {
                                return Err(format_error!(
                                    "Dependency {} (rules) is NOT visible to {}.",
                                    dep_task.rule.name,
                                    task.rule.name
                                ));
                            }
                        }
                        Some(rule::Visibility::Private) => {
                            // are task and dep in the same module
                            if label::get_path_from_label(dep_task.rule.name.as_ref()) != task_path
                            {
                                return Err(format_error!(
                                    "Dependency {} (private) is NOT visible to {}.",
                                    dep_task.rule.name,
                                    task.rule.name
                                ));
                            }
                        }
                    }
                }
            }
        };

        Ok(())
    }

    pub fn update_dependency_graph(
        &mut self,
        printer: &mut printer::Printer,
        workspace: Option<WorkspaceArc>,
        phase: task::Phase,
    ) -> anyhow::Result<()> {
        {
            let tasks = self.tasks.read();
            for (name, task) in tasks.iter() {
                self.check_task_deps_visibility(task)
                    .with_context(|| format_context!("Visibility check failed for task {name}"))?;
            }
        }

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
                    let all_rules = deps.collect_all_rules();
                    for dep in all_rules.iter() {
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

        if let Some(workspace) = workspace
            && phase != task::Phase::Checkout
        {
            let mut workspace_write = workspace.write();
            rules_printer_logger(printer).debug("cloning graph to workspace bin settings");
            workspace_write.settings.bin.graph = self.graph.clone();
            workspace_write.is_bin_dirty = true;
        }

        Ok(())
    }

    pub fn update_target_dependency_graph(
        &mut self,
        printer: &mut printer::Printer,
        target: Option<Arc<str>>,
    ) -> anyhow::Result<()> {
        rules_printer_logger(printer)
            .debug(format!("sorting graph with for {target:?}...").as_str());
        self.sorted = self
            .graph
            .get_sorted_tasks(target.clone())
            .context(format_context!("Failed to sort tasks"))?;

        rules_printer_logger(printer)
            .debug(format!("done with {} nodes", self.sorted.len()).as_str());

        if let Some(target) = target {
            let mut tasks = self.tasks.write();

            // enable any optional tasks in the graph
            for node_index in self.sorted.iter() {
                let task_name = self.graph.get_task(*node_index);
                let task = tasks
                    .get_mut(task_name)
                    .ok_or(format_error!("Task not found {task_name}"))?;
                if singleton::is_skip_deps_mode() {
                    if target != task.rule.name {
                        task.rule.type_ = Some(rule::RuleType::Optional);
                    } else {
                        task.rule.type_ = Some(rule::RuleType::Run);
                    }
                } else if task.rule.type_ == Some(rule::RuleType::Optional) {
                    task.rule.type_ = Some(rule::RuleType::Run);
                }
            }
        }

        Ok(())
    }

    pub fn import_tasks_from_workspace_settings(
        &mut self,
        printer: &mut printer::Printer,
        workspace: workspace::WorkspaceArc,
        needs_graph: NeedsGraph,
    ) -> anyhow::Result<()> {
        {
            let workspace = workspace.read();
            let mut tasks = self.tasks.write();
            *tasks = serde_json::from_str(&workspace.settings.bin.tasks_json)
                .context(format_context!("Failed to parse tasks"))?;

            for task in tasks.values_mut() {
                task.signal = task::SignalArc::new(task.rule.name.clone());
            }

            rules_printer_logger(printer).debug("loading graph from workspace bin settings");

            self.graph = workspace.settings.bin.graph.clone();
        }
        if let NeedsGraph::Yes(phase) = needs_graph {
            // if the graph is empty, populate it with the tasks
            if self.graph.directed_graph.edge_count() == 0 {
                rules_printer_logger(printer).debug("bin settings graph is empty - updating");
                self.update_dependency_graph(printer, None, phase)
                    .context(format_context!("Failed to update dependency graph"))?;

                self.update_tasks_digests(printer, workspace.clone())
                    .context(format_context!("updating digests"))?;
            }
        }
        {
            let mut workspace = workspace.write();
            let env: environment::AnyEnvironment =
                serde_json::from_str(&workspace.settings.bin.env_json)
                    .context(format_context!("Failed to parse env"))?;
            workspace
                .update_env(env)
                .context(format_context!("Failed to load bin saved env"))?;
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

        rules_printer_logger(printer).debug(
            format!(
                "sorted {} tasks of {:?}",
                topo_sorted.len(),
                self.graph.directed_graph.capacity()
            )
            .as_str(),
        );
        let mut tasks = self.tasks.write();
        for node in topo_sorted.iter() {
            let task_name = self.graph.get_task(*node);
            let task = tasks.get(task_name).cloned();
            if let Some(task) = task {
                let mut task_hasher = blake3::Hasher::new();
                task_hasher.update(task.calculate_digest().as_bytes());
                let mut deps = task
                    .rule
                    .deps
                    .as_ref()
                    .map(|d| d.collect_all_rules())
                    .unwrap_or_default();
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
        workspace: WorkspaceArc,
        phase: task::Phase,
        _target: Option<Arc<str>>,
        filter: &HashSet<Arc<str>>,
        strip_prefix: Option<Arc<str>>,
    ) -> anyhow::Result<()> {
        let tasks = self.tasks.read();

        #[derive(Serialize)]
        struct TaskInfo {
            source: String,
            help: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            deps: Option<Vec<Arc<str>>>,
        }

        let mut task_info_list: HashMap<Arc<str>, _> = std::collections::HashMap::new();
        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);
            let globs = glob::Globs::new_with_includes(filter);

            if !filter.is_empty()
                && !globs.is_match(task_name.strip_prefix("//").unwrap_or(task_name))
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
                    if let Some(strip_prefix) = strip_prefix.as_ref()
                        && let Some(stripped) = task_name.strip_prefix(strip_prefix.as_ref())
                    {
                        task_name = stripped.strip_prefix("/").unwrap_or(stripped);
                    }

                    let deps = if printer.verbosity.level <= printer::Level::Message {
                        if let Some(task_deps) = &task.rule.deps {
                            let mut dep_strings: Vec<Arc<str>> = Vec::new();

                            // Collect rule names
                            for rule_name in task_deps.collect_all_rules() {
                                dep_strings.push(rule_name.clone());
                            }

                            // Collect expanded glob file paths
                            if task_deps.has_globs() {
                                let globs = task_deps.collect_globs();
                                let mut progress = printer::MultiProgress::new(printer);
                                let mut progress_bar = progress.add_progress(
                                    "inspecting deps globs",
                                    None,
                                    Some("Complete"),
                                );
                                let files = workspace
                                    .read()
                                    .inspect_inputs(&mut progress_bar, &globs)
                                    .context(format_context!("Failed to inspect deps globs"))?;
                                dep_strings.extend(files.into_iter().map(|e| e.into()));
                            }

                            if dep_strings.is_empty() {
                                None
                            } else {
                                Some(dep_strings)
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let source = label::get_source_from_label(task_name);
                    task_info_list.insert(task_name.into(), TaskInfo { help, source, deps });
                }
            }
        }

        printer.info(phase.to_string().as_str(), &task_info_list)?;

        Ok(())
    }

    fn export_tasks_as_mardown(&self, path: &str) -> anyhow::Result<()> {
        let tasks = self.tasks.read();

        let checkout_rules = tasks
            .values()
            .filter_map(|task| {
                if task.phase == task::Phase::Checkout {
                    Some((&task.rule, task.executor.to_markdown()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let run_rules = tasks
            .values()
            .filter_map(|task| {
                if task.phase == task::Phase::Run {
                    Some((&task.rule, task.executor.to_markdown()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let mut printer = printer::Printer::new_file(path)
            .context(format_context!("Failed to create file {path}"))?;
        let mut md_printer = printer::markdown::Markdown::new(&mut printer);
        let md = &mut md_printer;
        rule::Rule::print_markdown_section(md, "Checkout Rules", &checkout_rules, false, false)?;
        rule::Rule::print_markdown_section(md, "Run Rules", &run_rules, true, true)?;
        Ok(())
    }

    fn get_run_targets(&self, has_help: HasHelp) -> anyhow::Result<Vec<Arc<str>>> {
        let tasks = self.tasks.read();

        let run_rules = tasks
            .values()
            .filter_map(|task| {
                if task.phase == task::Phase::Run {
                    if has_help == HasHelp::Yes {
                        if task.rule.help.is_some() {
                            Some(task.rule.name.clone())
                        } else {
                            None
                        }
                    } else {
                        Some(task.rule.name.clone())
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        Ok(run_rules)
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
                        format!("{rule_type:?}")
                    } else {
                        format!("{phase:?}")
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
        workspace_destinations: lock::StateLock::new(HashMap::new()),
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
    Ok(format!("build/{rule_name}").into())
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

pub fn get_latest_starlark_module() -> Option<Arc<str>> {
    let state = get_state().read();
    state.latest_starlark_module.clone()
}

pub fn set_latest_starlark_module(name: Arc<str>) {
    let mut state = get_state().write();
    state.latest_starlark_module = Some(name.clone());
    state.all_modules.insert(name);
}

pub fn show_tasks(
    printer: &mut printer::Printer,
    workspace: WorkspaceArc,
    phase: task::Phase,
    target: Option<Arc<str>>,
    filter: &HashSet<Arc<str>>,
    strip_prefix: Option<Arc<str>>,
) -> anyhow::Result<()> {
    let state = get_state().read();
    state.show_tasks(printer, workspace, phase, target, filter, strip_prefix)
}

pub fn get_run_targets(has_help: HasHelp) -> anyhow::Result<Vec<Arc<str>>> {
    let state = get_state().read();
    state.get_run_targets(has_help)
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
    workspace: Option<workspace::WorkspaceArc>,
    phase: task::Phase,
) -> anyhow::Result<()> {
    let mut state = get_state().write();
    state.update_dependency_graph(printer, workspace, phase)
}

pub fn update_target_dependency_graph(
    printer: &mut printer::Printer,
    target: Option<Arc<str>>,
) -> anyhow::Result<()> {
    let mut state = get_state().write();
    state.update_target_dependency_graph(printer, target)
}

pub fn import_tasks_from_workspace_settings(
    printer: &mut printer::Printer,
    workspace: workspace::WorkspaceArc,
    needs_graph: NeedsGraph,
) -> anyhow::Result<()> {
    let mut state = get_state().write();
    state.import_tasks_from_workspace_settings(printer, workspace, needs_graph)
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
                .get_or_insert_with(|| rule::Deps::Rules(Vec::new()))
                .push_rule(rule::SETUP_RULE_NAME.into());
        }
    }
    Ok(())
}

fn get_rules_by_type(rule_type: rule::RuleType) -> rule::Deps {
    let state = get_state().read();
    let tasks = state.tasks.read();
    rule::Deps::Any(
        tasks
            .values()
            .filter(|task| task.rule.type_ == Some(rule_type))
            .map(|task| rule::AnyDep::Rule(task.rule.name.clone()))
            .collect(),
    )
}

pub fn get_setup_rules() -> rule::Deps {
    get_rules_by_type(rule::RuleType::Setup)
}

pub fn get_test_rules() -> rule::Deps {
    get_rules_by_type(rule::RuleType::Test)
}

pub fn get_pre_commit_rules() -> rule::Deps {
    get_rules_by_type(rule::RuleType::PreCommit)
}

pub fn get_clean_rules() -> rule::Deps {
    get_rules_by_type(rule::RuleType::Clean)
}

pub fn is_git_rule(name: &str) -> bool {
    let state = get_state().read();
    let tasks = state.tasks.read();
    if let Some(task) = tasks.get(name) {
        matches!(task.executor, executor::Task::Git(_))
    } else {
        for task in tasks.values() {
            if task.rule.name.ends_with(name) && matches!(task.executor, executor::Task::Git(_)) {
                return true;
            }
        }
        false
    }
}

pub fn export_log_status(workspace: WorkspaceArc) -> anyhow::Result<()> {
    let state = get_state().read();
    let log_status = state.log_status.read().clone();
    let log_output_folder = workspace.read().log_directory.clone();
    let log_status_file_output = format!("{log_output_folder}/log_status.json");
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
        if let Some(task) = state.tasks.read().get(task_name)
            && task.phase == phase
        {
            rules_printer_logger(printer).debug(format!("Queued task {task_name}").as_str());
        }
    }
    Ok(())
}
