use crate::{inputs, singleton};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const WORKFLOW_TOML_NAME: &str = "workflows.spaces.toml";
pub const ENV_FILE_NAME: &str = "env.spaces.star";
pub const LOCK_FILE_NAME: &str = "lock.spaces.star";
pub const SPACES_MODULE_NAME: &str = "spaces.star";
pub const SPACES_STDIN_NAME: &str = "stdin.star";
const SPACES_CAPSULES_NAME: &str = "capsules";
const SPACES_CAPSULES_WORKSPACES_NAME: &str = "workspace";
const SPACES_CAPSULES_WORKFLOWS_NAME: &str = "workflows";
const SPACES_CAPSULES_STATUS_NAME: &str = "status";
const SPACES_CAPSULES_SYSROOT_NAME: &str = "sysroot";

pub const SPACES_CAPSULES_INFO_NAME: &str = "capsules.spaces.json";
pub const SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE: &str = "SPACES_IS_WORKSPACE_REPRODUCIBLE";
pub const SPACES_ENV_WORKSPACE_DIGEST: &str = "SPACES_WORKSPACE_DIGEST";
pub const SPACES_ENV_CAPSULE_WORKFLOWS: &str = "SPACES_CAPSULES_WORKFLOWS";
pub const WORKSPACE_FILE_HEADER: &str = r#"
"""
Spaces Workspace file
"""
"#;

pub type WorkspaceArc = std::sync::Arc<lock::StateLock<Workspace>>;

fn logger(progress: &mut printer::MultiProgressBar) -> logger::Logger<'_> {
    logger::Logger::new_progress(progress, "workspace".into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMetrics {
    elapsed_time: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMetricsFile {
    metrics: Vec<HashMap<Arc<str>, RuleMetrics>>,
}

impl RuleMetricsFile {
    pub fn update(workspace: WorkspaceArc) -> anyhow::Result<()> {
        let workspace_path = workspace.read().get_absolute_path();
        let metric_entry = workspace.read().rule_metrics.clone();
        let metrics_file = format!("{workspace_path}/{}", ws::METRICS_FILE_NAME);
        let metrics_path = std::path::Path::new(metrics_file.as_str());
        let metrics = if metrics_path.exists() {
            let content = std::fs::read_to_string(metrics_file.as_str()).context(
                format_context!("Failed to read metrics file {metrics_file}"),
            )?;
            let mut metrics_content: RuleMetricsFile = serde_json::from_str(content.as_str())
                .context(format_context!(
                    "Failed to parse metrics file {metrics_file}"
                ))?;
            metrics_content.metrics.push(metric_entry);
            metrics_content
        } else {
            RuleMetricsFile {
                metrics: vec![metric_entry],
            }
        };

        let content = serde_json::to_string_pretty(&metrics)
            .context(format_context!("Failed to serialize metrics"))?;

        std::fs::write(metrics_file.as_str(), content.as_str()).context(format_context!(
            "Failed to write metrics file {metrics_file}"
        ))?;

        Ok(())
    }
}

pub fn get_short_digest(digest: &str) -> Arc<str> {
    digest[0..8].into()
}

pub fn calculate_digest(modules: &[(Arc<str>, Arc<str>)]) -> Arc<str> {
    let mut hasher = blake3::Hasher::new();
    for (_, content) in modules {
        hasher.update(content.as_bytes());
    }
    hasher.finalize().to_string().into()
}

pub fn get_current_working_directory() -> anyhow::Result<Arc<str>> {
    let current_working_directory = std::env::current_dir()
        .context(format_context!("Failed to get current working directory - something might be wrong with your environment where CWD is not set"))?
        .to_string_lossy()
        .to_string();
    Ok(current_working_directory.into())
}

pub fn is_rules_module(path: &str) -> bool {
    path.ends_with(SPACES_MODULE_NAME)
}

pub fn get_workspace_path(workspace_path: &str, current_path: &str, target_path: &str) -> Arc<str> {
    if target_path.starts_with("//") {
        format!("{workspace_path}/{target_path}").into()
    } else {
        let name_path = std::path::Path::new(current_path);
        if let Some(parent) = name_path.parent() {
            format!("{}/{}", parent.to_string_lossy(), target_path).into()
        } else {
            target_path.into()
        }
    }
}

pub fn get_spaces_tools_path(store_path: &str) -> Arc<str> {
    format!("{store_path}/spaces_tools").into()
}

fn get_unique() -> anyhow::Result<String> {
    let duration_since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context(format_context!("No system time"))?;
    let duration_since_epoch_string = format!("{}", duration_since_epoch.as_nanos());
    let unique_sha256 = sha256::digest(duration_since_epoch_string.as_bytes());
    Ok(unique_sha256.as_str()[0..4].to_string())
}

pub fn build_directory() -> &'static str {
    "build"
}

pub fn get_inputs_path() -> &'static str {
    "build/workspace.inputs.spaces"
}

