use crate::{singleton, stardoc};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use utils::{age, environment, lock, logger, rule, store, ws};

pub const WORKFLOW_TOML_NAME: &str = "workflows.spaces.toml";
pub const SHELL_TOML_NAME: &str = "shell.spaces.toml";
pub const CHECKOUT_FILE_NAME: &str = "checkout.spaces.star";
pub const ENV_FILE_NAME: &str = "env.spaces.star";
pub const ENV_MD_FILE_NAME: &str = "env.spaces.md";
pub const LOCK_FILE_NAME: &str = "lock.spaces.star";
pub const SPACES_MODULE_NAME: &str = "spaces.star";
pub const STAR_FILE_SUFFIX: &str = ".star";
pub const SPACES_STDIN_NAME: &str = "stdin.star";
const SPACES_SYSROOT_NAME: &str = "sysroot";

const AUTOMATIC_WORKSPACE_ABSOLUTE_PATH: &str = "SPACES_WORKSPACE_ABSOLUTE_PATH";
const AUTOMATIC_WORKSPACE_DIGEST: &str = "SPACES_WORKSPACE_DIGEST";
const AUTOMATIC_WORKSPACE_IS_REPRODUCIBLE: &str = "SPACES_IS_WORKSPACE_REPRODUCIBLE";
const AUTOMATIC_WORKSPACE_STORE_PATH: &str = "SPACES_WORKSPACE_STORE_PATH";

pub const WORKSPACE_FILE_HEADER: &str = r#"
"""
Spaces Workspace file
"""
"#;

#[derive(Copy, Clone, PartialEq)]
pub enum IsCheckoutPhase {
    No,
    Yes,
}

#[derive(Copy, Clone, PartialEq)]
pub enum IsClearInputs {
    No,
    Yes,
}

impl From<bool> for IsClearInputs {
    fn from(value: bool) -> Self {
        if value {
            IsClearInputs::Yes
        } else {
            IsClearInputs::No
        }
    }
}

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
    if digest.len() < 8 {
        digest.into()
    } else {
        digest[0..8].into()
    }
}

pub fn calculate_digest(env_str: &str, modules: &[(Arc<str>, Arc<str>)]) -> Arc<str> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(env_str.as_bytes());
    for (_, content) in modules {
        hasher.update(content.as_bytes());
    }
    hasher.finalize().to_string().into()
}

