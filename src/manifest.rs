use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::format_error_context;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ArchiveLink {
    Soft,
    Hard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub url: String,
    pub sha256: String,
    pub link: ArchiveLink,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformArchive {
    pub archive: Archive,
    pub executables: Vec<String>,
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
pub struct Executables {
    pub macos_x86_64: Option<PlatformArchive>,
    pub macos_aarch64: Option<PlatformArchive>,
    pub windows_aarch64: Option<PlatformArchive>,
    pub windows_x86_64: Option<PlatformArchive>,
    pub linux_x86_64: Option<PlatformArchive>,
    pub linux_aarch64: Option<PlatformArchive>,
}

impl Executables {
    const FILE_NAME: &'static str = "spaces_executables.toml";

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
        let file_path = format!("{path}/{}", Self::FILE_NAME);
        let contents = toml::to_string(&self).with_context(|| {
            format_error_context!("failed to build toml string for {file_path}")
        })?;
        std::fs::write(&file_path, contents)
            .with_context(|| format_error_context!("failed write file to {file_path}"))?;
        Ok(())
    }

    pub fn get_platform_archive(&self) -> Option<PlatformArchive> {
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

    pub fn get_platform_archive_from_platform(
        &self,
        platform: Platform,
    ) -> Option<PlatformArchive> {
        match platform {
            Platform::MacosX86_64 => self.macos_x86_64.clone(),
            Platform::MacosAarch64 => self.macos_aarch64.clone(),
            Platform::WindowsX86_64 => self.windows_x86_64.clone(),
            Platform::WindowsAarch64 => self.windows_aarch64.clone(),
            Platform::LinuxX86_64 => self.linux_x86_64.clone(),
            Platform::LinuxAarch64 => self.linux_aarch64.clone(),
        }
    }

    pub fn get_platform_archive_from_platform_mut(
        &mut self,
        platform: Platform,
    ) -> Option<&mut PlatformArchive> {
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

#[derive(Debug, Serialize, Deserialize)]
pub enum AssetType {
    HardLink,
    Template,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceAsset {
    pub path: String,
    #[serde(rename = "type")]
    pub type_: AssetType,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Deps {
    pub deps: HashMap<String, Dependency>,
    pub archives: Option<HashMap<String, Archive>>,
    pub assets: Option<HashMap<String, WorkspaceAsset>>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct VsCodeTask {
    #[serde(rename = "type")]
    pub type_: String,
    pub command: String,
    #[serde(rename = "problemMatcher")]
    pub problem_matcher: Vec<String>,
    pub arguments: Vec<String>,
    pub label: String,
    pub group: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VsCodeConfig {
    tasks: Vec<VsCodeTask>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceConfigSettings {
    pub branch: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
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
                "user/{USER}/{SPACE}-{UNIQUE}".to_string()
            };

            const USER: &str = "{USER}";
            if dev_branch.contains("{USER}") {
                let user = std::env::var("USER").with_context(|| {
                    format!("Failed to replace {USER} with $USER for {dev_branch} naming")
                })?;
                dev_branch = dev_branch.replace(USER, &user);
            }

            const SPACE: &str = "{SPACE}";
            if dev_branch.contains(SPACE) {
                dev_branch = dev_branch.replace(SPACE, space_name);
            } else {
                return Err(anyhow::anyhow!("Branch name {dev_branch} must contain {SPACE}"));
            }

            const UNIQUE: &str = "{UNIQUE}";
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
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Workspace {
    pub repositories: HashMap<String, Dependency>,
    pub dependencies: HashMap<String, Dependency>,
    pub buck: Option<BuckConfig>,
    pub cargo: Option<CargoConfig>,
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