pub fn get_changes_path() -> &'static str {
    "build/workspace.changes.spaces"
}

#[derive(Debug)]
pub struct Workspace {
    pub modules: Vec<(Arc<str>, Arc<str>)>,
    pub absolute_path: Arc<str>,            // set at startup
    pub relative_invoked_path: Arc<str>,    // workspace relative path where spaces was invoked
    pub log_directory: Arc<str>,            // always @logs/timestamp
    pub is_create_lock_file: bool,          // set at startup
    pub digest: Arc<str>,                   // set at startup
    pub locks: HashMap<Arc<str>, Arc<str>>, // set during eval
    pub env: environment::Environment,      // set during eval
    #[allow(dead_code)]
    pub new_branch_name: Option<Arc<str>>, // set during eval - not used
    changes: changes::Changes,              // modified during run
    inputs: inputs::Inputs,                 // modified during run
    pub target: Option<Arc<str>>,           // target called from the command line
    pub trailing_args: Vec<Arc<str>>,
    pub updated_assets: HashSet<Arc<str>>, // used by assets to keep track of exclusive access
    pub rule_metrics: HashMap<Arc<str>, RuleMetrics>, // used to keep track of rule metrics
    pub settings: ws::Settings
}

impl Workspace {
    pub fn update_rule_metrics(&mut self, rule_name: &str, elapsed_time: std::time::Duration) {
        self.rule_metrics.insert(
            rule_name.into(),
            RuleMetrics {
                elapsed_time: elapsed_time.as_secs_f64(),
            },
        );
    }

    pub fn set_is_reproducible(&mut self, value: bool) {
        self.env.vars.insert(
            SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE.into(),
            value.to_string().into(),
        );
    }

    #[allow(dead_code)]
    pub fn get_relative_directory(&self, relative_path: &str) -> Arc<str> {
        format!("{}/{}", self.absolute_path, relative_path).into()
    }

    pub fn clear_members(&mut self) {
        self.settings.members.clear();
    }

    pub fn add_member(&mut self, member: ws::Member) {
        self.settings.push_member(member);
    }

    pub fn save_settings(&self) -> anyhow::Result<()> {
        self.settings.save(self.absolute_path.as_ref())
    }

    pub fn get_absolute_path(&self) -> Arc<str> {
        self.absolute_path.clone()
    }

    pub fn transform_target_path(&self, target: Arc<str>) -> Arc<str> {
        if target.starts_with("//") {
            target.strip_prefix("//").unwrap().into()
        } else {
            if self.relative_invoked_path.is_empty() {
                target
            } else {
                if target.starts_with(':') {
                    return format!("{}{}", self.relative_invoked_path, target).into();
                } else {
                    format!("{}/{}", self.relative_invoked_path, target).into()
                }
            }
        }
    }

    pub fn is_reproducible(&self) -> bool {
        if let Some(value) = self.env.vars.get(SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE) {
            return value.as_ref() == "true";
        }
        false
    }

