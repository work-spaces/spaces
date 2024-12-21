use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use crate::inputs;

pub const ENV_FILE_NAME: &str = "env.spaces.star";
pub const LOCK_FILE_NAME: &str = "lock.spaces.star";
pub const SPACES_MODULE_NAME: &str = "spaces.star";
pub const SPACES_STDIN_NAME: &str = "stdin.star";
pub const SPACES_LOGS_NAME: &str = "@logs";
pub const SPACES_CAPSULES_NAME: &str = "@capsules";
pub const SPACES_CAPSULES_INFO_NAME: &str = "capsules.spaces.json";
const SETTINGS_FILE_NAME: &str = "settings.spaces.json";
const SPACES_HOME_ENV_VAR: &str = "SPACES_HOME";
pub const SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE: &str = "SPACES_IS_WORKSPACE_REPRODUCIBLE";
pub const SPACES_ENV_WORKSPACE_DIGEST: &str = "SPACES_WORKSPACE_DIGEST";
pub const WORKSPACE_FILE_HEADER: &str = r#"
"""
Spaces Environment Workspace file
"""
"#;

pub type WorkspaceArc = std::sync::Arc<lock::StateLock<Workspace>>;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Settings {
    pub store_path: Arc<str>,
    order: Vec<Arc<str>>,
}

impl Settings {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let load_path = format!("{path}/{SETTINGS_FILE_NAME}");
        let content = std::fs::read_to_string(load_path.as_str()).context(format_context!(
            "Failed to read load order file {load_path}"
        ))?;
        let order: Settings = serde_json::from_str(content.as_str()).context(format_context!(
            "Failed to parse load order file {load_path}"
        ))?;
        Ok(order)
    }

    pub fn push(&mut self, module: Arc<str>) {
        self.order.push(module);
    }

    pub fn save(&self, workspace_path: &str) -> anyhow::Result<()> {
        let path = format!("{workspace_path}/{SETTINGS_FILE_NAME}");
        let content = serde_json::to_string_pretty(&self)
            .context(format_context!("Failed to serialize load order"))?;
        std::fs::write(path.as_str(), content.as_str())
            .context(format_context!("Failed to write load order file {path}"))?;

        Ok(())
    }
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

pub fn get_checkout_store_path() -> Arc<str> {
    if let Ok(spaces_home) = std::env::var(SPACES_HOME_ENV_VAR) {
        return format!("{}/.spaces/store", spaces_home).into();
    }
    if let Ok(Some(home_path)) = homedir::my_home() {
        return format!("{}/.spaces/store", home_path.to_string_lossy()).into();
    }
    panic!("Failed to get home directory");
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
    pub absolute_path: Arc<str>, // set at startup
    pub log_directory: Arc<str>, // always @logs/timestamp
    pub is_create_lock_file: bool, // set at startup
    pub digest: Arc<str>, // set at startup
    pub store_path: Option<Arc<str>>, // set at startup
    pub locks: HashMap<Arc<str>, Arc<str>>, // set during eval
    pub env: environment::Environment, // set during eval
    #[allow(dead_code)]
    pub new_branch_name: Option<Arc<str>>, // set during eval - not used
    changes: changes::Changes, // modified during run
    inputs: inputs::Inputs, // modified during run
    pub updated_assets: HashSet<Arc<str>>, // used by assets to keep track of exclusive access
}

impl Workspace {

    pub fn set_is_reproducible(&mut self, value: bool) {
        self.env.vars.insert(
            SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE.into(),
            value.to_string().into(),
        );
    }

    pub fn get_relative_directory(&self, relative_path: &str) -> Arc<str> {
        format!("{}/{}", self.absolute_path, relative_path).into()
    }

