use crate::workspace::WorkspaceArc;
use crate::{executor, singleton, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use utils::{changes::glob, targets};

use utils::{environment, graph, labels, lock, logger, logs, platform, rcache, rule, ws};

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

fn rules_printer_logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "rules".into())
}

fn _rules_progress_logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "rules".into())
}

fn task_logger(console: console::Console, name: Arc<str>) -> logger::Logger {
    logger::Logger::new(console, name)
}

fn get_task_signal_deps(task: &task::Task) -> anyhow::Result<Vec<task::SignalArc>> {
    let state = get_state().read();
    let tasks = state.tasks.read();

    let mut result = Vec::new();
    let rule_deps = task.collect_rule_deps();
    for dep in rule_deps.iter() {
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
    Ok(result)
}

pub fn execute_rule(
    mut progress: console::Progress,
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

    let console = progress.console.clone();

    progress.set_message(format!("Waiting for dependencies ({:?})", task.phase).as_str());

    std::thread::spawn(move || -> anyhow::Result<executor::TaskResult> {
        // check inputs/outputs to see if we need to run
        let name = task.rule.name.clone();
        let logger = task_logger(console.clone(), name.clone());

        // when this goes out of scope it will notify the dependents
        let _signal_on_drop = SignalOnDrop {
            signal: task.signal.clone(),
        };

        let displayed_rule = utils::labels::sanitize_rule_for_display(name.clone());
        let mut skip_execute_message: Vec<console::Line> = Vec::new();
        if let (Some(platforms), Some(current_platform)) = (
            task.rule.platforms.as_ref(),
            platform::Platform::get_platform(),
        ) && !platforms.contains(&current_platform)
        {
            skip_execute_message = logger::make_finalize_line(
                logger::FinalType::NotPlatform,
                None,
                displayed_rule.as_ref(),
            );
        }

        logger.debug(
            format!("Skip execute message after platform check? {skip_execute_message:?}").as_str(),
        );

        let deps_signals =
            get_task_signal_deps(&task).context(format_context!("Failed to get signal deps"))?;
        let total = deps_signals.len();

        logger.trace(format!("{total} dependencies").as_str());

        let mut count = 1;
        for deps_rule_signal in deps_signals {
            let signal_name = {
                let (lock, _) = &*deps_rule_signal.signal;
                let signal_access = lock.lock().unwrap();
                signal_access.name.clone()
            };

            logger.debug(
                format!("{name} Waiting for dependency {signal_name} {count}/{total}").as_str(),
            );

            deps_rule_signal.wait_is_ready(std::time::Duration::from_millis(100));
            count += 1;
        }

        logger.debug(format!("{name} All dependencies are done").as_str());

        {
            logger.debug(format!("{name} check for skipping/cancelation").as_str());
            let state = get_state().read();
            let tasks = state.tasks.read();
            let task = tasks
                .get(name.as_ref())
                .context(format_context!("Task not found {name}"))?;
            if task.phase == task::Phase::Cancelled {
                logger.debug(format!("Skipping {name}: cancelled").as_str());
                skip_execute_message = logger::make_finalize_line(
                    logger::FinalType::Cancelled,
                    None,
                    displayed_rule.as_ref(),
                );
            } else if skip_execute_message.is_empty()
                && task.rule.type_ == Some(rule::RuleType::Optional)
            {
                logger.debug("Skipping because it is optional");
                skip_execute_message = logger::make_finalize_line(
                    logger::FinalType::NotRequired,
                    None,
                    displayed_rule.as_ref(),
                );
            }
            logger.trace(format!("{name} done checking skip cancellation").as_str());
        }

        let rule_name = name.clone();

        let dep_globs = {
            let state = get_state().read();
            let tasks = state.tasks.read();
            task.collects_glob_deps(&tasks)
        };

        let updated_digest = if !dep_globs.is_empty() && skip_execute_message.is_empty() {
            logger
                .debug(format!("update workspace changes with deps globs {dep_globs:?}").as_str());

            workspace
                .write()
                .update_changes(&mut progress, &dep_globs)
                .context(format_context!(
                    "[{rule_name}] Failed to update workspace changes"
                ))?;

            logger.debug("check for new digest");

            let check_changes = workspace
                .read()
                .is_rule_deps_changed(
                    &mut progress,
                    &rule_name,
                    task.digest.as_ref(),
                    &dep_globs[..],
                )
                .context(format_context!("[{rule_name}] Failed to check deps globs"))?;

            // digest has not changed
            if !check_changes.is_changed {
                if task.rule.uses_rule_cache() {
                    // always run the rule cache even if inputs
                    // are the same, rule cache will restore targets
                    // if the user has manually deleted them
                    Some(check_changes.digest)
                } else {
                    skip_execute_message = logger::make_finalize_line(
                        logger::FinalType::NoChanges,
                        None,
                        displayed_rule.as_ref(),
                    );

                    None
                }
            } else {
                let digest = check_changes.digest;
                logger.debug(format!("New digest for {rule_name}={digest}").as_str());
                Some(digest)
            }
        } else {
            None
        };

        if skip_execute_message.is_empty() {
            logger.debug("Running task");
            progress.set_message("Running");
        }

        // time how long it takes to execute the task
        let start_time = std::time::Instant::now();

        let effective_rule_digest: Arc<str> = updated_digest
            .clone()
            .unwrap_or_else(|| task.digest.clone());

        let mut cache_status = workspace::CacheStatus::None;
        progress.reset_elapsed();
        let (did_complete, task_result) = if !skip_execute_message.is_empty() {
            if task.rule.uses_rule_cache()
                && let Some(digest) = updated_digest.clone()
            {
                cache_status = workspace::CacheStatus::Skipped(digest);
            }

            if task.rule.type_ == Some(rule::RuleType::Setup) {
                progress.set_finalize_none();
            } else {
                progress.set_finalize_lines(skip_execute_message);
            }
            (false, Ok(executor::TaskResult::new()))
        } else {
            let cache_path = {
                let store_path = workspace.read().get_store_path();
                ws::get_rcache_path(std::path::Path::new(store_path.as_ref()))
            };
            if task.rule.uses_rule_cache()
                && let Some(targets) = task.rule.targets.as_ref()
            {
                // if the rule defines targets, the rule is run through
                // the rule cache engine

                logger.debug(format!("rcache digest {effective_rule_digest}").as_str());

                let task_result_option = rcache::execute(
                    cache_path.as_ref(),
                    effective_rule_digest.clone(),
                    targets.as_slice(),
                    || {
                        task.executor
                            .execute(&mut progress, workspace.clone(), &rule_name)
                            .context(format_context!("[{rule_name}] Failed to exec"))
                    },
                    || task.rule.get_target_paths(),
                );
                match task_result_option {
                    Some(Ok(result)) => {
                        cache_status =
                            workspace::CacheStatus::Executed(effective_rule_digest.clone());
                        (true, Ok(result))
                    }
                    Some(Err(err)) => {
                        cache_status =
                            workspace::CacheStatus::Executed(effective_rule_digest.clone());
                        (
                            false,
                            Err(err)
                                .context(format_context!("[{rule_name}] while executing/caching")),
                        )
                    }
                    None => {
                        cache_status =
                            workspace::CacheStatus::Restored(effective_rule_digest.clone());
                        (false, Ok(executor::TaskResult::new()))
                    }
                }
            } else {
                (
                    true,
                    task.executor
                        .execute(&mut progress, workspace.clone(), &rule_name)
                        .context(format_context!("[{rule_name}] Failed to exec")),
                )
            }
        };

        let elapsed_time = start_time.elapsed();
        workspace
            .write()
            .update_rule_metrics(&rule_name, elapsed_time, cache_status.clone());

        if task_result.is_ok() {
            if did_complete {
                progress.set_finalize_lines(logger::make_finalize_line(
                    logger::FinalType::Completed,
                    Some(elapsed_time),
                    displayed_rule.as_ref(),
                ));
            }

            if let Some(digest) = updated_digest {
                workspace.write().update_rule_digest(&rule_name, digest);
            }
        }

        {
            let mut log_status = logs::Status {
                name: rule_name.clone(),
                duration: elapsed_time,
                file: if !did_complete {
                    "<skipped>".into()
                } else if matches!(cache_status, workspace::CacheStatus::Restored(_)) {
                    "<restored>".into()
                } else if singleton::get_is_logging_disabled() {
                    "<logging disabled>".into()
                } else {
                    workspace.read().get_log_file(&rule_name)
                },
                status: logs::Expect::Success,
                cache_status,
            };

            let state = get_state().read();
            let mut tasks = state.tasks.write();
            if task_result.is_ok() {
                log_status.status = logs::Expect::Success;
            } else {
                log_status.status = logs::Expect::Failure;
                progress.set_finalize_lines(logger::make_finalize_line(
                    logger::FinalType::Failed,
                    None,
                    displayed_rule.as_ref(),
                ));

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

#[derive(Debug)]
pub struct State {
    pub tasks: lock::StateLock<HashMap<Arc<str>, task::Task>>,
    pub workspace_destinations: lock::StateLock<HashMap<Arc<str>, Arc<str>>>,
    pub graph: graph::Graph,
    pub sorted: Vec<petgraph::prelude::NodeIndex>,
    pub default_module_visibility: rule::Visibility,
    pub all_modules: HashSet<Arc<str>>,
    pub log_status: lock::StateLock<Vec<logs::Status>>,
}

impl State {
    pub fn insert_task(&self, task: task::Task) -> anyhow::Result<()> {
        self.insert_task_with_context(task, &Arc::from(""), self.default_module_visibility.clone())
    }

    /// Insert a task using an explicitly supplied module name and default
    /// visibility.  This variant is called from builtin functions that carry an
    /// `EvalContext`, allowing multiple modules to be evaluated in parallel
    /// without races on `latest_starlark_module`.
    pub fn insert_task_with_context(
        &self,
        mut task_to_insert: task::Task,
        module_name: &Arc<str>,
        default_visibility: rule::Visibility,
    ) -> anyhow::Result<()> {
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

        let module_opt = if module_name.is_empty() {
            None
        } else {
            Some(module_name.clone())
        };

        let rule_label = labels::sanitize_rule(
            task_to_insert.rule.name,
            module_opt.clone(),
            workspace::SPACES_MODULE_NAME,
            labels::IsDep::No,
        );

        if let Some(ws_dest) = workspace_destination {
            let mut workspace_destinations = self.workspace_destinations.write();
            if let Some(rule_name) = workspace_destinations.get(&ws_dest) {
                return Err(format_error!(
                    "The workspace destination `{ws_dest}` is already being used by rule `{rule_name}`"
                ));
            }
            let _ = workspace_destinations.insert(ws_dest, rule_label.clone());
        }

        task_to_insert.rule.name = rule_label.clone();
        task_to_insert.signal = task::SignalArc::new(rule_label.clone());

        // Apply default visibility when the rule has no explicit visibility
        if task_to_insert.rule.visibility.is_none() {
            task_to_insert.rule.visibility = Some(default_visibility);
        }

        task_to_insert
            .rule
            .sanitize(
                rule_label.clone(),
                module_opt,
                workspace::SPACES_MODULE_NAME,
            )
            .context(format_context!("while sanitizing rule {rule_label}"))?;

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

        let rule_deps = task.collect_rule_deps();
        let task_path = labels::get_path_label_from_rule_label(task.rule.name.as_ref());
        for dep in rule_deps.iter() {
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
                        if labels::get_path_label_from_rule_label(dep_task.rule.name.as_ref())
                            != task_path
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

        Ok(())
    }

    pub fn update_dependency_graph(
        &mut self,
        console: console::Console,
        workspace: Option<WorkspaceArc>,
        phase: task::Phase,
    ) -> anyhow::Result<()> {
        let logger = rules_printer_logger(console.clone());
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
        logger.debug(format!("Adding {} tasks to graph", tasks.len()).as_str());
        for task in tasks.values() {
            self.graph.add_task(task.rule.name.clone());
        }

        logger.debug("Adding deps to graph tasks");

        {
            let start_time = std::time::Instant::now();
            let mut progress: Option<console::Progress> = None;
            for task in tasks.values_mut() {
                let now = std::time::Instant::now();
                if now.duration_since(start_time).as_millis() > 100 {
                    if let Some(progress) = progress.as_mut() {
                        progress.increment_with_overflow(1);
                        progress.set_message("populating dependency graph");
                    } else {
                        progress = Some(console::Progress::new(
                            console.clone(),
                            "workspace",
                            Some(200),
                            None,
                        ));
                    }
                }

                let task_phase = task.phase;
                if phase == task::Phase::Checkout && task_phase != task::Phase::Checkout {
                    // skip evaluating non-checkout tasks during checkout
                    continue;
                }

                // connect the dependencies
                let all_rules = task.collect_rule_deps();
                for rule_dep in all_rules.iter() {
                    self.graph
                        .add_dependency(&task.rule.name, rule_dep)
                        .context(format_context!(
                            "Failed to add dependency {rule_dep} to task {}: {}",
                            task.rule.name,
                            self.graph.get_target_not_found(rule_dep.clone())
                        ))?;
                }
            }
        }

        if let Some(workspace) = workspace
            && phase != task::Phase::Checkout
        {
            let mut workspace_write = workspace.write();
            logger.debug("cloning graph to workspace bin settings");
            workspace_write.settings.bin.graph = self.graph.clone();
            workspace_write.is_bin_dirty = true;
        }

        Ok(())
    }

    pub fn update_target_dependency_graph(
        &mut self,
        console: console::Console,
        target: Option<Arc<str>>,
    ) -> anyhow::Result<()> {
        let logger = rules_printer_logger(console.clone());
        logger.debug(format!("sorting graph with for {target:?}...").as_str());
        self.sorted = self
            .graph
            .get_sorted_tasks(target.clone())
            .context(format_context!("Failed to sort tasks"))?;

        logger.debug(format!("done with {} nodes", self.sorted.len()).as_str());

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
        console: console::Console,
        workspace: workspace::WorkspaceArc,
        needs_graph: NeedsGraph,
    ) -> anyhow::Result<()> {
        let logger = rules_printer_logger(console.clone());
        {
            let workspace = workspace.read();
            let mut tasks = self.tasks.write();
            *tasks = serde_json::from_str(&workspace.settings.bin.tasks_json)
                .context(format_context!("Failed to parse tasks"))?;

            for task in tasks.values_mut() {
                task.signal = task::SignalArc::new(task.rule.name.clone());
            }

            logger.debug("loading graph from workspace bin settings");

            self.graph = workspace.settings.bin.graph.clone();
        }
        if let NeedsGraph::Yes(phase) = needs_graph {
            // if the graph is empty, populate it with the tasks
            if self.graph.directed_graph.edge_count() == 0 {
                logger.debug("bin settings graph is empty - updating");
                self.update_dependency_graph(console.clone(), None, phase)
                    .context(format_context!("Failed to update dependency graph"))?;

                self.update_tasks_digests(console.clone(), workspace.clone())
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

    fn validate_one_rule_per_target(&self) -> anyhow::Result<()> {
        let tasks = self.tasks.read();

        // Map each target path to the rule that owns it.
        let mut file_map: HashMap<Arc<str>, Arc<str>> = HashMap::new();
        let mut dir_map: HashMap<Arc<str>, Arc<str>> = HashMap::new();

        for (rule_name, task) in tasks.iter() {
            for target in task.rule.targets.iter().flatten() {
                match target {
                    targets::Target::File(file) => {
                        if let Some(existing_rule) =
                            file_map.insert(file.clone(), rule_name.clone())
                        {
                            return Err(format_error!(
                                "Target `{file}` is claimed by both rule `{existing_rule}` and rule `{rule_name}`",
                            ));
                        };
                    }
                    targets::Target::Directory(file) => {
                        if let Some(existing_rule) = dir_map.insert(file.clone(), rule_name.clone())
                        {
                            return Err(format_error!(
                                "Target `{file}` is claimed by both rule `{existing_rule}` and rule `{rule_name}`",
                            ));
                        };
                    }
                }
            }
        }

        for (file_path_label, file_rule) in file_map.iter() {
            for (dir_path_label, dir_rule) in dir_map.iter() {
                let dir_prefix = if dir_path_label.ends_with('/') {
                    dir_path_label.to_string()
                } else {
                    format!("{dir_path_label}/")
                };
                if file_path_label.starts_with(dir_prefix.as_str()) {
                    return Err(format_error!(
                        "Target `{file_path_label}` from {file_rule} is contained in target {dir_path_label} from {dir_rule}",
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn update_tasks_digests(
        &self,
        console: console::Console,
        workspace: WorkspaceArc,
    ) -> anyhow::Result<()> {
        let logger = rules_printer_logger(console.clone());
        if !workspace.read().is_dirty {
            return Ok(());
        }
        logger.info("sorting and hashing");
        let topo_sorted = self
            .graph
            .get_sorted_tasks(None)
            .context(format_context!("Failed to sort tasks for phase digesting",))?;

        logger.debug(
            format!(
                "sorted {} tasks of {:?}",
                topo_sorted.len(),
                self.graph.directed_graph.capacity()
            )
            .as_str(),
        );
        {
            let mut tasks = self.tasks.write();
            for node in topo_sorted.iter() {
                let task_name = self.graph.get_task(*node);
                let task = tasks.get(task_name).cloned();
                if let Some(task) = task {
                    let mut task_hasher = blake3::Hasher::new();
                    task_hasher.update(task.calculate_digest().as_bytes());
                    let mut rule_deps = task.collect_rule_deps();
                    rule_deps.sort();
                    for dep in rule_deps {
                        if let Some(dep_task) = tasks.get(&dep) {
                            task_hasher.update(dep_task.digest.as_bytes());
                        }
                    }
                    if let Some(task_mut) = tasks.get_mut(task_name) {
                        task_mut.digest = task_hasher.finalize().to_string().into();
                    }
                }
            }
        }

        self.validate_one_rule_per_target()
            .context(format_context!("While checking for one rule per target"))?;

        let serde_tasks = self.tasks.read().clone();
        workspace.write().settings.bin.tasks_json = serde_json::to_string(&serde_tasks)
            .map_err(|e| format_error!("Failed to encode {e}"))?
            .into();

        Ok(())
    }

    pub fn show_tasks(
        &self,
        console: console::Console,
        workspace: WorkspaceArc,
        phase: task::Phase,
        filter: &HashSet<Arc<str>>,
        strip_prefix: Option<Arc<str>>,
        fuzzy_query: Option<&str>,
    ) -> anyhow::Result<()> {
        let tasks = self.tasks.read();

        #[derive(Serialize)]
        struct TaskInfo {
            source: String,
            help: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            deps: Option<Vec<Arc<str>>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            targets: Option<Vec<Arc<str>>>,
        }

        // When fuzzy matching, collect (score, task_name) pairs so we can
        // keep only the top matches.
        struct ScoredTask {
            score: isize,
            name: Arc<str>,
            info: TaskInfo,
        }
        let mut scored_tasks: Vec<ScoredTask> = Vec::new();
        let mut task_info_list: HashMap<Arc<str>, _> = std::collections::HashMap::new();
        let console_level = console.get_level();

        let glob_logger = logger::Logger::new(console.clone(), "glob".into());
        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);
            let globs = glob::Globs::new_with_includes(filter);

            if !filter.is_empty()
                && !globs.is_match(task_name.strip_prefix("//").unwrap_or(task_name))
            {
                glob_logger.debug(format!("Filtering {task_name} with {filter:?}").as_str());
                continue;
            }

            let task = tasks
                .get(task_name)
                .ok_or(format_error!("Task not found {task_name}"))?;

            if singleton::get_inspect_options().has_help && task.rule.help.is_none() {
                continue;
            }

            if task.phase == phase {
                if console_level == console::Level::Debug {
                    let task_yaml = serde_yaml::to_string(&task).unwrap_or_default();
                    console.debug(task_name, &task_yaml)?;
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

                    let (deps, targets) = if console_level <= console::Level::Message {
                        let mut dep_strings: Vec<Arc<str>> = Vec::new();
                        let mut target_strings: Vec<Arc<str>> = Vec::new();

                        if let Some(targets) = task.rule.targets.as_ref() {
                            for target in targets {
                                let target_str = match target {
                                    targets::Target::File(file_name) => file_name,
                                    targets::Target::Directory(dir_name) => dir_name,
                                };
                                target_strings.push(target_str.clone());
                            }
                        }

                        // Collect rule names
                        for rule_name in task.collect_rule_deps() {
                            dep_strings.push(rule_name);
                        }

                        // Collect expanded glob file paths
                        let globs = {
                            let state = get_state().read();
                            let tasks = state.tasks.read();
                            task.collects_glob_deps(&tasks)
                        };
                        let mut progress = console::Progress::new(
                            console.clone(),
                            "inspecting deps globs",
                            None,
                            None,
                        );
                        let files = workspace
                            .read()
                            .inspect_inputs(&mut progress, &globs)
                            .context(format_context!("Failed to inspect deps globs"))?;
                        dep_strings.extend(files.into_iter().map(|e| format!("//{e}").into()));

                        (Some(dep_strings), Some(target_strings))
                    } else {
                        (None, None)
                    };

                    let source = labels::get_source_from_label(task_name);

                    if let Some(query) = fuzzy_query {
                        // Score the task name against the fuzzy query
                        if let Some(match_result) = sublime_fuzzy::best_match(query, task_name) {
                            scored_tasks.push(ScoredTask {
                                score: match_result.score(),
                                name: task_name.into(),
                                info: TaskInfo {
                                    help,
                                    source,
                                    deps,
                                    targets,
                                },
                            });
                        }
                    } else {
                        task_info_list.insert(
                            task_name.into(),
                            TaskInfo {
                                help,
                                source,
                                deps,
                                targets,
                            },
                        );
                    }
                }
            }
        }

        if fuzzy_query.is_some() {
            // Sort by score descending so the best matches come first
            scored_tasks.sort_by(|a, b| b.score.cmp(&a.score));

            // Only show the top matching targets (top 10)
            let top_count = 10.min(scored_tasks.len());
            for scored in scored_tasks.into_iter().take(top_count) {
                task_info_list.insert(scored.name, scored.info);
            }
        }

        if console_level != console::Level::Debug {
            if task_info_list.is_empty() {
                console.error("No Results", "No matching rules available")?;
            } else {
                let task_info_list_yaml =
                    serde_yaml::to_string(&task_info_list).unwrap_or_default();
                console.write(&task_info_list_yaml)?;
            }
        }

        Ok(())
    }

    fn export_tasks_as_markdown(&self, path: &str) -> anyhow::Result<()> {
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

        let console = console::Console::new_file(path)
            .context(format_context!("Failed to create file {path}"))?;
        let mut md_printer = utils::markdown::Markdown::new(console.clone());
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
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        phase: task::Phase,
    ) -> anyhow::Result<executor::TaskResult> {
        let mut task_result = executor::TaskResult::new();
        let mut handle_list = Vec::new();
        let console = progress.console.clone();
        let task_count = self.sorted.len() as u64;
        let mut started = 0_u64;

        progress.set_message(format!("Executing {task_count} {phase} rules").as_str());
        progress.update_progress(0, task_count);

        let mut task_pending_set = HashSet::new();
        for node_index in self.sorted.iter() {
            let task_name = self.graph.get_task(*node_index);
            task_pending_set.insert(task_name);
        }

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
                let mut progress_bar =
                    console::Progress::new(console.clone(), task.rule.name.clone(), None, None);

                let displayed_rule =
                    utils::labels::sanitize_rule_for_display(task.rule.name.clone());
                if task.rule.type_ == Some(rule::RuleType::Optional) {
                    progress_bar.set_finalize_lines(logger::make_finalize_line(
                        logger::FinalType::NotRequired,
                        None,
                        displayed_rule.as_ref(),
                    ));
                }

                task_logger(console.clone(), task_name.into())
                    .debug(format!("Staging task {}", task.rule.name).as_str());

                started += 1;
                progress.set_prefix(format!("Queued {started}/{task_count}, Running").as_str());
                handle_list.push((
                    task.rule.name.clone(),
                    execute_rule(progress_bar, workspace.clone(), &task),
                ));

                loop {
                    let mut number_running = 0;
                    for (name, handle) in handle_list.iter() {
                        if !handle.is_finished() {
                            number_running += 1;
                        } else {
                            if task_pending_set.remove(name.as_ref()) {
                                progress.increment(1);
                            }
                        }
                    }

                    let active_tasks: Vec<_> = handle_list
                        .iter()
                        .filter_map(|(name, handle)| {
                            if !handle.is_finished() {
                                Some(utils::labels::get_rule_name_from_label(name.as_ref()))
                            } else {
                                None
                            }
                        })
                        .collect();

                    progress.set_message(active_tasks.join(",").as_str());

                    // this could be configured with a another global starlark function
                    if number_running < singleton::get_max_queue_count() {
                        break;
                    } else {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        }

        let mut active_tasks: Vec<_> = handle_list
            .iter()
            .filter_map(|(name, handle)| {
                if !handle.is_finished() {
                    Some(name.to_owned())
                } else {
                    None
                }
            })
            .collect();

        let mut first_error = None;
        for (name, handle) in handle_list {
            let handle_join_result = handle.join();
            if task_pending_set.remove(name.as_ref()) {
                progress.increment(1);
            }

            if let Some(offset) = active_tasks.iter().position(|e| *e == name) {
                active_tasks.remove(offset);
                let active_rule_names: Vec<_> = active_tasks
                    .iter()
                    .map(|e| utils::labels::get_rule_name_from_label(e.as_ref()))
                    .collect();
                progress.set_message(active_rule_names.join(",").as_str());
            }

            match handle_join_result {
                Ok(handle_result) => match handle_result {
                    Ok(handle_task_result) => {
                        task_result.extend(handle_task_result);
                    }
                    Err(err) => {
                        let err_message = err.to_string();
                        singleton::process_anyhow_error(err);
                        let log_status = self.log_status.read();
                        let mut logs = Vec::new();
                        for log in log_status.iter() {
                            if log.status == logs::Expect::Failure
                                && std::path::Path::new(log.file.as_ref()).exists()
                            {
                                logs.push(log.file.clone());
                            }
                        }
                        if !logs.is_empty() {
                            singleton::set_rule_failure(logs);
                        }
                        first_error = Some(format_error!("Rule failed: {err_message}"));
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
        } else {
            progress.set_finalize_lines(logger::make_finalize_line(
                logger::FinalType::Finished,
                progress.elapsed(),
                format!("{task_count} rules completed").as_str(),
            ));
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
        default_module_visibility: rule::Visibility::Public,
        all_modules: HashSet::new(),
        log_status: lock::StateLock::new(Vec::new()),
    }));
    STATE.get()
}

/// Derive the checkout directory path directly from a module name,
/// without consulting global state. Used by builtins that have an `EvalContext`.
pub fn get_checkout_path_for_module(module_name: &Arc<str>) -> Arc<str> {
    let path = std::path::Path::new(module_name.as_ref());
    path.parent()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default()
        .into()
}

/// Build-checkout path derived from a module name, without global state.
pub fn get_path_to_build_checkout_for_module(
    rule_name: Arc<str>,
    module_name: &Arc<str>,
) -> Arc<str> {
    let sanitized = get_sanitized_rule_name_for_module(rule_name, module_name);
    format!("build/{sanitized}").into()
}

/// Sanitize a rule name using a caller-supplied module name instead of global state.
pub fn get_sanitized_rule_name_for_module(rule_name: Arc<str>, module_name: &Arc<str>) -> Arc<str> {
    labels::sanitize_rule(
        rule_name,
        Some(module_name.clone()),
        workspace::SPACES_MODULE_NAME,
        labels::IsDep::No,
    )
}

/// Sanitize a working directory path using a caller-supplied module name.
pub fn get_sanitized_working_directory_for_module(
    rule_name: Arc<str>,
    module_name: &Arc<str>,
) -> Arc<str> {
    labels::sanitize_working_directory(rule_name, Some(module_name.clone()))
}

pub fn insert_task(task: task::Task) -> anyhow::Result<()> {
    let state = get_state().read();
    state.insert_task(task)
}

/// Insert a task using caller-supplied module name and default visibility,
/// without relying on the global `latest_starlark_module` state.
/// This is the preferred path when an `EvalContext` is available, as it
/// allows multiple modules to be evaluated concurrently.
pub fn insert_task_for_module(
    task: task::Task,
    module_name: &Arc<str>,
    default_visibility: rule::Visibility,
) -> anyhow::Result<()> {
    let state = get_state().read();
    state.insert_task_with_context(task, module_name, default_visibility)
}

/// Register a module name in `all_modules`.
/// Called by `evaluate_module` when an `EvalContext` carries the per-eval state.
pub fn register_module(name: Arc<str>) {
    let mut state = get_state().write();
    state.all_modules.insert(name);
}

pub fn show_tasks(
    console: console::Console,
    workspace: WorkspaceArc,
    phase: task::Phase,
    filter: &HashSet<Arc<str>>,
    strip_prefix: Option<Arc<str>>,
    fuzzy_query: Option<&str>,
) -> anyhow::Result<()> {
    let state = get_state().read();
    state.show_tasks(console, workspace, phase, filter, strip_prefix, fuzzy_query)
}

pub fn get_run_targets(has_help: HasHelp) -> anyhow::Result<Vec<Arc<str>>> {
    let state = get_state().read();
    state.get_run_targets(has_help)
}

pub fn export_tasks_as_mardown(path: &str) -> anyhow::Result<()> {
    let state = get_state().read();
    state.export_tasks_as_markdown(path)
}

pub fn update_tasks_digests(
    console: console::Console,
    workspace: workspace::WorkspaceArc,
) -> anyhow::Result<()> {
    let state = get_state().read();
    state.update_tasks_digests(console, workspace)
}

pub fn update_depedency_graph(
    console: console::Console,
    workspace: Option<workspace::WorkspaceArc>,
    phase: task::Phase,
) -> anyhow::Result<()> {
    let mut state = get_state().write();
    state.update_dependency_graph(console, workspace, phase)
}

pub fn update_target_dependency_graph(
    console: console::Console,
    target: Option<Arc<str>>,
) -> anyhow::Result<()> {
    let mut state = get_state().write();
    state.update_target_dependency_graph(console, target)
}

pub fn import_tasks_from_workspace_settings(
    console: console::Console,
    workspace: workspace::WorkspaceArc,
    needs_graph: NeedsGraph,
) -> anyhow::Result<()> {
    let mut state = get_state().write();
    state.import_tasks_from_workspace_settings(console, workspace, needs_graph)
}

pub fn get_pretty_tasks() -> String {
    let state = get_state().read();
    let tasks = state.tasks.read();
    let tasks = tasks.clone();
    serde_json::to_string_pretty(&tasks).unwrap()
}

pub fn execute(
    progress: &mut console::Progress,
    workspace: workspace::WorkspaceArc,
    phase: task::Phase,
) -> anyhow::Result<executor::TaskResult> {
    let state: std::sync::RwLockReadGuard<'_, State> = get_state().read();
    state.execute(progress, workspace, phase)
}

pub fn add_setup_dep_to_run_rules() -> anyhow::Result<()> {
    let state = get_state().read();
    let mut tasks = state.tasks.write();
    for task in tasks.values_mut() {
        if task.rule.type_ != Some(rule::RuleType::Setup) && task.phase == task::Phase::Run {
            rule::Deps::push_any_dep(
                &mut task.rule.deps,
                rule::AnyDep::Rule(rule::SETUP_RULE_NAME.into()),
            );
        }
    }
    Ok(())
}

fn get_rules_by_phase(phase: task::Phase) -> Vec<task::Task> {
    let state = get_state().read();
    let tasks = state.tasks.read();
    tasks
        .values()
        .filter(|task| task.phase == phase)
        .cloned()
        .collect()
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

pub fn get_checkout_rules() -> Vec<task::Task> {
    get_rules_by_phase(task::Phase::Checkout)
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

/// Get a task by its exact (already-sanitized) name.
pub fn get_task(name: &str) -> anyhow::Result<task::Task> {
    let state = get_state().read();
    let tasks = state.tasks.read();
    tasks
        .get(name)
        .cloned()
        .ok_or_else(|| format_error!("Task {} not found", name))
}

/// Clone a task using a caller-supplied module name instead of global state.
pub fn get_cloned_task_for_module(
    name: &str,
    module_name: &Arc<str>,
) -> anyhow::Result<task::Task> {
    let sanitized_name = get_sanitized_rule_name_for_module(name.into(), module_name);
    let state = get_state().read();
    let tasks = state.tasks.read();
    if let Some(task) = tasks.get(&sanitized_name) {
        Ok(task.clone())
    } else {
        Err(format_error!(
            "Task {} not found for cloning",
            sanitized_name
        ))
    }
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
    let log_status_file_output = format!("{log_output_folder}/{}", logs::LOG_STATUS_FILE_NAME);
    let content = serde_json::to_string_pretty(&log_status)
        .context(format_context!("Failed to serialize log status"))?;
    std::fs::write(log_status_file_output.as_str(), content).context(format_context!(
        "Failed to write log status to {log_status_file_output}"
    ))?;
    Ok(())
}

pub fn debug_sorted_tasks(console: console::Console, phase: task::Phase) -> anyhow::Result<()> {
    let logger = rules_printer_logger(console.clone());
    let state = get_state().read();
    for node_index in state.sorted.iter() {
        let task_name = state.graph.get_task(*node_index);
        if let Some(task) = state.tasks.read().get(task_name)
            && task.phase == phase
        {
            logger.debug(format!("Queued task {task_name}").as_str());
        }
    }
    Ok(())
}