    fn find_workspace_root(current_working_directory: &str) -> anyhow::Result<Arc<str>> {
        let mut current_directory = current_working_directory.to_owned();
        loop {
            let workspace_path = format!("{}/{}", current_directory, ENV_FILE_NAME);
            if std::path::Path::new(workspace_path.as_str()).exists() {
                return Ok(current_directory.into());
            }
            let parent_directory = std::path::Path::new(current_directory.as_str()).parent();
            if parent_directory.is_none() {
                return Err(format_error!(
                    "Failed to find {} in any parent directory",
                    ENV_FILE_NAME
                ));
            }
            current_directory = parent_directory.unwrap().to_string_lossy().to_string();
        }
    }

    fn filter_predicate(workspace_path: &std::path::Path, entry: &walkdir::DirEntry) -> bool {
        if entry.path() == workspace_path {
            return true;
        }

        if entry.file_type().is_dir() {
            if entry.path().starts_with(".") {
                return false;
            }

            let workflows_path = entry.path().join(WORKFLOW_TOML_NAME);
            if workflows_path.exists() {
                return false;
            }

            let spaces_env_path = entry.path().join(ENV_FILE_NAME);
            if spaces_env_path.exists() {
                return false;
            }

            let sysroot = workspace_path.join(SPACES_CAPSULES_SYSROOT_NAME);
            if entry.path() == sysroot {
                return false;
            }

            if entry.file_name() == SPACES_CAPSULES_NAME {
                return false;
            }
        }

        true
    }

