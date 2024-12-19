pub mod archive;
pub mod asset;
pub mod capsule;
pub mod env;
pub mod exec;
pub mod oras;
pub mod git;
pub mod http_archive;

use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

pub struct TaskResult {
    pub new_modules: Vec<String>,
    pub enabled_targets: Vec<String>,
}

impl TaskResult {
    pub fn new() -> Self {
        TaskResult {
            new_modules: Vec::new(),
            enabled_targets: Vec::new(),
        }
    }

    pub fn extend(&mut self, other: TaskResult) {
        self.new_modules.extend(other.new_modules);
        self.enabled_targets.extend(other.enabled_targets);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Task {
    Exec(exec::Exec),
    ExecIf(exec::ExecIf),
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
    Capsule(capsule::Capsule),
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
        let mut enabled_targets = Vec::new();
        match self {
            Task::HttpArchive(archive) => archive.execute(progress, workspace.clone(), name),
            Task::OrasArchive(archive) => archive.execute(progress, workspace.clone(), name),
            Task::Exec(exec) => exec.execute(&mut progress, workspace.clone(), name),
            Task::ExecIf(exec_if) => {
                enabled_targets = exec_if.execute(progress, workspace.clone(), name);
                Ok(())
            }
            Task::Kill(kill) => kill.execute(name, &mut progress),
            Task::CreateArchive(archive) => archive.execute(progress, workspace.clone(), name),
            Task::UpdateAsset(asset) => asset.execute(progress, workspace.clone(), name),
            Task::AddWhichAsset(asset) => asset.execute(progress, workspace.clone(), name),
            Task::AddHardLink(asset) => asset.execute(progress, workspace.clone(), name),
            Task::AddSoftLink(asset) => asset.execute(progress, workspace.clone(), name),
            Task::UpdateEnv(update_env) => update_env.execute(progress, workspace.clone(), name),
            Task::AddAsset(asset) => asset.execute(progress, workspace.clone(), name),
            Task::Capsule(capsule) => capsule.execute(&mut progress, workspace.clone(), name),
            Task::Git(git) => {
                check_new_modules = git.is_evaluate_spaces_modules;
                git.execute(&mut progress, workspace.clone(), name)
            }
            Task::Target => Ok(()),
        }
        .context(format_context!("Failed to execute task {}", name))?;

        let mut result = TaskResult {
            new_modules: Vec::new(),
            enabled_targets,
        };

        if check_new_modules {
            let workspace = workspace.read().absolute_path.to_owned();
            let parts = name.split(':').collect::<Vec<&str>>();
            if let Some(last) = parts.last() {
                if !last.starts_with(workspace::SPACES_CAPSULES_NAME) {
                    let workspace_path = std::path::Path::new(workspace.as_str());
                    let new_repo_path = workspace_path.join(last);
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
                                result.new_modules.push(relative_workspace_path);
                            }
                        }
                    }
                }
            }
        }
        Ok(result)
    }
}
