use crate::{singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn logger(progress: &mut printer::MultiProgressBar, url: Arc<str>) -> logger::Logger<'_> {
    logger::Logger::new_progress(progress, url)
}

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
    pub working_directory: Option<Arc<str>>,
}

impl Git {
    fn rev_to_version(rev: Option<Arc<str>>) -> Option<Arc<str>> {
        if let Some(rev) = rev {
            // many projects use name-v1.2.3 as the tag
            let split_rev = rev.as_ref().split('-').next_back().unwrap_or(rev.as_ref());
            // if rev is semver parse-able, set the version
            let stripped_rev = split_rev.strip_prefix("v").unwrap_or(split_rev.as_ref());
            let version: Option<Arc<str>> = semver::Version::parse(stripped_rev)
                .ok()
                .map(|version| version.to_string().into());
            version
        } else {
            None
        }
    }

    pub fn get_member(&self) -> anyhow::Result<ws::Member> {
        let rev = match &self.checkout {
            git::Checkout::NewBranch(branch_name) => branch_name.clone(),
            git::Checkout::Revision(revision) => revision.clone(),
        };

        let version = Self::rev_to_version(Some(rev.clone()));

        Ok(ws::Member {
            path: self.spaces_key.clone(),
            url: self.url.clone(),
            rev,
            version,
        })
    }

    fn get_clone_working_directory(&self, workspace: workspace::WorkspaceArc) -> Arc<str> {
        if let Some(directory) = self.working_directory.as_ref() {
            directory.clone()
        } else {
            workspace.read().get_absolute_path()
        }
    }

    fn get_working_directory_in_repo(&self, workspace: workspace::WorkspaceArc) -> Arc<str> {
        if let Some(directory) = self.working_directory.as_ref() {
            format!("{directory}/{}", self.spaces_key).into()
        } else {
            format!(
                "{}/{}",
                self.get_clone_working_directory(workspace),
                self.spaces_key
            )
            .into()
        }
    }