    pub fn get_absolute_path(&self) -> Arc<str> {
        self.absolute_path.clone()
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

    fn filter_predicate(entry: &walkdir::DirEntry) -> bool {
        if entry.file_name() == SPACES_CAPSULES_NAME {
            return false;
        }
        true
    }

    pub fn new(mut progress: printer::MultiProgressBar, absolute_path_to_workspace: Option<Arc<str>>) -> anyhow::Result<Self> {
        let date = chrono::Local::now();

        let absolute_path = if let Some(absolute_path) = absolute_path_to_workspace {
            absolute_path
        } else {
            let current_working_directory = get_current_working_directory().context(
                format_context!("Failed to get current working directory in new workspace"),
            )?;

            // search the current directory and all parent directories for the workspace file
            Self::find_workspace_root(current_working_directory.as_ref())
                .context(format_context!("While searching for workspace root"))?
        };

        // walkdir and find all spaces.star files in the workspace
        let walkdir: Vec<_> = walkdir::WalkDir::new(absolute_path.as_ref())
            .into_iter()
            .filter_entry(Self::filter_predicate)
            .collect();

        progress.set_total(walkdir.len() as u64);

        let mut loaded_modules = HashSet::new();
        let mut modules: Vec<(Arc<str>, Arc<str>)> = vec![];

        let env_content: Arc<str> = std::fs::read_to_string(format!("{}/{}", absolute_path, ENV_FILE_NAME))
            .context(format_context!(
            "Failed to read workspace file: {ENV_FILE_NAME}"
        ))?.into();

        loaded_modules.insert(ENV_FILE_NAME.into());
        modules.push((ENV_FILE_NAME.into(), env_content));

        let mut original_modules: Vec<(Arc<str>, Arc<str>)> = vec![];

        let mut store_path = None;
        if let Ok(load_order) = Settings::load(absolute_path.as_ref()) {
            progress.log(printer::Level::Trace, "Loading modules from sync order");
            store_path = Some(load_order.store_path);
            for module in load_order.order {
                if is_rules_module(module.as_ref()) {
                    progress.increment(1);
                    let path = format!("{}/{}", absolute_path, module);
                    let content = std::fs::read_to_string(path.as_str())
                        .context(format_context!("Failed to read file {}", path))?;
                    if !loaded_modules.contains(&module) {
                        progress.log(
                            printer::Level::Trace,
                            format!("Loading module from sync order: {}", module).as_str(),
                        );
                        loaded_modules.insert(module.clone());
                        original_modules.push((module, content.into()));
                    }
                }
            }
        } else {
            progress.log(
                printer::Level::Trace,
                format!("No sync order found at {absolute_path}").as_str(),
            );
        }

        for (name, _) in original_modules.iter() {
            progress.log(
                printer::Level::Message,
                format!("Digesting {}", name).as_str(),
            );
        }
        let workspace_digest = calculate_digest(&original_modules);
        modules.extend(original_modules);

        let mut unordered_modules: Vec<(Arc<str>, Arc<str>)> = vec![];

        for entry in walkdir {
            progress.increment(1);
            if let Ok(entry) = entry.context(format_context!("While walking directory")) {
                if entry.file_type().is_file()
                    && is_rules_module(entry.file_name().to_string_lossy().as_ref())
                {
                    let path: Arc<str> = entry.path().to_string_lossy().into();
                    let content: Arc<str> = std::fs::read_to_string(path.as_ref())
                        .context(format_context!("Failed to read file {path}"))?.into();

                    if let Some(stripped_path) = path.strip_prefix(format!("{}/", absolute_path).as_str()) {
                        if !loaded_modules.contains(stripped_path) {
                            progress.log(
                                printer::Level::Trace,
                                format!("Loading module from directory: {stripped_path}").as_str(),
                            );
                            loaded_modules.insert(stripped_path.into());
                            unordered_modules.push((stripped_path.into(), content));
                        }
                    }
                }
            }
        }

        unordered_modules.sort_by(|a, b| a.0.cmp(&b.0));
        modules.extend(unordered_modules);

        progress.log(
            printer::Level::Info,
            format!("Workspace working directory: {absolute_path}").as_str(),
        );

        std::env::set_current_dir(std::path::Path::new(absolute_path.as_ref())).context(
            format_context!("Failed to set current directory to {absolute_path}"),
        )?;

        let log_directory: Arc<str> = format!("{SPACES_LOGS_NAME}/logs_{}", date.format("%Y%m%d-%H-%M-%S")).into();

        std::fs::create_dir_all(log_directory.as_ref()).context(format_context!(
            "Failed to create log folder {log_directory}",
        ))?;

        std::fs::create_dir_all(build_directory())
            .context(format_context!("Failed to create build directory"))?;

        let changes_path = get_changes_path();
        let skip_folders = vec![SPACES_LOGS_NAME.into()];
        let changes = changes::Changes::new(changes_path, skip_folders);

        #[allow(unused)]
        let unique = get_unique().context(format_context!("failed to get unique marker"))?;

        let mut env = environment::Environment::default();

        env.vars.insert(
            SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE.into(),
            "true".into(),
        );

        
        Ok(Self { 
            modules,
            absolute_path,
            log_directory,
            is_create_lock_file: false,
            digest: workspace_digest,
            store_path,
            locks: HashMap::new(),
            env,
            new_branch_name: None,
            changes,
            updated_assets: HashSet::new(),
            inputs: inputs::Inputs::new(get_inputs_path()),
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

    pub fn update_changes(&mut self,
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
    
    pub fn get_rule_inputs_digest(&self,
        progress: &mut printer::MultiProgressBar,
        seed: &str,
        globs: &HashSet<Arc<str>>,
    ) -> anyhow::Result<Arc<str>> {
        self.changes.get_digest(progress, seed, globs)
    }

    pub fn get_store_path(&self) -> Arc<str> {
        self.store_path.clone().unwrap_or_else(get_checkout_store_path)
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
        let digest = self.get_rule_inputs_digest(progress, seed, inputs)
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
