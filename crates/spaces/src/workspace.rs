use crate::{singleton, stardoc};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const WORKFLOW_TOML_NAME: &str = "workflows.spaces.toml";
pub const ENV_FILE_NAME: &str = "env.spaces.star";
pub const LOCK_FILE_NAME: &str = "lock.spaces.star";
pub const SPACES_MODULE_NAME: &str = "spaces.star";
pub const STAR_FILE_SUFFIX: &str = ".star";
pub const SPACES_STDIN_NAME: &str = "stdin.star";
const SPACES_SYSROOT_NAME: &str = "sysroot";

pub const SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE: &str = "SPACES_IS_WORKSPACE_REPRODUCIBLE";
pub const SPACES_ENV_WORKSPACE_DIGEST: &str = "SPACES_WORKSPACE_DIGEST";
pub const WORKSPACE_FILE_HEADER: &str = r#"
"""
Spaces Workspace file
"""
"#;

pub type WorkspaceArc = std::sync::Arc<lock::StateLock<Workspace>>;

fn logger_printer(printer: &mut printer::Printer) -> logger::Logger<'_> {
    logger::Logger::new_printer(printer, "workspace".into())
}

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

pub fn is_star_file(path: &str) -> bool {
    path.ends_with(STAR_FILE_SUFFIX)
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
    pub is_dirty: bool,                     // true if any star files have changed
    pub is_bin_dirty: bool,                 // true if any star files have changed
    pub target: Option<Arc<str>>,           // target called from the command line
    pub trailing_args: Vec<Arc<str>>,
    pub updated_assets: HashSet<Arc<str>>, // used by assets to keep track of exclusive access
    pub rule_metrics: HashMap<Arc<str>, RuleMetrics>, // used to keep track of rule metrics
    pub stardoc: stardoc::StarDoc,         // used to keep track of rule documentation
    pub settings: ws::Settings,
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

    pub fn get_new_branch_name(&self) -> Arc<str> {
        let path = self.absolute_path.clone();
        let directory_name = std::path::Path::new(path.as_ref())
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        directory_name.into()
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
        self.settings.json.members.clear();
    }

    pub fn add_member(&mut self, member: ws::Member) {
        self.settings.json.push_member(member);
    }

    pub fn get_absolute_path(&self) -> Arc<str> {
        self.absolute_path.clone()
    }

    pub fn transform_target_path(&self, target: Arc<str>) -> Arc<str> {
        if target.starts_with("//") {
            target
        } else if self.relative_invoked_path.is_empty() {
            format!("//{target}").into()
        } else if target.starts_with(':') {
            format!("//{}{}", self.relative_invoked_path, target).into()
        } else {
            format!("//{}/{}", self.relative_invoked_path, target).into()
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
            let workspace_path = format!("{current_directory}/{ENV_FILE_NAME}");
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

    fn get_relative_invoked_path(
        progress: &mut printer::MultiProgressBar,
        current_working_directory: &str,
        absolute_path_to_workspace: &str,
    ) -> Arc<str> {
        let mut relative_invoked_path: Arc<str> = current_working_directory
            .strip_prefix(absolute_path_to_workspace)
            .unwrap_or("")
            .into();

        if relative_invoked_path.ends_with('/') {
            relative_invoked_path = relative_invoked_path.strip_suffix('/').unwrap().into();
        }

        if relative_invoked_path.starts_with('/') {
            relative_invoked_path = relative_invoked_path.strip_prefix('/').unwrap().into();
        }

        logger(progress).info(format!("Invoked at: //{relative_invoked_path}").as_str());

        relative_invoked_path
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

            let sysroot = workspace_path.join(SPACES_SYSROOT_NAME);
            if entry.path() == sysroot {
                return false;
            }
        }

        true
    }

    pub fn new(
        mut progress: printer::MultiProgressBar,
        absolute_path_to_workspace: Option<Arc<str>>,
        is_clear_inputs: bool,
        input_script_names: Option<Vec<Arc<str>>>,
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

        logger(&mut progress).message(absolute_path.as_ref());
        let relative_invoked_path = Self::get_relative_invoked_path(
            &mut progress,
            current_working_directory.as_ref(),
            absolute_path.as_ref(),
        );

        // From here on all, paths are relative to the workspace root
        std::env::set_current_dir(std::path::Path::new(absolute_path.as_ref())).context(
            format_context!("Failed to set current directory to {absolute_path}"),
        )?;

        std::fs::create_dir_all(build_directory())
            .context(format_context!("Failed to create build directory"))?;

        // hash set to prevent loading the same module twice
        let mut loaded_modules = HashSet::new();

        // populate all modules that need to be processed - has order can't be a hash map
        let mut modules: Vec<(Arc<str>, Arc<str>)> = vec![];

        let env_content: Arc<str> =
            std::fs::read_to_string(format!("{absolute_path}/{ENV_FILE_NAME}"))
                .context(format_context!(
                    "Failed to read workspace file: {ENV_FILE_NAME}"
                ))?
                .into();

        loaded_modules.insert(ENV_FILE_NAME.into());
        modules.push((ENV_FILE_NAME.into(), env_content));

        let mut is_run_or_inspect = true;
        let (mut settings, is_json_available) = ws::Settings::load();

        if is_json_available == ws::IsJsonAvailable::Yes {
            logger(&mut progress).debug("Loading modules from sync order");
            for module in settings.json.order.iter() {
                if is_rules_module(module.as_ref()) {
                    progress.increment(1);
                    if !loaded_modules.contains(module) {
                        let path = format!("{absolute_path}/{module}");
                        let content = std::fs::read_to_string(path.as_str())
                            .context(format_context!("Failed to read file {}", path))?;

                        logger(&mut progress)
                            .trace(format!("Loading module from sync order: {module}").as_str());
                        loaded_modules.insert(module.clone());
                        modules.push((module.clone(), content.into()));
                    }
                }
            }
        } else {
            logger(&mut progress).debug(format!("No sync order found at {absolute_path}").as_str());
            is_run_or_inspect = false;
            if let Some(scripts) = input_script_names {
                settings.json.order = scripts;
            }
        }

        // workspace digest is calculated using the original modules passed on the
        // command line. If any repos are on tip of branch, the workspace
        // is marked as not-reproducible
        for (name, _) in modules.iter() {
            logger(&mut progress).message(format!("Digesting {name}").as_str());
        }
        let workspace_digest = calculate_digest(&modules);

        let log_directory: Arc<str> = format!(
            "{}/logs_{}",
            ws::SPACES_LOGS_NAME,
            date.format("%Y%m%d-%H-%M-%S")
        )
        .into();

        std::fs::create_dir_all(log_directory.as_ref()).context(format_context!(
            "Failed to create log folder {log_directory}",
        ))?;

        // load the modules scanned from the workspace
        for module_path in settings.json.scanned_modules.iter() {
            let module_path_as_path = std::path::Path::new(module_path.as_ref());
            if !module_path_as_path.exists() {
                logger(&mut progress).warning(
                    format!("Module {module_path} does not exist - rescanning workspace").as_str(),
                );
                singleton::set_rescan(true);
            }
        }

        // The workspace is not scanned on every run, only on the first run or when --rescan is passed
        // For large workspaces, this can be a significant time saver
        // is_scanned starts as None then Some(false) then Some(true) to finish the state machine
        if !settings.json.is_scanned.unwrap_or(false)
            || singleton::get_is_rescan()
            || settings.bin.star_files.is_empty()
        {
            // if the workspace is scanned, this will save settings on exit
            singleton::set_rescan(true);

            // if this is a checkout, we need to scan on first run/inspect
            settings.json.is_scanned = Some(is_run_or_inspect);

            logger(&mut progress).message(format!("Scanning {absolute_path}").as_str());

            // walkdir and find all .star files in the workspace
            // skip sysroot/build/logs directories
            let walkdir: Vec<_> = walkdir::WalkDir::new(absolute_path.as_ref())
                .into_iter()
                .filter_entry(|entry| {
                    Self::filter_predicate(std::path::Path::new(absolute_path.as_ref()), entry)
                })
                .collect();

            progress.set_total(walkdir.len() as u64);
            settings.bin.star_files.clear();
            settings.json.scanned_modules.clear();
            for entry in walkdir {
                progress.increment(1);
                if let Ok(entry) = entry {
                    if !entry.file_type().is_file() {
                        continue;
                    }

                    let entry_name = entry.file_name().to_string_lossy().to_string();
                    if !is_star_file(entry_name.as_str()) {
                        continue;
                    }

                    let path: Arc<str> = entry.path().to_string_lossy().into();
                    if let Some(stripped_path) =
                        path.strip_prefix(format!("{absolute_path}/").as_str())
                    {
                        logger(&mut progress)
                            .message(format!("star file {stripped_path}").as_str());

                        // all star files are hashed and tracked when modified
                        settings.bin.star_files.insert(
                            stripped_path.into(),
                            ws::BinDetail {
                                modified: None,
                                ..Default::default()
                            },
                        );

                        // spaces.star files are added to the modules to be processed
                        // no need to grab original command line modules
                        // they are stored in settings.order
                        if is_rules_module(entry_name.as_str())
                            && !loaded_modules.contains(stripped_path)
                        {
                            logger(&mut progress)
                                .debug(format!("Loading module: {stripped_path}").as_str());
                            settings.json.scanned_modules.insert(stripped_path.into());
                        }
                    }
                }
            }
        } else {
            progress.set_ending_message(
                "Loaded modules from settings. Use `--rescan` to check for new modules.",
            );
        }

        // checks if any of the modules have changed
        // checks modified time then hashes
        let (updated_modules, settings_is_dirty) = settings
            .bin
            .update_hashes(&mut progress)
            .context(format_context!(
                "Failed to update hashes for modules in workspace"
            ))?;

        // if any star files have changed, workspace is dirty - need to re-run starlark
        let is_dirty = settings_is_dirty == ws::IsDirty::Yes;
        if is_dirty {
            logger(&mut progress).info("is dirty");
        }

        // message the user of changed modules
        for updated in updated_modules {
            logger(&mut progress).message(format!("dirty {updated}").as_str());
        }

        // load the modules scanned from the workspace
        for module_path in settings.json.scanned_modules.iter() {
            if !loaded_modules.contains(module_path) {
                let content: Arc<str> = std::fs::read_to_string(module_path.as_ref())
                    .context(format_context!("Failed to read file {module_path}"))?
                    .into();

                loaded_modules.insert(module_path.clone());
                modules.push((module_path.clone(), content));
            }
        }

        #[allow(unused)]
        let unique = get_unique().context(format_context!("failed to get unique marker"))?;

        // The inputs use the hashes calculated by Changes to keep
        // track of file hashes at the time a rule is executed
        if is_clear_inputs {
            settings
                .clear_inputs()
                .context(format_context!("Failed to clear inputs"))?;
        }

        // Workspace is assumed to reproducible until a rule is processed
        // that is not reproducible - such as a repo on tip of branch
        let mut env = environment::Environment::default();

        let is_reproducible = if singleton::get_args_env().is_empty() {
            "true"
        } else {
            "false"
        };
        env.vars.insert(
            SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE.into(),
            is_reproducible.into(),
        );

        if !singleton::get_args_env().is_empty() {
            // scripts may read ENV variables
            // so they need to rerun if any are passed on the command line
            settings.bin.is_always_evaluate = true;
        }

        Ok(Self {
            modules,
            absolute_path,
            log_directory,
            is_create_lock_file: false,
            digest: workspace_digest,
            locks: HashMap::new(),
            env,
            is_dirty,
            is_bin_dirty: is_dirty,
            updated_assets: HashSet::new(),
            rule_metrics: HashMap::new(),
            stardoc: stardoc::StarDoc::default(),
            trailing_args: vec![],
            target: None,
            relative_invoked_path,
            settings,
        })
    }

    pub fn set_env(&mut self, env: environment::Environment) {
        self.env = env;
        self.settings.bin.env_json = serde_json::to_string(&self.env).unwrap().into();
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
        workspace_file_content.push_str("WORKSPACE_ENV = ");
        workspace_file_content.push_str(env);
        workspace_file_content.push_str("\n\nworkspace.set_env(env = WORKSPACE_ENV) \n");
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
        workspace_file_content.push_str("WORKSPACE_LOCKS = ");
        let locks_str = serde_json::to_string_pretty(&self.locks)
            .context(format_context!("Failed to serialize locks"))?;
        workspace_file_content.push_str(locks_str.as_str());
        workspace_file_content.push_str("\n\nworkspace.set_locks(locks = WORKSPACE_LOCKS) \n");

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
        self.is_bin_dirty = true;
        self.settings
            .bin
            .changes
            .update_from_inputs(progress, inputs)
            .context(format_context!("Failed to update workspace changes"))?;

        Ok(())
    }

    pub fn add_git_commit_lock(&mut self, rule_name: &str, commit: Arc<str>) {
        self.locks.insert(rule_name.into(), commit);
    }

    pub fn get_short_digest(&self) -> Arc<str> {
        get_short_digest(self.digest.as_ref())
    }

    pub fn get_store_path(&self) -> Arc<str> {
        self.settings.json.store_path.clone()
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
            .settings
            .bin
            .changes
            .get_digest(progress, seed, inputs)
            .context(format_context!("Failed to get digest for rule {rule_name}"))?;

        let is_changed_result = self
            .settings
            .bin
            .inputs
            .is_changed(rule_name, digest)
            .context(format_context!("Failed to check if rule inputs changed"))?;

        logger(progress)
            .debug(format!("Rule {rule_name} inputs changed: {is_changed_result:?}",).as_str());

        Ok(is_changed_result)
    }

    pub fn save_bin(&self, printer: &mut printer::Printer) -> anyhow::Result<()> {
        if !self.settings.bin.changes.entries.is_empty() {
            for (key, _) in self.settings.bin.changes.entries.iter() {
                logger_printer(printer).trace(format!("Changes: {key}").as_str());
            }
        } else {
            logger_printer(printer).debug("No changes");
        }

        if !self.settings.bin.inputs.entries.is_empty() {
            for (key, value) in self.settings.bin.inputs.entries.iter() {
                logger_printer(printer).trace(format!("Inputs: {key}:{value}").as_str());
            }
        } else {
            logger_printer(printer).debug("No changes");
        }
        self.settings.save_bin()
    }

    pub fn update_rule_digest(&mut self, rule: &str, digest: Arc<str>) {
        self.settings.bin.inputs.save_digest(rule, digest);
    }
}
