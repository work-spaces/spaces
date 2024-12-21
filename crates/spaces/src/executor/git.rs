use crate::workspace;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Git {
    pub url: Arc<str>,
    pub spaces_key: Arc<str>,
    pub worktree_path: Arc<str>,
    pub checkout: git::Checkout,
    pub clone: git::Clone,
    pub is_evaluate_spaces_modules: bool,
    pub sparse_checkout: Option<git::SparseCheckout>,
}

impl Git {
    fn execute_worktree_clone(
        &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let (relative_bare_store_path, name_dot_git) =
            git::BareRepository::url_to_relative_path_and_name(&self.url)
                .context(format_context!("Failed to parse {name} url: {}", self.url))?;
        let store_path = workspace.read().get_store_path();
        let lock_file_path =
            format!("{store_path}/{relative_bare_store_path}/{name_dot_git}.spaces.lock");
        let mut lock_file = lock::FileLock::new(lock_file_path.into());

        lock_file.lock(progress).context(format_context!(
            "{name} - Failed to lock the repository {}",
            self.spaces_key
        ))?;

        let bare_repo =
            git::BareRepository::new(progress, store_path.as_ref(), &self.spaces_key, &self.url)
                .context(format_context!("Failed to create bare repository"))?;

        let worktree = bare_repo
            .add_worktree(progress, &self.worktree_path)
            .context(format_context!("{name} - Failed to add worktree"))?;

        match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                let repository = worktree.to_repository();
                let revision = repository
                    .resolve_revision(progress, branch_name)
                    .context(format_context!("failed to resolve revision"))?;

                worktree
                    .switch_new_branch(progress, branch_name, &revision)
                    .context(format_context!("{name} - Failed to checkout new branch"))?;
            }
            git::Checkout::Revision(revision) => {
                let repository = worktree.to_repository();
                let revision = repository
                    .resolve_revision(progress, revision)
                    .context(format_context!("failed to resolve revision"))?;

                worktree
                    .checkout(progress, &revision)
                    .context(format_context!("{name} - Failed to switch branch"))?;
            }
        }

        Ok(())
    }

    fn execute_default_clone(
        &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
        filter: Option<String>,
    ) -> anyhow::Result<()> {
        let mut clone_arguments: Vec<Arc<str>> = vec!["clone".into()];
        if let Some(filter) = filter {
            clone_arguments.push(format!("--filter={}", filter).into());
        }

        if self.sparse_checkout.is_some() {
            clone_arguments.push("--no-checkout".into());
        }

        clone_arguments.push(self.url.clone());
        clone_arguments.push(self.spaces_key.clone());

        let workspace_directory = workspace.read().get_absolute_path();
        let repo_directory: Arc<str> =
            format!("{}/{}", workspace_directory, self.spaces_key).into();

        let repository = git::Repository::new_clone(
            progress,
            self.url.clone(),
            workspace_directory.clone(),
            self.spaces_key.clone(),
            clone_arguments,
        ).context(format_context!(
            "{name} - Failed to clone repository {}",
            self.spaces_key
        ))?;

        if let Some(sparse_checkout) = self.sparse_checkout.as_ref() {
            repository.setup_sparse_checkout(
                    progress,sparse_checkout)
                .context(format_context!(
                    "Failed to init sparse checkout in {repo_directory}"
                ))?;
        }

        repository
            .checkout(progress, &self.checkout)
            .context(format_context!(
                "{name} - Failed to checkout repository {}",
                self.spaces_key
            ))?;

        Ok(())
    }

    fn execute_shallow_clone(
    &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let branch = match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                return Err(format_error!(
                    "Cannot create a new branch {branch_name} with a shallow clone"
                ));
            }
            git::Checkout::Revision(branch_name) => branch_name.clone(),
        };

        let clone_options = printer::ExecuteOptions {
            arguments: vec![
                "clone".into(),
                "--depth".into(),
                "1".into(),
                self.url.clone(),
                self.spaces_key.clone(),
                "--branch".into(),
                branch.clone(),
                "--single-branch".into(),
            ],
            working_directory: Some(workspace.read().get_absolute_path()),
            ..Default::default()
        };

        let clone_path = std::path::Path::new(self.spaces_key.as_ref());
        if clone_path.exists() {
            progress.log(
                printer::Level::Info,
                format!("{} already exists", self.spaces_key).as_str(),
            );
        } else {
            progress.log(
                printer::Level::Trace,
                format!("git clone {clone_options:?}").as_str(),
            );

            progress
                .execute_process("git", clone_options)
                .context(format_context!(
                    "{name} - Failed to clone repository {}",
                    self.spaces_key
                ))?;
        }

        Ok(())
    }

    pub fn execute(
        &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        match self.clone {
            git::Clone::Worktree => self
                .execute_worktree_clone(progress, workspace.clone(), name)
                .context(format_context!("spaces clone failed"))?,
            git::Clone::Default => self
                .execute_default_clone(progress, workspace.clone(), name, None)
                .context(format_context!("default clone failed"))?,
            git::Clone::Blobless => self
                .execute_default_clone(
                    progress,
                    workspace.clone(),
                    name,
                    Some("blob:none".to_string()),
                )
                .context(format_context!("default clone failed"))?,
            git::Clone::Shallow => self
                .execute_shallow_clone(progress, workspace.clone(), name)
                .context(format_context!("default clone failed"))?,
        }

        let ref_name = match &self.checkout {
            git::Checkout::NewBranch(branch_name) => branch_name.clone(),
            git::Checkout::Revision(branch_name) => branch_name.clone(),
        };

        let mut is_locked = false;
        if workspace.read().is_create_lock_file {
            if let Some(commit_hash) =
                git::get_commit_hash(&self.url, &self.spaces_key, progress).context(
                    format_context!("Failed to get commit hash for {}", self.spaces_key),
                )?
            {
                let rev: Arc<str> =
                    if let Some(tag) = git::get_commit_tag(&self.url, &self.spaces_key, progress) {
                        tag
                    } else {
                        commit_hash
                    };
                // strip the trailing newline
                workspace.write().add_git_commit_lock(name, rev);
            }
        } else if let Some(commit_hash) = workspace.read().locks.get(name) {
            let options = printer::ExecuteOptions {
                working_directory: Some(
                    workspace
                        .read()
                        .get_relative_directory(self.spaces_key.as_ref()),
                ),
                arguments: vec!["checkout".into(), "--detach".into(), commit_hash.clone()],
                ..Default::default()
            };

            progress.log(
                printer::Level::Debug,
                format!("{}: git {options:?}", self.spaces_key).as_str(),
            );

            git::execute_git_command(&self.url, progress, options).context(format_context!(
                "Failed to checkout commit hash from {}",
                self.spaces_key
            ))?;

            is_locked = true;
        }

        // after possibly applying the lock commit, check for reproducibility
        if !is_locked {
            // check if checkout is on a branch or commiy
            let is_branch = git::is_branch(&self.url, &self.spaces_key, &ref_name, progress);
            if is_branch {
                progress.log(
                    printer::Level::Info,
                    format!(
                        "{} is a branch - workspace is not reproducible",
                        self.spaces_key
                    )
                    .as_str(),
                );
                workspace.write().set_is_reproducible(false);
            }
        }

        Ok(())
    }
}
