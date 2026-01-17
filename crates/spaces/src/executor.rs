pub mod archive;
pub mod asset;
pub mod env;
pub mod exec;
pub mod git;
pub mod http_archive;
pub mod oras;

use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub struct TaskResult {
    pub new_modules: Vec<Arc<str>>,
}

impl TaskResult {
    pub fn new() -> Self {
        TaskResult {
            new_modules: Vec::new(),
        }
    }

    pub fn extend(&mut self, other: TaskResult) {
        self.new_modules.extend(other.new_modules);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Task {
    Exec(exec::Exec),
    Kill(exec::Kill),
    Target,
    CreateArchive(archive::Archive),
    HttpArchive(http_archive::HttpArchive),
    OrasArchive(oras::OrasArchive),
    AddWhichAsset(asset::AddWhichAsset),
    AddHardLink(asset::AddHardLink),
    AddSoftLink(asset::AddSoftLink),
    UpdateAsset(asset::UpdateAsset),
    UpdateEnv(env::UpdateEnv),
    AddAsset(asset::AddAsset),
    AddAnyAssets(asset::AddAnyAssets),
    Git(git::Git),
}

impl Task {
    pub fn execute(
        &self,
        mut progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<TaskResult> {
        let mut check_new_modules = false;
        match self {
            Task::HttpArchive(archive) => archive.execute(progress, workspace.clone(), name),
            Task::OrasArchive(archive) => archive.execute(progress, workspace.clone(), name),
            Task::Exec(exec) => exec.execute(&mut progress, workspace.clone(), name),
            Task::Kill(kill) => kill.execute(name, &mut progress),
            Task::CreateArchive(archive) => archive.execute(progress, workspace.clone(), name),
            Task::UpdateAsset(asset) => asset.execute(progress, workspace.clone(), name),
            Task::AddWhichAsset(asset) => asset.execute(&mut progress, workspace.clone(), name),
            Task::AddHardLink(asset) => asset.execute(&mut progress, workspace.clone(), name),
            Task::AddSoftLink(asset) => asset.execute(&mut progress, workspace.clone(), name),
            Task::UpdateEnv(update_env) => update_env.execute(progress, workspace.clone(), name),
            Task::AddAsset(asset) => asset.execute(&mut progress, workspace.clone(), name),
            Task::AddAnyAssets(any_assets) => {
                any_assets.execute(&mut progress, workspace.clone(), name)
            }
            Task::Git(git) => {
                check_new_modules =
                    git.is_evaluate_spaces_modules && git.working_directory.is_none();
                git.execute(&mut progress, workspace.clone(), name)
            }
            Task::Target => Ok(()),
        }
        .context(format_context!("Failed to execute task {}", name))?;

        let mut result = TaskResult {
            new_modules: Vec::new(),
        };

        if check_new_modules {
            let workspace = workspace.read().absolute_path.to_owned();
            let parts = name.split(':').collect::<Vec<&str>>();
            if let Some(last) = parts.last() {
                let workspace_path = std::path::Path::new(workspace.as_ref());
                let new_repo_path = workspace_path.join(last);
                let workflows_file_path = new_repo_path.join(workspace::WORKFLOW_TOML_NAME);

                // if the repo is a workflows repo, don't add the modules
                if !workflows_file_path.exists() {
                    // add files in the directory that end in spaces.star
                    let modules = std::fs::read_dir(new_repo_path.clone()).context(
                        format_context!("Failed to read workspace directory {new_repo_path:?}"),
                    )?;

                    for module in modules.flatten() {
                        let path = module.path();
                        if path.is_file() {
                            let path = path.to_string_lossy().to_string();
                            if workspace::is_rules_module(path.as_str()) {
                                let relative_workspace_path =
                                    format!("{}/{}", last, module.file_name().to_string_lossy());
                                result.new_modules.push(relative_workspace_path.into());
                            }
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    fn target_to_markdown() -> String {
        let mut result = String::new();
        use printer::markdown;
        result.push_str(&markdown::paragraph(
            "This target executes its dependencies.",
        ));
        result.push('\n');
        result
    }

    fn details_to_json_markdown<Input: serde::Serialize>(input: Input) -> String {
        use printer::markdown;
        let mut result = String::new();
        let code_block = markdown::code_block(
            "json",
            serde_json::to_string_pretty(&input)
                .unwrap_or_default()
                .as_str(),
        );
        result.push_str(code_block.as_str());
        result.push('\n');
        result
    }

    pub fn to_markdown(&self) -> Option<String> {
        match self {
            Task::Git(task) => Some(Self::details_to_json_markdown(task)),
            Task::HttpArchive(task) => {
                Some(Self::details_to_json_markdown(&task.http_archive.archive))
            }
            Task::OrasArchive(task) => Some(Self::details_to_json_markdown(task)),
            Task::AddWhichAsset(task) => Some(Self::details_to_json_markdown(task)),
            Task::AddHardLink(task) => Some(Self::details_to_json_markdown(task)),
            Task::AddSoftLink(task) => Some(Self::details_to_json_markdown(task)),
            Task::UpdateAsset(task) => Some(Self::details_to_json_markdown(task)),
            Task::UpdateEnv(task) => Some(Self::details_to_json_markdown(task)),
            Task::Exec(task) => Some(task.to_markdown()),
            Task::Kill(task) => Some(task.to_markdown()),
            Task::Target => Some(Self::target_to_markdown()),
            _ => None,
        }
    }
}