    pub fn new(
        mut progress: printer::MultiProgressBar,
        absolute_path_to_workspace: Option<Arc<str>>,
        is_clear_inputs: bool,
        input_script_names: Option<Vec<Arc<str>>>
    ) -> anyhow::Result<Self> {
        let date = chrono::Local::now();

        let current_working_directory = get_current_working_directory().context(
            format_context!("Failed to get current working directory in new workspace"),
        )?;

        let absolute_path = if let Some(absolute_path) = absolute_path_to_workspace {
            absolute_path
        } else {
            // search the current directory and all parent directories for the workspace file
            Self::find_workspace_root(current_working_directory.as_ref())
                .context(format_context!("While searching for workspace root"))?
        };

        let mut relative_invoked_path: Arc<str> = current_working_directory
            .strip_prefix(absolute_path.as_ref())
            .unwrap_or("".into())
            .into();

        if relative_invoked_path.ends_with('/') {
            relative_invoked_path = relative_invoked_path.strip_suffix('/').unwrap().into();
        }

        if relative_invoked_path.starts_with('/') {
            relative_invoked_path = relative_invoked_path.strip_prefix('/').unwrap().into();
        }

        logger(&mut progress).message(format!("{absolute_path}").as_str());

        logger(&mut progress).info(format!("Invoked at: {relative_invoked_path}").as_str());

        std::env::set_current_dir(std::path::Path::new(absolute_path.as_ref())).context(
            format_context!("Failed to set current directory to {absolute_path}"),
        )?;

        let mut loaded_modules = HashSet::new();
        let mut modules: Vec<(Arc<str>, Arc<str>)> = vec![];

        let env_content: Arc<str> =
            std::fs::read_to_string(format!("{}/{}", absolute_path, ENV_FILE_NAME))
                .context(format_context!(
                    "Failed to read workspace file: {ENV_FILE_NAME}"
                ))?
                .into();

        loaded_modules.insert(ENV_FILE_NAME.into());
        modules.push((ENV_FILE_NAME.into(), env_content));

        let mut original_modules: Vec<(Arc<str>, Arc<str>)> = vec![];

        let mut is_run_or_inspect = true;
        let mut settings = if let Ok(settings) = ws::Settings::load(absolute_path.as_ref()) {
            logger(&mut progress).debug("Loading modules from sync order");
            for module in settings.order.iter() {
                if is_rules_module(module.as_ref()) {
                    progress.increment(1);
                    let path = format!("{}/{}", absolute_path, module);
                    let content = std::fs::read_to_string(path.as_str())
                        .context(format_context!("Failed to read file {}", path))?;
                    if !loaded_modules.contains(module) {
                        logger(&mut progress)
                            .trace(format!("Loading module from sync order: {}", module).as_str());
                        loaded_modules.insert(module.clone());
                        original_modules.push((module.clone(), content.into()));
                    }
                }
            }
            settings
        } else {
            logger(&mut progress).debug(format!("No sync order found at {absolute_path}").as_str());
            is_run_or_inspect = false;
            logger(&mut progress).debug("New Settings");
            let mut settings = ws::Settings::new();
            if let Some(scripts) = input_script_names {
                settings.order = scripts;
            }
            settings
        };

        for (name, _) in original_modules.iter() {
            logger(&mut progress).message(format!("Digesting {}", name).as_str());
        }
        let workspace_digest = calculate_digest(&original_modules);

        std::fs::create_dir_all(build_directory())
            .context(format_context!("Failed to create build directory"))?;

        let log_directory: Arc<str> = format!(
            "{}/logs_{}",
            ws::SPACES_LOGS_NAME,
            date.format("%Y%m%d-%H-%M-%S")
        )
        .into();

        std::fs::create_dir_all(log_directory.as_ref()).context(format_context!(
            "Failed to create log folder {log_directory}",
        ))?;

        modules.extend(original_modules);

        let mut scanned_modules = HashSet::new();

        if !settings.is_scanned.unwrap_or(false) || singleton::get_is_rescan() {
            // not scanned until walkdir runs during run or inspect
            settings.is_scanned = Some(is_run_or_inspect);

            // walkdir and find all spaces.star files in the workspace
            let walkdir: Vec<_> = walkdir::WalkDir::new(absolute_path.as_ref())
                .into_iter()
                .filter_entry(|entry| {
                    Self::filter_predicate(std::path::Path::new(absolute_path.as_ref()), entry)
                })
                .collect();

            progress.set_total(walkdir.len() as u64);

            for entry in walkdir {
                progress.increment(1);
                if let Ok(entry) = entry.context(format_context!("While walking directory")) {
                    if entry.file_type().is_file()
                        && is_rules_module(entry.file_name().to_string_lossy().as_ref())
                    {
                        let path: Arc<str> = entry.path().to_string_lossy().into();
                        if let Some(stripped_path) =
                            path.strip_prefix(format!("{}/", absolute_path).as_str())
                        {
                            if !stripped_path.starts_with(".spaces")
                                && !loaded_modules.contains(stripped_path)
                            {
                                logger(&mut progress).debug(
                                    format!("Loading module from directory: {stripped_path}")
                                        .as_str(),
                                );
                                loaded_modules.insert(stripped_path.into());
                                scanned_modules.insert(stripped_path.into());
                            }
                        }
                    }
                }
            }

            settings.scanned_modules = scanned_modules.clone();
            settings
                .save(absolute_path.as_ref())
                .context(format_context!(
                    "Failed to save settings file for {absolute_path}"
                ))?;
        } else {
            progress.set_ending_message(
                "Loaded modules from settings. Use `--rescan` to check for new modules.",
            );
            scanned_modules = settings.scanned_modules.clone();
        }

        let mut unordered_modules: Vec<(Arc<str>, Arc<str>)> = vec![];
        for module_path in scanned_modules {
            let content: Arc<str> = std::fs::read_to_string(module_path.as_ref())
                .context(format_context!("Failed to read file {module_path}"))?
                .into();
            unordered_modules.push((module_path, content));
        }
        unordered_modules.sort_by(|a, b| a.0.cmp(&b.0));
        modules.extend(unordered_modules);

        let changes_path = get_changes_path();
        let skip_folders = vec![ws::SPACES_LOGS_NAME.into()];
        let changes = changes::Changes::new(changes_path, skip_folders);

        #[allow(unused)]
        let unique = get_unique().context(format_context!("failed to get unique marker"))?;

        if is_clear_inputs {
            let inputs_path = get_inputs_path();
            if std::path::Path::new(inputs_path).exists() {
                std::fs::remove_file(inputs_path).context(format_context!(
                    "Failed to remove inputs file {inputs_path}"
                ))?;
            }
        }

        let mut env = environment::Environment::default();

        env.vars
            .insert(SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE.into(), "true".into());

        Ok(Self {
            modules,
            absolute_path,
            log_directory,
            is_create_lock_file: false,
            digest: workspace_digest,
            locks: HashMap::new(),
            env,
            new_branch_name: None,
            changes,
            updated_assets: HashSet::new(),
            inputs: inputs::Inputs::new(get_inputs_path()),
            rule_metrics: HashMap::new(),
            trailing_args: vec![],
            target: None,
            relative_invoked_path,
            settings
        })
    }