pub fn get_age(workspace_path: &std::path::Path) -> Option<age::LastUsed> {
    let env_path = workspace_path.join(ENV_FILE_NAME);
    let settings_path = workspace_path.join(ws::SETTINGS_FILE_NAME);
    if let (Some(env_age), Some(settings_age)) = (
        age::LastUsed::new_from_file(&env_path),
        age::LastUsed::new_from_file(&settings_path),
    ) {
        let now = age::get_now();
        let current_env_age = env_age.get_age(now);
        let current_settings_age = settings_age.get_age(now);
        // return the youngest of the two
        Some(if current_env_age > current_settings_age {
            settings_age
        } else {
            env_age
        })
    } else {
        None
    }
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

pub fn save_env_file_at(dir_path: &std::path::Path, env_md: &str, env: &str) -> anyhow::Result<()> {
    let md_path = dir_path.join(ENV_MD_FILE_NAME);
    std::fs::write(&md_path, env_md)
        .context(format_context!("Failed to save {}", md_path.display()))?;

    let mut workspace_file_content = String::new();
    workspace_file_content.push_str(WORKSPACE_FILE_HEADER);
    workspace_file_content.push('\n');
    workspace_file_content.push_str("WORKSPACE_ENV = ");
    workspace_file_content.push_str(env);
    workspace_file_content.push_str(
        r#"
info.set_minimum_version("0.15.27")
        "#,
    );
    workspace_file_content.push_str("\n\nworkspace.set_env(env = WORKSPACE_ENV) \n");
    let workspace_file_path = dir_path.join(ENV_FILE_NAME);
    std::fs::write(workspace_file_path, workspace_file_content)
        .context(format_context!("Failed to write workspace file"))?;

    Ok(())
}

#[derive(Debug)]
pub struct Workspace {
    pub modules: Vec<(Arc<str>, Arc<str>)>,
    pub absolute_path: Arc<str>,            // set at startup
    pub relative_invoked_path: Arc<str>,    // workspace relative path where spaces was invoked
    pub log_directory: Arc<str>,            // always @logs/timestamp
    pub is_create_lock_file: bool,          // set at startup
    pub locks: HashMap<Arc<str>, Arc<str>>, // set during eval
    pub is_env_set: bool,
    env: environment::AnyEnvironment, // set during eval
    pub is_dirty: bool,               // true if any star files have changed
    pub is_bin_dirty: bool,           // true if any star files have changed
    pub is_reproducible: bool,        // true if workspace has no repos checked out on branches
    pub target: Option<Arc<str>>,     // target called from the command line
    pub trailing_args: Vec<Arc<str>>,
    pub updated_assets: HashSet<Arc<str>>, // used by assets to keep track of exclusive access
    pub rule_metrics: HashMap<Arc<str>, RuleMetrics>, // used to keep track of rule metrics
    pub stardoc: stardoc::StarDoc,         // used to keep track of rule documentation
    pub settings: ws::Settings,
    pub is_any_digest_updated: bool,
    pub minimum_version: semver::Version,
    pub store: store::Store,
}

impl Workspace {
    pub fn update_locks(&mut self, locks: &HashMap<Arc<str>, Arc<str>>) {
        for (key, value) in locks.iter() {
            self.locks.insert(key.clone(), value.clone());
        }
    }

    pub fn update_rule_metrics(&mut self, rule_name: &str, elapsed_time: std::time::Duration) {
        self.rule_metrics.insert(
            rule_name.into(),
            RuleMetrics {
                elapsed_time: elapsed_time.as_secs_f64(),
            },
        );
    }

    pub fn update_minimum_version(&mut self, version: &semver::Version) {
        if *version > self.minimum_version {
            self.minimum_version = version.clone();
        }
    }

    pub fn is_dev_branch(&self, rule_name: &str) -> bool {
        if self.settings.json.dev_branches.contains(&rule_name.into()) {
            true
        } else {
            for item in &self.settings.json.dev_branches {
                // dev branch can be specified as just the location of the repository
                if rule_name.ends_with(item.as_ref()) {
                    return true;
                }
            }
            false
        }
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

    pub fn add_checkout_asset(&mut self, path: Arc<str>, contents: Arc<str>) {
        self.settings.checkout.insert_asset(path, contents);
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
        self.is_reproducible
    }

    pub fn set_is_not_reproducible(&mut self) {
        self.is_reproducible = false;
    }

    pub fn find_workspace_root(current_working_directory: &str) -> anyhow::Result<Arc<str>> {
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
            if let Some(file_name) = entry.file_name().to_str() {
                if file_name.starts_with('.') {
                    return false;
                }
            } else {
                // skip directories with non UTF-8 names
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
        is_clear_inputs: IsClearInputs,
        input_script_names: Option<Vec<Arc<str>>>,
        is_checkout_phase: IsCheckoutPhase,
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
        modules.push((ENV_FILE_NAME.into(), env_content.clone()));

        let mut is_run_or_inspect = true;
        let (mut settings, is_json_available) = ws::Settings::load();

        if let Some(required_version) = settings.json.minimum_version.as_ref() {
            logger(&mut progress)
                .info(format!("Minimum Required version: {required_version}",).as_str());
            let current_semver = singleton::get_spaces_version()
                .context(format_context!("While checking minimum version"))?;

            let required_semver = required_version.parse::<semver::Version>().context(
                format_context!(
                    "Required version in .spaces/settings.spaces.json is invalid {required_version}"
                ),
            )?;

            if required_semver > current_semver {
                let exec_path = std::env::current_exe()
                    .context(format_context!("Failed to get current executable path"))?;

                return Err(format_error!(
                    r#"
  - This workspaces requires spaces version {required_version}.
  - Spaces is executing version `{current_semver}` from `{}`.
  - Use `spaces version fetch --tag=v{required_version}` to update."#,
                    exec_path.display()
                ));
            }
        }

        if singleton::get_is_use_locks() {
            settings.json.is_use_locks = Some(true);
        }

        if is_checkout_phase == IsCheckoutPhase::Yes {
            settings.json.scanned_modules = HashSet::default();
        }

        settings.json.assets.insert(
            ENV_FILE_NAME.into(),
            ws::Asset::new_contents(env_content.as_ref()),
        );

        settings
            .json
            .dev_branches
            .extend(singleton::get_new_branches());

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
                            .debug(format!("Loading module from sync order: {module}").as_str());
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
            logger(&mut progress).debug(format!("Digesting {name}").as_str());
        }

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
        if (!settings.json.is_scanned.unwrap_or(false)
            || singleton::get_is_rescan()
            || settings.bin.star_files.is_empty())
            && is_checkout_phase == IsCheckoutPhase::No
        {
            // if the workspace is scanned, this will save settings on exit
            singleton::set_rescan(true);

            // if this is a checkout, we need to scan on first run/inspect
            settings.json.is_scanned = Some(is_run_or_inspect);

            logger(&mut progress).debug(format!("Scanning {absolute_path}").as_str());

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
                        logger(&mut progress).debug(format!("star file {stripped_path}").as_str());

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

            if is_checkout_phase == IsCheckoutPhase::Yes {
                settings.json.is_scanned = None;
            }
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
        let is_dirty = settings_is_dirty != ws::IsDirty::No;
        if is_dirty {
            logger(&mut progress).info(format!("is dirty {settings_is_dirty}").as_str());
        }

        // message the user of changed modules
        for updated in updated_modules {
            logger(&mut progress).debug(format!("dirty {updated}").as_str());
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
        if is_clear_inputs == IsClearInputs::Yes {
            settings
                .clear_inputs()
                .context(format_context!("Failed to clear inputs"))?;
        }

        // Workspace is assumed to reproducible until a rule is processed
        // that is not reproducible - such as a repo on tip of branch
        let mut env = environment::AnyEnvironment::default();

        Self::insert_automatic_vars(&mut env);
        let args_env = singleton::get_args_env();
        env.insert_assign_from_args(&args_env);

        if !args_env.is_empty() {
            // scripts may read ENV variables
            // so they need to rerun if any are passed on the command line
            settings.bin.is_always_evaluate = true;
        }

        Ok(Self {
            modules,
            absolute_path,
            log_directory,
            is_reproducible: true,
            is_create_lock_file: false,
            locks: HashMap::new(),
            env,
            is_dirty,
            is_env_set: false,
            is_bin_dirty: is_dirty,
            minimum_version: semver::Version::new(0, 0, 0),
            updated_assets: HashSet::new(),
            rule_metrics: HashMap::new(),
            stardoc: stardoc::StarDoc::default(),
            trailing_args: vec![],
            target: None,
            relative_invoked_path,
            settings,
            is_any_digest_updated: false,
            store: store::Store::default(),
        })
    }

    /// This should be called exactly once when the env.spaces.star file is evaluated
    pub fn set_env_from_workspace_builtin(
        &mut self,
        env: environment::AnyEnvironment,
    ) -> anyhow::Result<()> {
        if self.is_env_set {
            if singleton::is_lsp_mode() {
                return Ok(());
            }

            return Err(format_error!(
                "workspace.set_env() can only be called once and only from `env.spaces.star`"
            ));
        }

        self.env = env;
        Self::insert_automatic_vars(&mut self.env);
        self.is_env_set = true;

        if singleton::is_sync() {
            self.env.retain_vars_from_args();
        }

        let env_json = serde_json::to_string(&self.env)
            .context(format_context!("Internal Error: failed to serialize env"))?;
        self.settings.bin.env_json = env_json.into();

        Ok(())
    }

    /// This is called by checkout rules to update the environment.
    pub fn update_env(&mut self, env: environment::AnyEnvironment) -> anyhow::Result<()> {
        self.env.append(env);
        Ok(())
    }

    pub fn add_store_entry(&mut self, path_in_store: Arc<str>) -> anyhow::Result<()> {
        self.store
            .add_entry(std::path::Path::new(path_in_store.as_ref()))
            .context(format_context!("while adding store entry"))?;
        Ok(())
    }

    pub fn finalize_store(&self) -> anyhow::Result<()> {
        let store_path = ws::get_checkout_store_path();
        let mut saved_store =
            store::Store::new_from_store_path(std::path::Path::new(store_path.as_ref())).context(
                format_context!("Failed to load store manifest from {}", store_path),
            )?;

        saved_store.merge(self.store.clone());

        saved_store
            .save(std::path::Path::new(store_path.as_ref()))
            .context(format_context!("Failed to save store in {}", store_path))?;

        Ok(())
    }

    fn get_automatic_vars(&self) -> HashMap<&'static str, Arc<str>> {
        let mut vars = HashMap::new();
        vars.insert(AUTOMATIC_WORKSPACE_ABSOLUTE_PATH, self.get_absolute_path());
        vars.insert(AUTOMATIC_WORKSPACE_DIGEST, self.get_short_digest());
        vars.insert(AUTOMATIC_WORKSPACE_STORE_PATH, self.get_store_path());
        vars.insert(
            AUTOMATIC_WORKSPACE_IS_REPRODUCIBLE,
            self.is_reproducible().to_string().into(),
        );
        vars
    }

    fn insert_automatic_vars(env: &mut environment::AnyEnvironment) {
        const AUTOMATIC_VARS: &[(&str, &str)] = &[
            (
                AUTOMATIC_WORKSPACE_ABSOLUTE_PATH,
                "Abosolute path to the workspace",
            ),
            (
                AUTOMATIC_WORKSPACE_DIGEST,
                "Workspace digest (empty if no reproducible)",
            ),
            (
                AUTOMATIC_WORKSPACE_IS_REPRODUCIBLE,
                "`true` if the workspace is reproducible",
            ),
            (
                AUTOMATIC_WORKSPACE_STORE_PATH,
                "Absolute path to the spaces store used by this workspace",
            ),
        ];

        for (key, help) in AUTOMATIC_VARS {
            env.insert_or_update(environment::Any {
                name: (*key).into(),
                value: environment::Value::Automatic,
                source: None,
                help: Some((*help).into()),
            });
        }
    }

    pub fn insert_automatic_var_placeholders(&self, env: &mut environment::AnyEnvironment) {
        let auto_vars = self.get_automatic_vars();
        env.replace_values_with_automatic_placeholders(&auto_vars);
        Self::insert_automatic_vars(env);
    }

    pub fn get_env_vars(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut env_vars = self
            .env
            .get_vars()
            .context(format_context!("While getting workspace env vars"))?;

        let automatic_vars = self.get_automatic_vars();

        for (_replace_key, replace_value) in env_vars.iter_mut() {
            let mut new_value = replace_value.to_string();
            for (with_key, with_value) in automatic_vars.iter() {
                let automatic_token = environment::Value::get_automatic_placeholder(with_key);
                new_value = new_value.replace(&automatic_token, with_value.as_ref());
            }
            *replace_value = new_value.into();
        }

        Ok(env_vars)
    }

    pub fn get_secret_values(&self) -> anyhow::Result<Vec<Arc<str>>> {
        self.env.get_secret_values()
    }

    pub fn is_env_var_set(&self, key: &str) -> bool {
        self.env.is_env_var_set(key)
    }

    pub fn is_env_var_set_to(&self, key: &str, value: &str) -> bool {
        self.env.is_env_var_set_to(key, value)
    }

    pub fn get_env_mut(&mut self) -> &mut environment::AnyEnvironment {
        &mut self.env
    }

    pub fn save_env_file(&mut self, modules: &[(Arc<str>, Arc<str>)]) -> anyhow::Result<()> {
        let auto_vars = self.get_automatic_vars();
        let env_markdown = self
            .env
            .to_markdown(&auto_vars)
            .context(format_context!("Failed to convert environment to markdown"))?;

        let env_str = serde_json::to_string_pretty(&self.env)?;

        save_env_file_at(
            std::path::Path::new(self.absolute_path.as_ref()),
            &env_markdown,
            &env_str,
        )
        .context(format_context!("Failed to save workspace env file"))?;

        if self.is_reproducible() {
            self.settings.json.digest = Some(calculate_digest(&env_str, modules));
        } else {
            self.settings.json.digest = None;
        }

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
        inputs: &rule::InputsOutputs,
    ) -> anyhow::Result<()> {
        self.is_bin_dirty = true;
        self.settings
            .bin
            .changes
            .update_from_inputs(progress, inputs)
            .context(format_context!("Failed to update workspace changes"))?;

        Ok(())
    }

    pub fn inspect_inputs(
        &self,
        progress: &mut printer::MultiProgressBar,
        inputs: &rule::InputsOutputs,
    ) -> anyhow::Result<Vec<String>> {
        let globs = inputs.get_globs();
        self.settings
            .bin
            .changes
            .inspect_inputs(progress, &globs)
            .context(format_context!("Failed to inspect workspace inputs"))
    }

    pub fn add_git_commit_lock(&mut self, rule_name: &str, commit: Arc<str>) {
        self.locks.insert(rule_name.into(), commit);
    }

    pub fn get_short_digest(&self) -> Arc<str> {
        let digest = self.settings.json.digest.clone().unwrap_or_default();
        get_short_digest(&digest)
    }

    pub fn get_store_path(&self) -> Arc<str> {
        self.settings.json.store_path.clone()
    }

    pub fn get_spaces_tools_path(&self) -> Arc<str> {
        ws::get_spaces_tools_path(self.get_store_path().as_ref())
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
        inputs: &rule::InputsOutputs,
    ) -> anyhow::Result<Option<Arc<str>>> {
        let globs = inputs.get_globs();
        let digest = self
            .settings
            .bin
            .changes
            .get_digest(progress, seed, &globs)
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
        self.is_any_digest_updated = true;
        self.settings.bin.inputs.save_digest(rule, digest);
    }
}
