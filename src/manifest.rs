use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Deps {
    pub deps: HashMap<String, Dependency>,
    pub archives: Option<HashMap<String, Archive>>,
}

impl Deps {
    const FILE_NAME: &'static str = "spaces_deps.toml";

    pub fn new(path: &str) -> anyhow::Result<Option<Self>> {
        let file_path = format!("{path}/{}", Self::FILE_NAME); //change to spaces_dependencies.toml
        let contents = std::fs::read_to_string(&file_path)
            .with_context(|| format!("Failed to read deps file {file_path}"));

        if let Ok(contents) = contents {
            let result: Deps = toml::from_str(&contents)
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
pub struct WorkspaceConfig {
    pub repositories: HashMap<String, Dependency>,
    pub buck: Option<BuckConfig>,
    pub cargo: Option<CargoConfig>,
}

impl WorkspaceConfig {
    const FILE_NAME: &'static str = "spaces_workspace_config.toml";

    pub fn new(path: &str) -> anyhow::Result<Self> {
        let file_path = format!("{path}/{}", Self::FILE_NAME);
        let contents = std::fs::read_to_string(&file_path)
            .with_context(|| format!("Failed to read workspace config file {file_path}"))?;
        let result: WorkspaceConfig = toml::from_str(&contents)
            .with_context(|| format!("Failed to workspace config file {file_path}"))?;

        Ok(result)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Workspace {
    pub repositories: HashMap<String, Dependency>,
    pub dependencies: HashMap<String, Dependency>,
    pub buck: Option<BuckConfig>,
    pub cargo: Option<CargoConfig>,
}

impl Workspace {
    const FILE_NAME: &'static str = "spaces_workspace.toml";
    pub fn new_from_workspace_config(workspace_config: &WorkspaceConfig, space_name: &str) -> Self {
        let mut repositories = workspace_config.repositories.clone();
        for (_key, dependency) in repositories.iter_mut() {
            dependency.dev = Some(space_name.to_string());
            dependency.checkout = Some(CheckoutOption::Develop);
        }
        Workspace {
            repositories,
            buck: workspace_config.buck.clone(),
            cargo: workspace_config.cargo.clone(),
            dependencies: HashMap::new(),
        }
    }

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
}