    pub fn set_env(&mut self, env: environment::Environment) {
        self.env = env;
    }

    pub fn update_env(&mut self, env: environment::Environment) -> anyhow::Result<()> {
        self.env.vars.extend(env.vars);
        self.env.paths.extend(env.paths);
        if let Some(inherited_vars) = env.inherited_vars {
            if let Some(existing_inherited_vars) = self.env.inherited_vars.as_mut() {
                existing_inherited_vars.extend(inherited_vars.clone());
            } else {
                self.env.inherited_vars = Some(inherited_vars);
            }
        }

        if let Some(system_paths) = env.system_paths {
            if let Some(existing_system_paths) = self.env.system_paths.as_mut() {
                existing_system_paths.extend(system_paths.clone());
            } else {
                self.env.system_paths = Some(system_paths);
            }
        }
        Ok(())
    }

    pub fn get_env(&self) -> environment::Environment {
        self.env.clone()
    }

    pub fn save_env_file(&self, env: &str) -> anyhow::Result<()> {
        let mut workspace_file_content = String::new();
        workspace_file_content.push_str(WORKSPACE_FILE_HEADER);
        workspace_file_content.push('\n');
        workspace_file_content.push_str("workspace_env = ");
        workspace_file_content.push_str(env);
        workspace_file_content.push_str("\n\ninfo.set_env(env = workspace_env) \n");
        let workspace_file_path = format!("{}/{}", self.absolute_path, ENV_FILE_NAME);
        std::fs::write(workspace_file_path.as_str(), workspace_file_content)
            .context(format_context!("Failed to write workspace file"))?;

        Ok(())
    }

    pub fn save_lock_file(&self) -> anyhow::Result<()> {
        if !self.is_create_lock_file {
            return Ok(());
        }
        let mut workspace_file_content = String::new();
        workspace_file_content.push_str(WORKSPACE_FILE_HEADER);
        workspace_file_content.push('\n');
        workspace_file_content.push_str("workspace_locks = ");
        let locks_str = serde_json::to_string_pretty(&self.locks)
            .context(format_context!("Failed to serialize locks"))?;
        workspace_file_content.push_str(locks_str.as_str());
        workspace_file_content.push_str("\n\ninfo.set_locks(locks = workspace_locks) \n");

        let workspace_file_path = format!("{}/{}", self.absolute_path, LOCK_FILE_NAME);
        std::fs::write(workspace_file_path.as_str(), workspace_file_content)
            .context(format_context!("Failed to write workspace file"))?;

        Ok(())
    }

    pub fn update_changes(
        &mut self,
        progress: &mut printer::MultiProgressBar,
        inputs: &HashSet<Arc<str>>,
    ) -> anyhow::Result<()> {
        self.changes
            .update_from_inputs(progress, inputs)
            .context(format_context!("Failed to update workspace changes"))?;

        Ok(())
    }

