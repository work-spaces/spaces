use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{anyhow_error, format_error_context};

pub const SPACES_OVERLAY: &str = "{SPACES_OVERLAY}";
pub const SPACE: &str = "{SPACE}";
pub const USER: &str = "{USER}";
pub const UNIQUE: &str = "{UNIQUE}";
pub const SPACES_SYSROOT: &str = "{SPACES_SYSROOT}";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CheckoutOption {
    Revision,
    Branch,
    Artifact,
    Develop,
}

pub enum Checkout {
    Artifact(String),
    ReadOnly(String),
    ReadOnlyBranch(String),
    Develop(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Dependency {
    /// The git https or ssh URL
    pub git: String,
    /// The revision of the dependency. This can be a commit digest, a branch name, or a tag.
    pub rev: Option<String>,
    /// The branch associated with the dependency.
    pub branch: Option<String>,
    /// The URL to the artifact tar.gz file
    pub artifact: Option<String>,
    /// The branch associated with the dependency.
    pub checkout: Option<CheckoutOption>,
    /// The branch associated with the dependency.
    pub dev: Option<String>,
}

impl Dependency {
    pub fn get_checkout(&self) -> anyhow::Result<Checkout> {
        match &self.checkout {
            Some(CheckoutOption::Artifact) => {
                if let Some(value) = &self.artifact {
                    Ok(Checkout::Artifact(value.clone()))
                } else {
                    Err(anyhow::anyhow!(
                        "No `artifact` found for dependency {}",
                        self.git
                    ))
                }
            }
            Some(CheckoutOption::Revision) => {
                if let Some(value) = &self.rev {
                    Ok(Checkout::ReadOnly(value.clone()))
                } else {
                    Err(anyhow::anyhow!(
                        "No `rev` found for dependency {}",
                        self.git
                    ))
                }
            }
            Some(CheckoutOption::Branch) => {
                if let Some(value) = &self.branch {
                    Ok(Checkout::ReadOnlyBranch(value.clone()))
                } else {
                    Err(anyhow::anyhow!(
                        "No `branch` found for dependency {}",
                        self.git
                    ))
                }
            }
            Some(CheckoutOption::Develop) => {
                if let Some(value) = &self.dev {
                    Ok(Checkout::Develop(value.clone()))
                } else {
                    Err(anyhow::anyhow!(
                        "No `dev` found for dependency {}",
                        self.git
                    ))
                }
            }
            None => {
                if let Some(value) = &self.rev {
                    Ok(Checkout::ReadOnly(value.clone()))
                } else if let Some(value) = &self.branch {
                    Ok(Checkout::ReadOnlyBranch(value.clone()))
                } else {
                    Err(anyhow::anyhow!(
                        "No checkout option found for dependency {}. Please specify a `branch`, `rev`, or `artifact`",
                        self.git
                    ))
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateArchive {
    pub input: String,
    pub output: String,
    pub platform_archives: String,
    pub executables: Option<Vec<String>>,
    pub macos_x86_64: Option<String>,
    pub macos_aarch64: Option<String>,
    pub windows_x86_64: Option<String>,
    pub windows_aarch64: Option<String>,
    pub linux_x86_64: Option<String>,
    pub linux_aarch64: Option<String>,
}

impl CreateArchive {
    const FILE_NAME: &'static str = "spaces_create_archive.toml";

    pub fn new(path: &str) -> anyhow::Result<Self> {
        let file_path = format!("{path}/{}", Self::FILE_NAME); //change to spaces_dependencies.toml
        let contents = std::fs::read_to_string(&file_path)
            .with_context(|| format!("Failed to read create archive file {file_path}"))?;

        let result: Self = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse create archive file {file_path}"))?;

        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ArchiveLink {
    None,
    Hard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub url: String,
    pub sha256: String,
    pub link: ArchiveLink,
    pub files: Option<Vec<String>>,
    pub strip_prefix: Option<String>,
    pub add_prefix: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum Platform {
    MacosX86_64,
    MacosAarch64,
    WindowsX86_64,
    WindowsAarch64,
    LinuxX86_64,
    LinuxAarch64,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::MacosX86_64 => write!(f, "macos-x86_64"),
            Platform::MacosAarch64 => write!(f, "macos-aarch64"),
            Platform::WindowsX86_64 => write!(f, "windows-x86_64"),
            Platform::WindowsAarch64 => write!(f, "windows-aarch64"),
            Platform::LinuxX86_64 => write!(f, "linux-x86_64"),
            Platform::LinuxAarch64 => write!(f, "linux-aarch64"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformArchive {
    pub macos_x86_64: Option<Archive>,
    pub macos_aarch64: Option<Archive>,
    pub windows_aarch64: Option<Archive>,
    pub windows_x86_64: Option<Archive>,
    pub linux_x86_64: Option<Archive>,
    pub linux_aarch64: Option<Archive>,
}

impl PlatformArchive {
    pub fn get_archive(&self) -> Option<Archive> {
        if cfg!(target_os = "macos") {
            if cfg!(target_arch = "x86_64") {
                return self.macos_x86_64.clone();
            } else if cfg!(target_arch = "aarch64") {
                return self.macos_aarch64.clone();
            }
        } else if cfg!(target_os = "windows") {
            if cfg!(target_arch = "x86_64") {
                return self.windows_x86_64.clone();
            } else if cfg!(target_arch = "aarch64") {
                return self.windows_aarch64.clone();
            }
        } else if cfg!(target_os = "linux") {
            if cfg!(target_arch = "x86_64") {
                return self.linux_x86_64.clone();
            } else if cfg!(target_arch = "aarch64") {
                return self.linux_aarch64.clone();
            }
        }
        None
    }

    pub fn get_archive_from_platform(&self, platform: Platform) -> Option<Archive> {
        match platform {
            Platform::MacosX86_64 => self.macos_x86_64.clone(),
            Platform::MacosAarch64 => self.macos_aarch64.clone(),
            Platform::WindowsX86_64 => self.windows_x86_64.clone(),
            Platform::WindowsAarch64 => self.windows_aarch64.clone(),
            Platform::LinuxX86_64 => self.linux_x86_64.clone(),
            Platform::LinuxAarch64 => self.linux_aarch64.clone(),
        }
    }

    pub fn get_archive_from_platform_mut(&mut self, platform: Platform) -> Option<&mut Archive> {
        match platform {
            Platform::MacosX86_64 => self.macos_x86_64.as_mut(),
            Platform::MacosAarch64 => self.macos_aarch64.as_mut(),
            Platform::WindowsX86_64 => self.windows_x86_64.as_mut(),
            Platform::WindowsAarch64 => self.windows_aarch64.as_mut(),
            Platform::LinuxX86_64 => self.linux_x86_64.as_mut(),
            Platform::LinuxAarch64 => self.linux_aarch64.as_mut(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum AssetType {
    HardLink,
    Template,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceAsset {
    pub path: String,
    #[serde(rename = "type")]
    pub type_: AssetType,
}

impl WorkspaceAsset {
    pub fn apply(
        &self,
        workspace_path: &str,
        dependency_name: &str,
        key: &str,
    ) -> anyhow::Result<()> {
        let path = format!("{workspace_path}/{dependency_name}/{key}");
        let dest_path = format!("{workspace_path}/{}", self.path);

        match self.type_ {
            AssetType::HardLink => {
                if std::path::Path::new(dest_path.as_str()).exists() {
                    std::fs::remove_file(dest_path.as_str())
                        .with_context(|| format_error_context!("While removing {dest_path}"))?;
                }
                std::fs::hard_link(path.as_str(), dest_path.as_str()).with_context(|| {
                    format_error_context!("While creating hard link from {path} to {dest_path}")
                })?;
            }
            AssetType::Template => {
                let contents = std::fs::read_to_string(&path)
                    .with_context(|| format_error_context!("While reading {path}"))?;

                // do the substitutions

                // create a copy
                std::fs::write(dest_path.as_str(), contents)
                    .with_context(|| format_error_context!("While writing to {dest_path}"))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Deps {
    pub deps: HashMap<String, Dependency>,
    pub archives: Option<HashMap<String, Archive>>,
    pub platform_archives: Option<HashMap<String, PlatformArchive>>,
    pub assets: Option<HashMap<String, WorkspaceAsset>>,
    pub vscode: Option<VsCodeConfig>,
}

impl Deps {
    const FILE_NAME: &'static str = "spaces_deps.toml";

    pub fn new(path: &str) -> anyhow::Result<Option<Self>> {
        let file_path = format!("{path}/{}", Self::FILE_NAME); //change to spaces_dependencies.toml
        let contents = std::fs::read_to_string(&file_path)
            .with_context(|| format!("Failed to read deps file {file_path}"));

        if let Ok(contents) = contents {
            let result: Self = toml::from_str(&contents)
                .with_context(|| format!("Failed to parse deps file {file_path}"))?;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let file_path = format!("{path}/{}", Self::FILE_NAME); //change to spaces_dependencies.toml
        let contents = toml::to_string(&self).with_context(|| {
            format_error_context!("failed to build toml string for {file_path}")
        })?;
        std::fs::write(&file_path, contents)
            .with_context(|| format_error_context!("failed write file to {file_path}"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoConfig {
    pub patches: Option<HashMap<String, Vec<String>>>,
    pub net: Option<HashMap<String, String>>,
    pub http: Option<HashMap<String, String>>,
    pub build: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuckConfig {
    pub cells: Option<HashMap<String, String>>,
    pub cell_aliases: Option<HashMap<String, String>>,
    pub parser: Option<HashMap<String, String>>,
    pub project: Option<HashMap<String, String>>,
    pub build: Option<HashMap<String, String>>,
}

impl BuckConfig {
    fn stringify(name: &str, map: &Option<HashMap<String, String>>) -> String {
        let mut result = format!("[{name}]\n");

        if let Some(map) = map.as_ref() {
            for (key, value) in map.iter() {
                result.push_str(&format!("    {} = {}\n", key, value));
            }
        }
        result.push('\n');
        result
    }

    pub fn export(&self, path: &str) -> anyhow::Result<()> {
        let file_path = format!("{path}/.buckconfig");
        let mut contents = String::new();
        contents.push_str(&Self::stringify("cells", &self.cells));
        contents.push_str(&Self::stringify("cell_aliases", &self.cell_aliases));
        contents.push_str(&Self::stringify("parser", &self.parser));
        contents.push_str(&Self::stringify("project", &self.project));
        std::fs::write(&file_path, contents)
            .with_context(|| format!("Failed to write buckconfig file {file_path}"))?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VsCodeTask {
    #[serde(rename = "type")]
    pub type_: String,
    pub command: String,
    #[serde(rename = "problemMatcher")]
    pub problem_matcher: Vec<String>,
    pub arguments: Vec<String>,
    pub options: HashMap<String, String>,
    pub label: String,
    pub group: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VsCodeExtensions {
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VsCodeConfig {
    tasks: Option<Vec<VsCodeTask>>,
    settings: Option<HashMap<String, toml::Value>>,
    extensions: Option<VsCodeExtensions>,
}

impl VsCodeConfig {
    fn load_json_file(
        path: &str,
        default_value: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let contents = std::fs::read(path);
        let result = if let Ok(contents) = contents {
            serde_json::from_slice(&contents)
                .with_context(|| format_error_context!("failed to parse {path} as JSON"))?
        } else {
            default_value
        };
        Ok(result)
    }

    fn save_json_file(path: &str, value: serde_json::Value) -> anyhow::Result<()> {
        let contents = serde_json::to_string_pretty(&value)
            .with_context(|| format_error_context!("failed to serialize JSON to {path}"))?;
        std::fs::write(path, contents)
            .with_context(|| format_error_context!("failed to write JSON to {path}"))?;
        Ok(())
    }

    pub fn apply(&self, workspace_path: &str) -> anyhow::Result<()> {
        let vs_code_directory = format!("{workspace_path}/.vscode");
        std::fs::create_dir_all(vs_code_directory.as_str()).with_context(|| {
            format_error_context!("failed to create director {vs_code_directory}")
        })?;

        if let Some(own_tasks) = self.tasks.as_ref() {
            let tasks_file = format!("{vs_code_directory}/tasks.json");
            let mut tasks = Self::load_json_file(
                tasks_file.as_str(),
                serde_json::json!({
                    "version": "2.0.0",
                    "tasks": []
                }),
            )
            .with_context(|| format_error_context!("while loading {tasks_file}"))?;

            let tasks_list = tasks
                .as_object_mut()
                .and_then(|e| e.get_mut("tasks"))
                .and_then(|e| e.as_array_mut())
                .ok_or(anyhow::anyhow!(
                    "Failed to get tasks from {tasks_file} JSON object"
                ))?;

            for task in own_tasks.iter() {
                let entry = serde_json::to_value(task).with_context(|| {
                    format_error_context!("Internal Error: failed to serialize task {task:?}")
                })?;
                tasks_list.push(entry);
            }

            Self::save_json_file(tasks_file.as_str(), tasks)
                .with_context(|| format_error_context!("while saving {tasks_file}"))?;
        }

        if let Some(own_settings) = self.settings.as_ref() {
            let settings_file = format!("{vs_code_directory}/settings.json");
            let mut settings = Self::load_json_file(settings_file.as_str(), serde_json::json!({}))
                .with_context(|| format_error_context!("while loading {settings_file}"))?;

            let settings_object = settings.as_object_mut().ok_or(anyhow::anyhow!(
                "Failed to get settings from {settings_file} JSON object"
            ))?;

            for (key, value) in own_settings {
                let json_value = serde_json::to_value(value).with_context(|| {
                    format_error_context!("toml value {value:?} cannot be converted to JSON")
                })?;

                settings_object.insert(key.clone(), json_value);
            }
            Self::save_json_file(settings_file.as_str(), settings)
                .with_context(|| format_error_context!("while saving {settings_file}"))?;
        }

        if let Some(own_extensions) = self.extensions.as_ref() {
            let extensions_file = format!("{vs_code_directory}/extensions.json");
            let mut extensions = Self::load_json_file(
                extensions_file.as_str(),
                serde_json::json!({"recommendations": []}),
            )
            .with_context(|| format_error_context!("while loading {extensions_file}"))?;

            let extensions_object = extensions.as_object_mut().ok_or(anyhow_error!(
                "Failed to get extensions from {extensions_file} JSON object"
            ))?;
            if !extensions_object.contains_key("recommendations") {
                extensions_object.insert("recommendations".to_string(), serde_json::json!({}));
            }

            let recommendations_array = extensions_object
                .get_mut("recommendations")
                .and_then(|e| e.as_array_mut())
                .ok_or(anyhow_error!(
                "Internl Erorr: Failed to get recommendations from {extensions_file} JSON object"
            ))?;

            for value in own_extensions.recommendations.iter() {
                let value = serde_json::Value::String(value.clone());
                if !recommendations_array.contains(&value) {
                    recommendations_array.push(value);
                }
            }

            Self::save_json_file(&extensions_file, extensions)
                .with_context(|| format_error_context!("while saving {extensions_file}"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceConfigSettings {
    pub branch: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct WorkspaceConfig {
    pub repositories: HashMap<String, Dependency>,
    pub buck: Option<BuckConfig>,
    pub cargo: Option<CargoConfig>,
    pub settings: Option<WorkspaceConfigSettings>,
    pub vscode: Option<VsCodeConfig>,
}

impl WorkspaceConfig {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read spaces workspace config file {path}"))?;
        let result: WorkspaceConfig = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse spaces workspace config file {path}"))?;
        Ok(result)
    }

    pub fn to_workspace(&self, space_name: &str) -> anyhow::Result<Workspace> {
        let mut repositories = self.repositories.clone();
        for (_key, dependency) in repositories.iter_mut() {
            let mut dev_branch = if let Some(branch) = self
                .settings
                .as_ref()
                .and_then(|e| e.branch.as_ref())
                .as_ref()
            {
                (*branch).to_owned()
            } else {
                format!("user/{USER}/{SPACE}-{UNIQUE}")
            };

            if dev_branch.contains(USER) {
                let user = std::env::var("USER").with_context(|| {
                    format!("Failed to replace {USER} with $USER for {dev_branch} naming")
                })?;
                dev_branch = dev_branch.replace(USER, &user);
            }

            if dev_branch.contains(SPACE) {
                dev_branch = dev_branch.replace(SPACE, space_name);
            } else {
                return Err(anyhow::anyhow!(
                    "Branch name {dev_branch} must contain {SPACE}"
                ));
            }

            if dev_branch.contains(UNIQUE) {
                //create a unique digest from the current time
                let unique = format!(
                    "{dev_branch}{}",
                    std::time::Instant::now().elapsed().as_nanos()
                );
                let unique_sha256 = sha256::digest(unique.as_bytes());
                let unique_start = unique_sha256.as_str()[0..8].to_string();

                dev_branch = dev_branch.replace(UNIQUE, unique_start.as_str());
            }

            dependency.dev = Some(dev_branch);
            dependency.checkout = Some(CheckoutOption::Develop);
        }
        Ok(Workspace {
            repositories,
            buck: self.buck.clone(),
            cargo: self.cargo.clone(),
            dependencies: HashMap::new(),
            assets: None,
            vscode: self.vscode.clone(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Workspace {
    pub repositories: HashMap<String, Dependency>,
    pub dependencies: HashMap<String, Dependency>,
    pub buck: Option<BuckConfig>,
    pub cargo: Option<CargoConfig>,
    pub assets: Option<HashMap<String, WorkspaceAsset>>,
    pub vscode: Option<VsCodeConfig>,
}

impl Workspace {
    const FILE_NAME: &'static str = "spaces_workspace.toml";

    pub fn new(path: &str) -> anyhow::Result<Self> {
        let file_path = format!("{path}/{}", Self::FILE_NAME);
        let contents = std::fs::read_to_string(&file_path)
            .with_context(|| format!("Failed to read workspace file {file_path}"))?;
        let result: Workspace = toml::from_str(&contents)
            .with_context(|| format!("Failed to workspace file {file_path}"))?;
        Ok(result)
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let file_path = format!("{path}/{}", Self::FILE_NAME);
        let contents = toml::to_string(&self)
            .with_context(|| format!("Failed to serialize workspace {self:?}"))?;

        std::fs::write(&file_path, contents)
            .with_context(|| format!("Failed to save workspace file {file_path}"))?;
        Ok(())
    }

    pub fn get_cargo_patches(&self) -> Option<&HashMap<String, Vec<String>>> {
        self.cargo.as_ref().and_then(|e| e.patches.as_ref())
    }

    pub fn get_cargo_build(&self) -> Option<&HashMap<String, String>> {
        self.cargo.as_ref().and_then(|e| e.build.as_ref())
    }

    pub fn get_cargo_net(&self) -> Option<&HashMap<String, String>> {
        self.cargo.as_ref().and_then(|e| e.net.as_ref())
    }

    pub fn get_cargo_http(&self) -> Option<&HashMap<String, String>> {
        self.cargo.as_ref().and_then(|e| e.http.as_ref())
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Ledger {
    pub workspaces: HashMap<String, Workspace>,
}

impl Ledger {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read ledger file {path}"))?;
        let result: Ledger = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse ledger file {path}"))?;
        Ok(result)
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let contents = toml::to_string(&self).with_context(|| "Failed to serialize ledger")?;
        std::fs::write(path, contents)
            .with_context(|| format!("Failed to save ledger file {path}"))?;
        Ok(())
    }
}
