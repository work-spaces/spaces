use crate::workspace;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Git {
    pub url: String,
    pub spaces_key: String,
    pub worktree_path: String,
    pub checkout: git::Checkout,
    pub clone: git::Clone,
    pub is_evaluate_spaces_modules: bool,
}

impl Git {
    fn resolve_revision(
        &self,
        revision: &str,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<String> {
        let mut result = revision.to_string();
        let parts = revision.split(':').collect::<Vec<&str>>();
        if parts.len() == 2 {
            let branch = parts[0];
            let semver = parts[1];
            let logs = git::get_branch_log(&self.url, &self.spaces_key, branch, progress).context(
                format_context!("Failed to get branch log for {}", self.spaces_key),
            )?;

            let required = semver::VersionReq::parse(semver)
                .context(format_context!("Failed to parse semver {}", semver,))?;

            // logs has tags in reverse chronological order
            // uses the newest commit that does not violate the semver requirement
            let mut commit = None;
            let mut is_semver_satisfied = false;
            for log in logs {
                let current_commit = log.commit.clone();
                if let Some(tag) = log.tag.as_ref() {
                    let tag = tag.trim_matches('v');
                    if let Ok(version) = semver::Version::parse(tag) {
                        if required.matches(&version) {
                            progress.log(
                                printer::Level::Debug,
                                format!(
                                    "Found tag {} for branch {} that satisfies semver requirement",
                                    tag, branch
                                )
                                .as_str(),
                            );
                            is_semver_satisfied = true;
                        } else if is_semver_satisfied {
                            progress.log(printer::Level::Debug,
                            format!("Using commit {commit:?} for branch {branch} as it is the newest commit that satisfies semver requirement").as_str());
                            break;
                        }
                    } else {
                        commit = Some(current_commit);
                    }
                } else {
                    commit = Some(current_commit);
                }
            }

            if let Some(commit) = commit {
                result = commit.to_string();
            }
        } else if parts.len() != 1 {
            return Err(format_error!(
                "Invalid revision format. Use `<branch>:<semver requirement>`"
            ));
        }
        progress.log(
            printer::Level::Info,
            format!(
                "Resolved revision {} to {} for {}",
                revision, result, self.url
            )
            .as_str(),
        );
        Ok(result)
    }

    fn execute_worktree_clone(
        &self,
        name: &str,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let bare_repo = git::BareRepository::new(
            progress,
            workspace::get_store_path().as_str(),
            &self.spaces_key,
            &self.url,
        )
        .context(format_context!("Failed to create bare repository"))?;

        let worktree = bare_repo
            .add_worktree(progress, &self.worktree_path)
            .context(format_context!("{name} - Failed to add worktree"))?;

        match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                let revision = self
                    .resolve_revision(branch_name, progress)
                    .context(format_context!("failed to resolve revision"))?;

                worktree
                    .switch_new_branch(progress, branch_name, &revision)
                    .context(format_context!("{name} - Failed to checkout new branch"))?;
            }
            git::Checkout::Revision(revision) => {
                let revision = self
                    .resolve_revision(revision, progress)
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
        name: &str,
        filter: Option<String>,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let mut clone_arguments = vec!["clone".to_string()];
        if let Some(filter) = filter {
            clone_arguments.push(format!("--filter={}", filter));
        }

        clone_arguments.push(self.url.clone());
        clone_arguments.push(self.spaces_key.clone());

        let clone_options = printer::ExecuteOptions {
            arguments: clone_arguments,
            ..Default::default()
        };

        let clone_path = std::path::Path::new(&self.spaces_key);
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

        let mut checkout_options = printer::ExecuteOptions {
            working_directory: Some(self.spaces_key.clone()),
            ..Default::default()
        };

        match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                checkout_options.arguments.push("switch".to_string());
                checkout_options.arguments.push("-c".to_string());
                checkout_options.arguments.push(branch_name.clone());
                // TODO: switch to a new branch
            }
            git::Checkout::Revision(revision) => {
                // if revision of the format "branch:semver" then get the tags on the branch
                let revision = self
                    .resolve_revision(revision, progress)
                    .context(format_context!("failed to resolve revision"))?;

                checkout_options.arguments.push("checkout".to_string());
                checkout_options.arguments.push(revision.clone());
            }
        }

        progress.log(
            printer::Level::Trace,
            format!("git clone {checkout_options:?}").as_str(),
        );

        progress
            .execute_process("git", checkout_options)
            .context(format_context!(
                "{name} - Failed to clone repository {}",
                self.spaces_key
            ))?;

        Ok(())
    }

    fn execute_shallow_clone(
        &self,
        name: &str,
        progress: &mut printer::MultiProgressBar,
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
                "clone".to_string(),
                "--depth".to_string(),
                "1".to_string(),
                self.url.clone(),
                self.spaces_key.clone(),
                "--branch".to_string(),
                branch.clone(),
                "--single-branch".to_string(),
            ],
            ..Default::default()
        };

        let clone_path = std::path::Path::new(&self.spaces_key);
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
        name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        match self.clone {
            git::Clone::Worktree => self
                .execute_worktree_clone(name, &mut progress)
                .context(format_context!("spaces clone failed"))?,
            git::Clone::Default => self
                .execute_default_clone(name, None, &mut progress)
                .context(format_context!("default clone failed"))?,
            git::Clone::Blobless => self
                .execute_default_clone(name, Some("blob:none".to_string()), &mut progress)
                .context(format_context!("default clone failed"))?,
            git::Clone::Shallow => self
                .execute_shallow_clone(name, &mut progress)
                .context(format_context!("default clone failed"))?,
        }

        let ref_name = match &self.checkout {
            git::Checkout::NewBranch(branch_name) => branch_name.clone(),
            git::Checkout::Revision(branch_name) => branch_name.clone(),
        };

        let mut is_locked = false;
        if workspace::is_create_lock_file() {
            if let Some(commit_hash) =
                git::get_commit_hash(&self.url, &self.spaces_key, &mut progress).context(
                    format_context!("Failed to get commit hash for {}", self.spaces_key),
                )?
            {
                let rev = if let Some(tag) =
                    git::get_commit_tag(&self.url, &self.spaces_key, &mut progress)
                {
                    tag
                } else {
                    commit_hash.to_string()
                };
                // strip the trailing newline
                workspace::add_git_commit_lock(name, rev);
            }
        } else if let Some(commit_hash) = workspace::get_git_commit_lock(name) {
            let options = printer::ExecuteOptions {
                working_directory: Some(self.spaces_key.clone()),
                arguments: vec!["checkout".to_string(), "--detach".to_string(), commit_hash],
                ..Default::default()
            };

            progress.log(
                printer::Level::Debug,
                format!("{}: git {options:?}", self.spaces_key).as_str(),
            );

            git::execute_git_command(&self.url, &mut progress, options).context(
                format_context!("Failed to checkout commit hash from {}", self.spaces_key),
            )?;

            is_locked = true;
        }

        // after possibly applying the lock commit, check for reproducibility
        if !is_locked {
            // check if checkout is on a branch or commiy
            let is_branch = git::is_branch(&self.url, &self.spaces_key, &ref_name, &mut progress);
            if is_branch {
                progress.log(
                    printer::Level::Info,
                    format!(
                        "{} is a branch - workspace is not reproducible",
                        self.spaces_key
                    )
                    .as_str(),
                );
                workspace::set_is_reproducible(false);
            }
        }

        Ok(())
    }
}