    pub fn save_changes(&mut self) -> anyhow::Result<()> {
        let changes_path = get_changes_path();
        self.changes
            .save(changes_path)
            .context(format_context!("Failed to save changes file"))?;
        Ok(())
    }

    pub fn add_git_commit_lock(&mut self, rule_name: &str, commit: Arc<str>) {
        self.locks.insert(rule_name.into(), commit);
    }

    pub fn get_rule_inputs_digest(
        &self,
        progress: &mut printer::MultiProgressBar,
        seed: &str,
        globs: &HashSet<Arc<str>>,
    ) -> anyhow::Result<Arc<str>> {
        self.changes.get_digest(progress, seed, globs)
    }

    pub fn get_short_digest(&self) -> Arc<str> {
        get_short_digest(self.digest.as_ref())
    }

    pub fn get_store_path(&self) -> Arc<str> {
        self.settings.store_path.clone()
    }

    fn get_path_to_capsule_store(&self) -> Arc<str> {
        format!("{}/{}", self.get_store_path(), SPACES_CAPSULES_NAME).into()
    }

    pub fn get_path_to_capsule_store_workspaces(&self) -> Arc<str> {
        format!(
            "{}/{}",
            self.get_path_to_capsule_store(),
            SPACES_CAPSULES_WORKSPACES_NAME
        )
        .into()
    }

    pub fn get_path_to_capsule_store_workflows(&self) -> Arc<str> {
        format!(
            "{}/{}",
            self.get_path_to_capsule_store(),
            SPACES_CAPSULES_WORKFLOWS_NAME
        )
        .into()
    }

    pub fn get_path_to_workflows(&self) -> Arc<str> {
        // capsules will pass SPACES_ENV_CAPSULE_WORKFLOWS to child processes
        // the top level process will use the digest
        std::env::var(SPACES_ENV_CAPSULE_WORKFLOWS)
            .unwrap_or_else(|_| {
                format!(
                    "{}/{}",
                    self.get_path_to_capsule_store_workflows(),
                    self.get_short_digest()
                )
            })
            .into()
    }

    pub fn get_path_to_capsule_store_status(&self) -> Arc<str> {
        format!(
            "{}/{}",
            self.get_path_to_capsule_store(),
            SPACES_CAPSULES_STATUS_NAME
        )
        .into()
    }

    pub fn get_path_to_capsule_store_sysroot(&self) -> Arc<str> {
        format!(
            "{}/{}",
            self.get_path_to_capsule_store(),
            SPACES_CAPSULES_SYSROOT_NAME
        )
        .into()
    }

    pub fn get_spaces_tools_path(&self) -> Arc<str> {
        get_spaces_tools_path(self.get_store_path().as_ref())
    }

    pub fn get_cargo_binstall_root(&self) -> Arc<str> {
        format!("{}/cargo_binstall_bin_dir", self.get_spaces_tools_path()).into()
    }

    pub fn get_log_file(&self, rule_name: &str) -> Arc<str> {
        let rule_name = rule_name.replace('/', "_");
        let rule_name = rule_name.replace(':', "_");
        format!("{}/{rule_name}.log", self.log_directory).into()
    }

    pub fn is_rule_inputs_changed(
        &self,
        progress: &mut printer::MultiProgressBar,
        rule_name: &str,
        seed: &str,
        inputs: &HashSet<Arc<str>>,
    ) -> anyhow::Result<Option<Arc<str>>> {
        let digest = self
            .get_rule_inputs_digest(progress, seed, inputs)
            .context(format_context!("Failed to get digest for rule {rule_name}"))?;
        self.inputs.is_changed(rule_name, digest)
    }

    pub fn update_rule_digest(&mut self, rule: &str, digest: Arc<str>) {
        self.inputs.save_digest(rule, digest);
    }

    pub fn save_inputs(&self) -> anyhow::Result<()> {
        let inputs_path = get_inputs_path();
        self.inputs.save(inputs_path)
    }
}