    fn execute_worktree_clone(
        &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        logger(progress, self.url.clone()).debug("execute worktree clone");

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
                    .switch_new_branch(progress, branch_name, &revision.commit)
                    .context(format_context!("{name} - Failed to checkout new branch"))?;
            }
            git::Checkout::Revision(revision) => {
                let repository = worktree.to_repository();
                let revision = repository
                    .resolve_revision(progress, revision)
                    .context(format_context!("failed to resolve revision"))?;

                worktree
                    .checkout(progress, &revision.commit)
                    .context(format_context!("{name} - Failed to switch branch"))?;
            }
        };

        Ok(())
    }

    fn execute_default_clone(
        &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
        filter: Option<String>,
    ) -> anyhow::Result<()> {
        logger(progress, self.url.clone())
            .debug(format!("execute clone to store with filter {filter:?}").as_str());

        let spaces_name_path = std::path::Path::new(self.spaces_key.as_ref());
        if std::path::Path::new(spaces_name_path).exists() {
            // check if the directory is empty
            let entries = std::fs::read_dir(spaces_name_path).context(format_context!(
                "Internal Error: failed to read directory {}",
                self.spaces_key
            ))?;

            if entries.count() > 0 {
                let existing_repo = git::Repository::new(self.url.clone(), self.spaces_key.clone());

                if existing_repo.is_dirty(progress) {
                    logger(progress, self.url.clone()).warning(
                        format!(
                            "{} already exists and is dirty - not updating",
                            self.spaces_key
                        )
                        .as_str(),
                    );
                    return Ok(());
                }

                if existing_repo.is_head_branch(progress) {
                    // only pull if the remote branch is being tracked

                    if existing_repo.is_remote_branch_tracked(progress) {
                        existing_repo.pull(progress).context(format_context!(
                            "{name} - Failed to pull repository {}",
                            self.spaces_key
                        ))?;
                    } else {
                        logger(progress, self.url.clone())
                            .warning("Remote not tracked - not updating");
                    }
                    return Ok(());
                }

                // fetch to ensure any new tags are made available
                existing_repo.fetch(progress).context(format_context!(
                    "{name} - Failed to fetch repository {}",
                    self.spaces_key
                ))?;

                existing_repo
                    .checkout(progress, &self.checkout)
                    .context(format_context!(
                        "{name} - Failed to checkout repository {}",
                        self.spaces_key
                    ))?;

                return Ok(());
            }
        }

        let suffix: Arc<str> = if let Some(sparse_checkout) = self.sparse_checkout.as_ref() {
            let mut sparse_string = sparse_checkout.mode.to_string();
            for item in sparse_checkout.list.iter() {
                sparse_string.push_str(item);
            }
            // do a blake3 hash of sparse_string for the suffix
            let hash = blake3::hash(sparse_string.as_bytes());
            hash.to_string().into()
        } else {
            "".into()
        };

        let (relative_bare_store_path, name_dot_git) =
            git::BareRepository::url_to_relative_path_and_name(&self.url)
                .context(format_context!("Failed to parse {name} url: {}", self.url))?;
        let store_path = workspace.read().get_store_path();
        let store_repo_name: Arc<str> = format!("{name_dot_git}{suffix}").into();
        let working_directory: Arc<str> =
            format!("{store_path}/cow/{relative_bare_store_path}").into();

        logger(progress, self.url.clone())
            .debug(format!("cow copy in store at {working_directory}").as_str());

        std::fs::create_dir_all(working_directory.as_ref()).context(format_context!(
            "{name} - Failed to create working directory {}",
            working_directory
        ))?;

        let lock_file_path = format!("{working_directory}/{store_repo_name}.spaces.lock");
        let mut lock_file = lock::FileLock::new(lock_file_path.into());

        lock_file.lock(progress).context(format_context!(
            "{name} - Failed to lock the repository {}",
            self.spaces_key
        ))?;

        let repo_path: Arc<str> = format!("{working_directory}/{store_repo_name}").into();
        let store_repository = if !std::path::Path::new(repo_path.as_ref()).exists() {
            let mut clone_arguments: Vec<Arc<str>> = vec!["clone".into()];
            if let Some(filter) = filter {
                clone_arguments.push(format!("--filter={filter}").into());
            }

            if self.sparse_checkout.is_some() {
                clone_arguments.push("--no-checkout".into());
            }

            clone_arguments.push(self.url.clone());
            clone_arguments.push(store_repo_name.clone());

            let repository = git::Repository::new_clone(
                progress,
                self.url.clone(),
                working_directory.clone(),
                store_repo_name.clone(),
                clone_arguments,
            )
            .context(format_context!(
                "{name} - Failed to clone repository {working_directory}/{store_repo_name}",
            ))?;

            if let Some(sparse_checkout) = self.sparse_checkout.as_ref() {
                repository
                    .setup_sparse_checkout(progress, sparse_checkout)
                    .context(format_context!(
                        "Failed to init sparse checkout in {repo_path}"
                    ))?;
            }

            repository
        } else {
            let repository = git::Repository::new(self.url.clone(), repo_path.clone());
            repository.fetch(progress).context(format_context!(
                "{name} - Failed to fetch repository {working_directory}/{store_repo_name}",
            ))?;
            repository
        };

        store_repository
            .checkout(progress, &self.checkout)
            .context(format_context!(
                "{name} - Failed to checkout repository {}",
                self.spaces_key
            ))?;

        if store_repository.is_head_branch(progress) {
            store_repository.pull(progress).context(format_context!(
                "{name} - Failed to pull repository {}",
                self.spaces_key
            ))?;
        }

        // This is a local clone from the store repository to the workspace repository
        let clone_arguments: Vec<Arc<str>> = vec![
            "clone".into(),
            store_repository.full_path.clone(),
            self.spaces_key.clone(),
        ];

        git::Repository::new_clone(
            progress,
            self.url.clone(),
            ".".into(),
            self.spaces_key.clone(),
            clone_arguments,
        )
        .context(format_context!(
            "{name} - Failed to clone repository local repo {} to {store_repo_name}",
            store_repository.full_path
        ))?;

        Ok(())
    }

    fn execute_shallow_clone(
        &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        logger(progress, self.url.clone()).debug("execute shallow clone");

        let branch = match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                return Err(format_error!(
                    "Cannot create a new branch {branch_name} with a shallow clone"
                ));
            }
            git::Checkout::Revision(branch_name) => branch_name.clone(),
        };

        let workspace_directory = self.get_clone_working_directory(workspace.clone());

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
            working_directory: Some(workspace_directory),
            ..Default::default()
        };

        let clone_path = std::path::Path::new(self.spaces_key.as_ref());
        if clone_path.exists() {
            logger(progress, self.url.clone())
                .warning(format!("{} already exists", self.spaces_key).as_str());
        } else {
            logger(progress, self.url.clone())
                .trace(format!("git clone {clone_options:?}").as_str());

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
        // The logic in Repo::is_cow_semantics() needs to stay in sync
        // with the logic here. Default and Blobless use cow semantics.

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
        logger(progress, self.url.clone()).debug(format!("using ref {ref_name}").as_str());

        let mut is_locked = false;
        let working_directory = self.get_working_directory_in_repo(workspace.clone());

        let mut member = match self.get_member() {
            Ok(mut member) => {
                let latest_tag =
                    git::get_latest_tag(progress, &self.url, &self.spaces_key).context(
                        format_context!("Failed to get latest tag for {}", self.spaces_key),
                    )?;
                member.version = Self::rev_to_version(latest_tag.clone());
                Some(member)
            }
            Err(_) => {
                logger(progress, self.url.clone())
                    .warning(format!("Failed to member to settings for: {}", self.url).as_str());
                None
            }
        };

        if workspace.read().is_create_lock_file {
            logger(progress, self.url.clone()).debug("creating lock file");
            if let Some(commit_hash) =
                git::get_commit_hash(progress, &self.url, working_directory.as_ref()).context(
                    format_context!("Failed to get commit hash for {working_directory}"),
                )?
            {
                let rev: Arc<str> =
                    if let Some(tag) = git::get_commit_tag(progress, &self.url, &self.spaces_key) {
                        tag
                    } else {
                        commit_hash
                    };
                // strip the trailing newline
                workspace.write().add_git_commit_lock(name, rev);
            }
        } else if let Some(commit_hash) = workspace.read().locks.get(name) {
            logger(progress, self.url.clone())
                .info(format!("applying {commit_hash} from lock file at {name}").as_str());

            let options = printer::ExecuteOptions {
                working_directory: Some(working_directory.clone()),
                arguments: vec!["checkout".into(), "--detach".into(), commit_hash.clone()],
                ..Default::default()
            };

            if let Some(member) = member.as_mut() {
                member.rev = commit_hash.clone();
                member.version = Self::rev_to_version(Some(commit_hash.clone()));
            }

            logger(progress, self.url.clone())
                .debug(format!("{}: git {options:?}", self.spaces_key).as_str());

            git::execute_git_command(progress, &self.url, options).context(format_context!(
                "Failed to checkout commit hash from {}",
                self.spaces_key
            ))?;

            is_locked = true;
        }

        if singleton::get_new_branches().contains(&name.into()) {
            logger(progress, self.url.clone()).message("creating new branch");
            let new_branch = workspace.read().get_new_branch_name();
            let options = printer::ExecuteOptions {
                working_directory: Some(working_directory.clone()),
                arguments: vec!["switch".into(), "-c".into(), new_branch],
                ..Default::default()
            };

            logger(progress, self.url.clone())
                .debug(format!("{}: git {options:?}", self.spaces_key).as_str());

            git::execute_git_command(progress, &self.url, options).context(format_context!(
                "Failed to create new branch for {}",
                self.spaces_key
            ))?;

            workspace.write().set_is_reproducible(false);
        }

        // after possibly applying the lock commit, check for reproducibility
        if !is_locked {
            // check if checkout is on a branch or commit
            let is_branch =
                git::is_branch(progress, &self.url, working_directory.as_ref(), &ref_name);
            if is_branch {
                logger(progress, self.url.clone()).info(
                    format!(
                        "{} is a branch - workspace is not reproducible",
                        self.spaces_key
                    )
                    .as_str(),
                );
                workspace.write().set_is_reproducible(false);

                // try to pull the latest version from the branch
            }
        }

        if let Some(member) = member {
            logger(progress, self.url.clone()).debug("Adding member to workspace");
            workspace.write().add_member(member);
        }

        Ok(())
    }
}
