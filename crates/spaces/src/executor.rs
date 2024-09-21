pub mod archive;
pub mod asset;
pub mod env;
pub mod exec;
pub mod git;
pub mod http_archive;

use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Task {
    Exec(exec::Exec),
    Target,
    CreateArchive(archive::Archive),
    HttpArchive(http_archive::HttpArchive),
    UpdateAsset(asset::UpdateAsset),
    UpdateEnv(env::UpdateEnv),
    AddAsset(asset::AddAsset),
    Git(git::Git),
}

impl Task {
    pub fn execute(
        &self,
        name: &str,
        progress: printer::MultiProgressBar,
    ) -> anyhow::Result<Vec<String>> {
        let mut check_new_modules = false;
        match self {
            Task::HttpArchive(archive) => archive.execute(name, progress),
            Task::Exec(exec) => exec.execute(name, progress),
            Task::CreateArchive(archive) => archive.execute(name, progress),
            Task::UpdateAsset(asset) => asset.execute(name, progress),
            Task::UpdateEnv(update_env) => update_env.execute(name, progress),
            Task::AddAsset(asset) => asset.execute(name, progress),
            Task::Git(git) => {
                check_new_modules = true;
                git.execute(name, progress)
            }
            Task::Target => Ok(()),
        }
        .context(format_context!("Failed to execute task {}", name))?;

        let mut new_modules = Vec::new();

        if check_new_modules {
            let workspace = workspace::get_workspace_path()
                .context(format_context!("No workspace directory found"))?;

            let parts = name.split(':').collect::<Vec<&str>>();

            if let Some(last) = parts.last() {
                let workspace_path = std::path::Path::new(workspace.as_str());
                let spaces_star_path = workspace_path
                    .join(*last)
                    .join(workspace::SPACES_MODULE_NAME);
                if spaces_star_path.exists() {
                    new_modules.push(spaces_star_path.to_string_lossy().to_string());
                }
            }
        }
        Ok(new_modules)
    }
}
