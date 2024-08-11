use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{context, platform};
use anyhow_source_location::{format_context, format_error};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum CheckoutOption {
    Artifact,
    #[default]
    Revision,
    BranchHead,
    NewBranch,
}

pub enum Checkout {
    Artifact(String),
    Revision(String),
    BranchHead(String),
    NewBranch(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Dependency {
    /// The git https or ssh URL
    pub git: String,
    /// The revision of the dependency. This can be a commit digest or a tag.
    pub rev: Option<String>,
    /// The branch associated with the dependency.
    pub branch: Option<String>,
    /// The URL to the artifact tar.gz file
    pub artifact: Option<String>,
    /// The checkout option.
    pub checkout: CheckoutOption,
}

impl Dependency {
    pub fn get_checkout(&self) -> anyhow::Result<Checkout> {
        match &self.checkout {
            CheckoutOption::Artifact => {
                if let Some(value) = &self.artifact {
                    Ok(Checkout::Artifact(value.clone()))
                } else {
                    Err(anyhow::anyhow!(
                        "No `artifact` found for dependency {}",
                        self.git
                    ))
                }
            }
            CheckoutOption::Revision => {
                if let Some(value) = &self.rev {
                    Ok(Checkout::Revision(value.clone()))
                } else {
                    Err(anyhow::anyhow!(
                        "No `rev` found for dependency {}",
                        self.git
                    ))
                }
            }
            CheckoutOption::BranchHead => {
                if let Some(value) = &self.branch {
                    Ok(Checkout::BranchHead(value.clone()))
                } else {
                    Err(anyhow::anyhow!(
                        "No `branch` found for dependency {}",
                        self.git
                    ))
                }
            }
            CheckoutOption::NewBranch => {
                if let Some(value) = &self.branch {
                    Ok(Checkout::NewBranch((*value).clone()))
                } else {
                    Err(anyhow::anyhow!(
                        "No `branch` found for dependency {}",
                        self.git
                    ))
                }
            }
        }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArchiveDriver {
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.bz2")]
    TarBz2,
    #[serde(rename = "tar.7z")]
    Tar7z,
    #[serde(rename = "zip")]
    Zip,
}

impl ArchiveDriver {
    fn get_extension(&self) -> &'static str {
        match self {
            ArchiveDriver::TarGz => "tar.gz",
            ArchiveDriver::TarBz2 => "tar.bz2",
            ArchiveDriver::Tar7z => "tar.7z",
            ArchiveDriver::Zip => "zip",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateArchive {
    pub input: String,
    pub output: String,
    pub version: String,
    pub driver: ArchiveDriver,
    pub platform: Option<platform::Platform>,
    pub include_globs: Option<Vec<String>>,
    pub exclude_globs: Option<Vec<String>>,
}

impl CreateArchive {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path).context(format_context!("{path}"))?;
        let result: Self = toml::from_str(&contents).context(format_context!("{path}"))?;
        Ok(result)
    }

    pub fn get_output_file(&self) -> String {
        let mut result = format!("{}-{}", self.output, self.version);
        if let Some(platform) = self.platform.as_ref() {
            result.push_str(format!("-{}", platform).as_str());
        }
        result.push('.');
        result.push_str(self.driver.get_extension());
        result
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
        if let Some(platform) = platform::Platform::get_platform() {
            self.get_archive_from_platform(platform)
        } else {
            None
        }
    }

    pub fn get_archive_from_platform(&self, platform: platform::Platform) -> Option<Archive> {
        match platform {
            platform::Platform::MacosX86_64 => self.macos_x86_64.clone(),
            platform::Platform::MacosAarch64 => self.macos_aarch64.clone(),
            platform::Platform::WindowsX86_64 => self.windows_x86_64.clone(),
            platform::Platform::WindowsAarch64 => self.windows_aarch64.clone(),
            platform::Platform::LinuxX86_64 => self.linux_x86_64.clone(),
            platform::Platform::LinuxAarch64 => self.linux_aarch64.clone(),
        }
    }

    #[allow(dead_code)]
    pub fn get_archive_from_platform_mut(
        &mut self,
        platform: platform::Platform,
    ) -> Option<&mut Archive> {
        match platform {
            platform::Platform::MacosX86_64 => self.macos_x86_64.as_mut(),
            platform::Platform::MacosAarch64 => self.macos_aarch64.as_mut(),
            platform::Platform::WindowsX86_64 => self.windows_x86_64.as_mut(),
            platform::Platform::WindowsAarch64 => self.windows_aarch64.as_mut(),
            platform::Platform::LinuxX86_64 => self.linux_x86_64.as_mut(),
            platform::Platform::LinuxAarch64 => self.linux_aarch64.as_mut(),
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
        context: std::sync::Arc<context::Context>,
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
                        .context(format_context!("{dest_path}"))?;
                }
                std::fs::hard_link(path.as_str(), dest_path.as_str())
                    .context(format_context!("{path} -> {dest_path}"))?;
            }
            AssetType::Template => {
                // remove the destination file if it exists
                if std::path::Path::new(dest_path.as_str()).exists() {
                    std::fs::remove_file(dest_path.as_str())
                        .context(format_context!("{dest_path}"))?;
                }

                let contents = context
                    .template_model
                    .render_template_path(&path)
                    .context(format_context!(""))?;

                // create a copy
                std::fs::write(dest_path.as_str(), contents)
                    .context(format_context!("{dest_path}"))?;
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
            .context(format_context!("Failed to read deps file {file_path}"));

        if let Ok(contents) = contents {
            let result: Self = toml::from_str(&contents)
                .context(format_context!("Failed to parse deps file {file_path}"))?;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    #[allow(dead_code)]
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let file_path = format!("{path}/{}", Self::FILE_NAME); //change to spaces_dependencies.toml
        let contents = toml::to_string(&self).context(format_context!(
            "failed to build toml string for {file_path}"
        ))?;
        std::fs::write(&file_path, contents)
            .context(format_context!("failed write file to {file_path}"))?;
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
        std::fs::write(&file_path, contents).context(format_context!(
            "Failed to write buckconfig file {file_path}"
        ))?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VsCodeExtensions {
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VsCodeConfig {
    tasks: Option<HashMap<String, toml::Value>>,
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
                .context(format_context!("failed to parse {path} as JSON"))?
        } else {
            default_value
        };
        Ok(result)
    }

    fn save_json_file(path: &str, value: serde_json::Value) -> anyhow::Result<()> {
        let contents = serde_json::to_string_pretty(&value)
            .context(format_context!("failed to serialize JSON to {path}"))?;
        std::fs::write(path, contents)
            .context(format_context!("failed to write JSON to {path}"))?;
        Ok(())
    }

    pub fn apply(&self, workspace_path: &str) -> anyhow::Result<()> {
        let vs_code_directory = format!("{workspace_path}/.vscode");
        std::fs::create_dir_all(vs_code_directory.as_str()).context(format_context!(
            "failed to create director {vs_code_directory}"
        ))?;

        if let Some(own_tasks) = self.tasks.as_ref() {
            let tasks_file = format!("{vs_code_directory}/tasks.json");
            let mut tasks = Self::load_json_file(
                tasks_file.as_str(),
                serde_json::json!({"version": "2.0.0", "tasks": []}),
            )
            .context(format_context!("while loading {tasks_file}"))?;

            let tasks_object = tasks.as_object_mut().ok_or(anyhow::anyhow!(
                "Failed to get settings from {tasks_file} JSON object"
            ))?;

            let tasks_array = tasks_object
                .get_mut("tasks")
                .and_then(|e| e.as_array_mut())
                .ok_or(anyhow::anyhow!(
                    "Failed to get tasks from {tasks_file} JSON object"
                ))?;

            for value in own_tasks.values() {
                let json_value = serde_json::to_value(value).context(format_context!(
                    "toml value {value:?} cannot be converted to JSON"
                ))?;

                tasks_array.push(json_value);
            }
            Self::save_json_file(tasks_file.as_str(), tasks)
                .context(format_context!("while saving {tasks_file}"))?;
        }

        if let Some(own_settings) = self.settings.as_ref() {
            let settings_file = format!("{vs_code_directory}/settings.json");
            let mut settings = Self::load_json_file(settings_file.as_str(), serde_json::json!({}))
                .context(format_context!("while loading {settings_file}"))?;

            let settings_object = settings.as_object_mut().ok_or(anyhow::anyhow!(
                "Failed to get settings from {settings_file} JSON object"
            ))?;

            for (key, value) in own_settings {
                let json_value = serde_json::to_value(value).context({
                    format_context!("toml value {value:?} cannot be converted to JSON")
                })?;

                settings_object.insert(key.clone(), json_value);
            }
            Self::save_json_file(settings_file.as_str(), settings)
                .context(format_context!("while saving {settings_file}"))?;
        }

        if let Some(own_extensions) = self.extensions.as_ref() {
            let extensions_file = format!("{vs_code_directory}/extensions.json");
            let mut extensions = Self::load_json_file(
                extensions_file.as_str(),
                serde_json::json!({"recommendations": []}),
            )
            .context(format_context!("while loading {extensions_file}"))?;

            let extensions_object = extensions.as_object_mut().ok_or(format_error!(
                "Failed to get extensions from {extensions_file} JSON object"
            ))?;
            if !extensions_object.contains_key("recommendations") {
                extensions_object.insert("recommendations".to_string(), serde_json::json!({}));
            }

            let recommendations_array = extensions_object
                .get_mut("recommendations")
                .and_then(|e| e.as_array_mut())
                .ok_or(format_error!(
                "Internl Erorr: Failed to get recommendations from {extensions_file} JSON object"
            ))?;

            for value in own_extensions.recommendations.iter() {
                let value = serde_json::Value::String(value.clone());
                if !recommendations_array.contains(&value) {
                    recommendations_array.push(value);
                }
            }

            Self::save_json_file(&extensions_file, extensions)
                .context(format_context!("while saving {extensions_file}"))?;
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
    pub actions: Option<HashMap<String, Vec<Action>>>,
}

impl WorkspaceConfig {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path).context(format_context!(
            "Failed to read spaces workspace config file {path}"
        ))?;
        let result: WorkspaceConfig = toml::from_str(&contents).context(format_context!(
            "Failed to parse spaces workspace config file {path}"
        ))?;
        Ok(result)
    }

    pub fn to_workspace(
        &self,
        context: std::sync::Arc<context::Context>
    ) -> anyhow::Result<Workspace> {
        let mut repositories = self.repositories.clone();
        for dependency in repositories.values_mut() {
            let dev_branch = if let Some(branch) = self
                .settings
                .as_ref()
                .and_then(|e| e.branch.as_ref())
                .as_ref()
            {
                (*branch).to_owned()
            } else {
                r#"user/{{ spaces.user }}/{{ spaces.space_name }}-{{ spaces.unique }}"#.to_string()
            };

            let dev_branch = context
                .template_model
                .render_template_string(&dev_branch)
                .context(format_context!("{dev_branch}"))?;

            dependency.branch = Some(dev_branch);
            dependency.checkout = CheckoutOption::NewBranch;
        }
        Ok(Workspace {
            repositories,
            buck: self.buck.clone(),
            cargo: self.cargo.clone(),
            dependencies: HashMap::new(),
            assets: None,
            vscode: self.vscode.clone(),
            actions: self.actions.clone(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Action {
    pub name: String,
    pub command: String,
    pub arguments: Option<Vec<String>>,
    pub environment: Option<HashMap<String, String>>,
    pub working_directory: Option<String>,
    pub display: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Workspace {
    pub repositories: HashMap<String, Dependency>,
    pub dependencies: HashMap<String, Dependency>,
    pub buck: Option<BuckConfig>,
    pub cargo: Option<CargoConfig>,
    pub assets: Option<HashMap<String, WorkspaceAsset>>,
    pub vscode: Option<VsCodeConfig>,
    pub actions: Option<HashMap<String, Vec<Action>>>,
}

impl Workspace {
    const FILE_NAME: &'static str = "spaces_workspace.toml";

    pub fn new(path: &str) -> anyhow::Result<Self> {
        let file_path = format!("{path}/{}", Self::FILE_NAME);
        let contents = std::fs::read_to_string(&file_path)
            .context(format_context!("Failed to read workspace file {file_path}"))?;
        let result: Workspace = toml::from_str(&contents)
            .context(format_context!("Failed to workspace file {file_path}"))?;
        Ok(result)
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let file_path = format!("{path}/{}", Self::FILE_NAME);
        let contents = toml::to_string(&self)
            .context(format_context!("Failed to serialize workspace {self:?}"))?;

        std::fs::write(&file_path, contents)
            .context(format_context!("Failed to save workspace file {file_path}"))?;
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
            .context(format_context!("Failed to read ledger file {path}"))?;
        let result: Ledger = toml::from_str(&contents)
            .context(format_context!("Failed to parse ledger file {path}"))?;
        Ok(result)
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let contents = toml::to_string(&self).context("Failed to serialize ledger")?;
        std::fs::write(path, contents)
            .context(format_context!("Failed to save ledger file {path}"))?;
        Ok(())
    }
}



#[cfg(test)]
mod test {

    use super::*;

    const UNIQUE: &str = "1234";

    fn get_execution_context() -> context::ExecutionContext {
        let mut execution_context = context::ExecutionContext::new().unwrap();
        execution_context.context.template_model.spaces.space_name = "spaces-dev".to_string();
        execution_context.context.template_model.spaces.sysroot = "test_data/spaces/spaces-dev/sysroot".to_string();
        execution_context.context.template_model.spaces.unique = UNIQUE.to_string();
        execution_context.context.template_model.spaces.user = "test".to_string();
        execution_context
    }

    fn test_to_workspace_path(path: &str){
        let workspace_config = WorkspaceConfig::new(path).unwrap();
        let execution_context = get_execution_context();
        let context = std::sync::Arc::new(execution_context.context);
        let workspace = workspace_config.to_workspace(context).unwrap();
        for (_space_name, dependency) in workspace.dependencies.iter() {
            let dev_branch = dependency.branch.as_ref().unwrap();
            assert_eq!(dev_branch, "spaces-dev-1234");
        }
    }

    #[test]
    fn test_to_workspace(){
        test_to_workspace_path("test_data/workflows/spaces_develop.toml");
        test_to_workspace_path("test_data/workflows/spaces_develop_legacy.toml");
    }
}