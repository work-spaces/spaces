use crate::{label, singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utils::{git, lock, logger, ws};

fn logger(console: console::Console, url: Arc<str>) -> logger::Logger {
    logger::Logger::new(console, url)
}

#[derive(Clone, Copy, PartialEq)]
enum IsNewBranch {
    No,
    Yes,
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
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        logger::push_deprecation_warning(None, "Support for worktrees will be removed in v0.16");
        logger(progress.console.clone(), self.url.clone()).message("execute worktree clone");

        let (relative_bare_store_path, name_dot_git) =
            git::BareRepository::url_to_relative_path_and_name(&self.url)
                .context(format_context!("Failed to parse {name} url: {}", self.url))?;
        let store_path = workspace.read().get_store_path();
        let lock_file_path = format!(
            "{store_path}/{relative_bare_store_path}/{name_dot_git}.{}",
            lock::LOCK_FILE_SUFFIX
        );
        let mut lock_file = lock::FileLock::new(std::path::Path::new(&lock_file_path).into());

        lock_file
            .lock(progress.console.clone())
            .context(format_context!(
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
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
        filter: Option<String>,
        is_new_branch: IsNewBranch,
    ) -> anyhow::Result<()> {
        logger(progress.console.clone(), self.url.clone()).message(
            format!(
                "execute reference clone with filter {:?}",
                filter.clone().unwrap_or("None".to_string())
            )
            .as_str(),
        );

        if is_new_branch == IsNewBranch::Yes && singleton::get_is_sync() {
            logger(progress.console.clone(), self.url.clone())
                .warning("Skipping update for dev branch during sync operation.");
            return Ok(());
        }

        let (relative_bare_store_path, name_dot_git) =
            git::BareRepository::url_to_relative_path_and_name(&self.url)
                .context(format_context!("Failed to parse {name} url: {}", self.url))?;
        let store_path = workspace.read().get_store_path();
        let store_repo_name: Arc<str> = name_dot_git.clone();
        let bare_repo_path: Arc<str> = format!(
            "{store_path}/{}/{relative_bare_store_path}/{store_repo_name}",
            utils::store::SPACES_STORE_BARE
        )
        .into();

        logger(progress.console.clone(), self.url.clone())
            .debug(format!("bare repository in store at {bare_repo_path}").as_str());

        // Create parent directory for bare repo
        if let Some(parent) = std::path::Path::new(bare_repo_path.as_ref()).parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "{name} - Failed to create bare repository parent directory {}",
                parent.display()
            ))?;
        }

        let lock_file_path = format!("{bare_repo_path}.spaces.lock");
        let mut lock_file = lock::FileLock::new(std::path::Path::new(&lock_file_path).into());

        lock_file
            .lock(progress.console.clone())
            .context(format_context!(
                "{name} - Failed to lock the bare repository {}",
                self.spaces_key
            ))?;

        // Step 1: Ensure bare repository exists and is up to date
        if !std::path::Path::new(bare_repo_path.as_ref()).exists() {
            logger(progress.console.clone(), self.url.clone())
                .debug(format!("Creating bare repository at {bare_repo_path}").as_str());

            let mut clone_arguments: Vec<Arc<str>> = vec!["clone".into(), "--bare".into()];

            if let Some(ref filter_str) = filter {
                clone_arguments.push(format!("--filter={filter_str}").into());
            }

            clone_arguments.push(self.url.clone());
            clone_arguments.push(bare_repo_path.clone());

            git::execute_git_command(
                progress,
                &self.url,
                console::ExecuteOptions {
                    arguments: clone_arguments,
                    ..Default::default()
                },
            )
            .context(format_context!(
                "{name} - Failed to create bare repository at {bare_repo_path}"
            ))?;
        } else {
            logger(progress.console.clone(), self.url.clone())
                .debug(format!("Bare repository exists at {bare_repo_path}").as_str());

            logger(progress.console.clone(), self.url.clone())
                .debug("Fetching updates in bare repository");

            // Fetch updates in bare repo (safe - no working tree to corrupt)
            git::execute_git_command(
                progress,
                &self.url,
                console::ExecuteOptions {
                    arguments: vec![
                        "--git-dir".into(),
                        bare_repo_path.clone(),
                        "fetch".into(),
                        "--all".into(),
                        "--tags".into(),
                        "--prune".into(),
                    ],
                    ..Default::default()
                },
            )
            .context(format_context!(
                "{name} - Failed to fetch in bare repository {bare_repo_path}"
            ))?;
        }

        // Step 2: Handle existing workspace
        let workspace_path = std::path::Path::new(self.spaces_key.as_ref());
        if workspace_path.exists() {
            let entries = std::fs::read_dir(workspace_path).context(format_context!(
                "Internal Error: failed to read directory {}",
                self.spaces_key
            ))?;

            if entries.count() > 0 {
                logger(progress.console.clone(), self.url.clone()).debug(
                    format!(
                        "{} already exists and is populated - try to update",
                        workspace_path.display()
                    )
                    .as_str(),
                );

                let existing_repo = git::Repository::new(self.url.clone(), self.spaces_key.clone());

                if existing_repo.is_dirty(progress) {
                    logger(progress.console.clone(), self.url.clone()).warning(
                        format!(
                            "{} already exists and is dirty - not updating",
                            self.spaces_key
                        )
                        .as_str(),
                    );
                    return Ok(());
                }

                // Fetch from the bare repo (fast, local operation)
                logger(progress.console.clone(), self.url.clone())
                    .debug("Fetching updates in existing workspace");

                existing_repo.fetch(progress).context(format_context!(
                    "{name} - Failed to fetch repository {}",
                    self.spaces_key
                ))?;

                // Checkout the desired revision
                existing_repo
                    .checkout(progress, &self.checkout)
                    .context(format_context!(
                        "{name} - Failed to checkout repository {}",
                        self.spaces_key
                    ))?;

                // If on a branch, pull latest
                if existing_repo.is_head_branch(progress)
                    && existing_repo.is_remote_branch_tracked(progress)
                {
                    existing_repo.pull(progress).context(format_context!(
                        "{name} - Failed to pull repository {} after switching to a branch",
                        self.spaces_key
                    ))?;
                }

                return Ok(());
            }

            logger(progress.console.clone(), self.url.clone()).debug(
                format!(
                    "{} already exists, but is not populated, try to clone",
                    workspace_path.display()
                )
                .as_str(),
            );
        }

        // Step 3: Clone from bare repo using --reference for object sharing
        // This creates a new repo with:
        // - Original URL as remote (not the local bare repo path)
        // - Objects borrowed from bare repo (fast, no network needed)
        // - Fully independent git repository

        logger(progress.console.clone(), self.url.clone())
            .debug(format!("Cloning {} from bare repo with reference", self.spaces_key).as_str());

        let mut clone_arguments: Vec<Arc<str>> =
            vec!["clone".into(), "--reference".into(), bare_repo_path.clone()];

        // Add filter if specified (currently unused - Default and Blobless both use full clones)
        if let Some(ref filter_str) = filter {
            clone_arguments.push(format!("--filter={filter_str}").into());
        }

        // Handle sparse checkout
        if self.sparse_checkout.is_some() {
            clone_arguments.push("--no-checkout".into());
        }

        // Add the original URL (this becomes the remote)
        clone_arguments.push(self.url.clone());

        // Add the destination
        clone_arguments.push(self.spaces_key.clone());

        git::execute_git_command(
            progress,
            &self.url,
            console::ExecuteOptions {
                arguments: clone_arguments,
                ..Default::default()
            },
        )
        .context(format_context!(
            "{name} - Failed to clone with reference from {bare_repo_path}"
        ))?;

        // Step 4: Setup sparse checkout if needed
        if let Some(sparse_checkout) = self.sparse_checkout.as_ref() {
            let workspace_repo = git::Repository::new(self.url.clone(), self.spaces_key.clone());
            workspace_repo
                .setup_sparse_checkout(progress, sparse_checkout)
                .context(format_context!(
                    "Failed to setup sparse checkout in {}",
                    self.spaces_key
                ))?;
        }

        // Step 5: Checkout the desired revision
        let workspace_repo = git::Repository::new(self.url.clone(), self.spaces_key.clone());
        workspace_repo
            .checkout(progress, &self.checkout)
            .context(format_context!(
                "{name} - Failed to checkout revision in {}",
                self.spaces_key
            ))?;

        // Step 6: If on a branch, reset to origin
        if let git::Checkout::Revision(rev) = &self.checkout
            && workspace_repo.is_branch(progress, rev)
        {
            logger(progress.console.clone(), self.url.clone())
                .debug(format!("Resetting to origin/{rev}").as_str());
            workspace_repo
                .reset_hard_origin_branch(progress, rev)
                .context(format_context!(
                    "Failed to reset to origin/{rev} in {}",
                    self.spaces_key
                ))?;
        }

        logger(progress.console.clone(), self.url.clone())
            .debug("Reference clone completed successfully");

        Ok(())
    }

    fn execute_shallow_clone(
        &self,
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let url_logger = logger(progress.console.clone(), self.url.clone());
        url_logger.message("execute shallow clone");

        let branch = match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                return Err(format_error!(
                    "Cannot create a new branch {branch_name} with a shallow clone"
                ));
            }
            git::Checkout::Revision(branch_name) => branch_name.clone(),
        };

        let workspace_directory = self.get_clone_working_directory(workspace.clone());

        let mut clone_arguments: Vec<Arc<str>> = vec![
            "clone".into(),
            "--depth".into(),
            "1".into(),
            self.url.clone(),
            self.spaces_key.clone(),
            "--branch".into(),
            branch.clone(),
            "--single-branch".into(),
        ];

        // Add --no-checkout if sparse checkout is specified
        if self.sparse_checkout.is_some() {
            clone_arguments.push("--no-checkout".into());
        }

        let clone_options = console::ExecuteOptions {
            arguments: clone_arguments,
            working_directory: Some(workspace_directory),
            ..Default::default()
        };

        let clone_path = std::path::Path::new(self.spaces_key.as_ref());
        if clone_path.exists() {
            url_logger.warning(format!("{} already exists", self.spaces_key).as_str());
        } else {
            url_logger.trace(format!("git clone {clone_options:?}").as_str());

            progress
                .execute_process("git", clone_options)
                .context(format_context!(
                    "{name} - Failed to clone repository {}",
                    self.spaces_key
                ))?;
        }

        // Setup sparse checkout if needed
        if let Some(sparse_checkout) = self.sparse_checkout.as_ref() {
            let workspace_repo = git::Repository::new(self.url.clone(), self.spaces_key.clone());

            url_logger.debug("Setting up sparse checkout for shallow clone");

            workspace_repo
                .setup_sparse_checkout(progress, sparse_checkout)
                .context(format_context!(
                    "Failed to setup sparse checkout in {}",
                    self.spaces_key
                ))?;

            // Checkout the files according to sparse checkout configuration
            url_logger.debug("Checking out files for sparse checkout");

            workspace_repo
                .checkout(progress, &self.checkout)
                .context(format_context!(
                    "{name} - Failed to checkout files in sparse shallow clone {}",
                    self.spaces_key
                ))?;
        }

        Ok(())
    }

    pub fn execute(
        &self,
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        // The logic in Repo::uses_bare_repository() needs to stay in sync
        // with the logic here. Default and Blobless use bare repository
        // with reference clone (shared object store via git alternates).
        // Note: Default and Blobless are now equivalent - both create full clones
        // without filters to avoid "unable to read sha1 file" errors.
        //
        let is_new_branch = if workspace.read().is_dev_branch(name) {
            IsNewBranch::Yes
        } else {
            IsNewBranch::No
        };

        match self.clone {
            git::Clone::Worktree => self
                .execute_worktree_clone(progress, workspace.clone(), name)
                .context(format_context!("spaces clone failed"))?,
            git::Clone::Default | git::Clone::Blobless => self
                .execute_default_clone(progress, workspace.clone(), name, None, is_new_branch)
                .context(format_context!("default clone failed"))?,
            git::Clone::Shallow => self
                .execute_shallow_clone(progress, workspace.clone(), name)
                .context(format_context!("default clone failed"))?,
        }

        let ref_name = match &self.checkout {
            git::Checkout::NewBranch(branch_name) => branch_name.clone(),
            git::Checkout::Revision(branch_name) => branch_name.clone(),
        };
        logger(progress.console.clone(), self.url.clone())
            .debug(format!("using ref {ref_name}").as_str());

        let mut is_locked = false;
        let working_directory = self.get_working_directory_in_repo(workspace.clone());

        let mut member = match self.get_member() {
            Ok(mut member) => {
                if member.version.is_none() {
                    let latest_tag =
                        git::get_latest_tag(progress, &self.url, &self.spaces_key).context(
                            format_context!("Failed to get latest tag for {}", self.spaces_key),
                        )?;
                    member.version = Self::rev_to_version(latest_tag.clone());
                }
                Some(member)
            }
            Err(_) => {
                logger(progress.console.clone(), self.url.clone())
                    .warning(format!("Failed to member to settings for: {}", self.url).as_str());
                None
            }
        };

        let is_use_lock = if is_new_branch == IsNewBranch::No {
            workspace.read().settings.is_use_locks()
        } else {
            false
        };

        let is_create_lock_file = workspace.read().is_create_lock_file;

        logger(progress.console.clone(), self.url.clone())
            .debug(format!("Is Create lock file: {is_create_lock_file}").as_str());
        logger(progress.console.clone(), self.url.clone())
            .debug(format!("Is use lock: {is_use_lock}").as_str());

        if workspace.read().is_create_lock_file {
            logger(progress.console.clone(), self.url.clone()).debug("creating lock file");
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
        } else {
            let repo_name = label::get_rule_name_from_label(name);

            // First check for command line locks (always apply if present)
            // Uses comprehensive lookup that handles both simple names and fully-qualified labels
            let command_line_lock = singleton::get_args_lock_for_repo(name);

            // Then check workspace locks only if --locked was passed
            let (commit_hash_lock, lock_source) = if let Some(lock) = command_line_lock {
                logger(progress.console.clone(), self.url.clone())
                    .debug(format!("Found command line lock for {name}: {lock}").as_str());
                (Some(lock), "command line")
            } else if is_use_lock {
                let workspace_read = workspace.read();
                let workspace_lock = workspace_read
                    .locks
                    .get(name)
                    .or(workspace_read.locks.get(repo_name))
                    .cloned();
                if workspace_lock.is_some() {
                    logger(progress.console.clone(), self.url.clone()).debug(
                        format!("Found workspace lock for {name}: {workspace_lock:?}").as_str(),
                    );
                }
                (workspace_lock, "workspace lock file")
            } else {
                (None, "")
            };

            logger(progress.console.clone(), self.url.clone())
                .debug(format!("Is lock for {name}: {commit_hash_lock:?}").as_str());

            if let Some(commit_hash) = commit_hash_lock {
                logger(progress.console.clone(), self.url.clone()).info(
                    format!("applying locked revision {commit_hash} from {lock_source} at {name}")
                        .as_str(),
                );

                let options = console::ExecuteOptions {
                    working_directory: Some(working_directory.clone()),
                    arguments: vec!["checkout".into(), "--detach".into(), commit_hash.clone()],
                    ..Default::default()
                };

                if let Some(member) = member.as_mut() {
                    member.rev = commit_hash.clone();
                    member.version = Self::rev_to_version(Some(commit_hash.clone()));
                }

                logger(progress.console.clone(), self.url.clone())
                    .debug(format!("{}: git {options:?}", self.spaces_key).as_str());

                git::execute_git_command(progress, &self.url, options).context(format_context!(
                    "Failed to checkout commit hash from {}",
                    self.spaces_key
                ))?;

                is_locked = true;
            }
        }

        if is_new_branch == IsNewBranch::Yes && !singleton::get_is_sync() {
            logger(progress.console.clone(), self.url.clone()).message("creating new branch");
            let new_branch = workspace.read().get_new_branch_name();
            let options = console::ExecuteOptions {
                working_directory: Some(working_directory.clone()),
                arguments: vec!["switch".into(), "-c".into(), new_branch],
                ..Default::default()
            };

            logger(progress.console.clone(), self.url.clone())
                .debug(format!("{}: git {options:?}", self.spaces_key).as_str());

            git::execute_git_command(progress, &self.url, options).context(format_context!(
                "Failed to create new branch for {}",
                self.spaces_key
            ))?;

            workspace.write().set_is_not_reproducible();
        }

        // after possibly checking out the lock commit, check for reproducibility
        if !is_locked {
            // check if checkout is on a branch or commit
            let is_branch =
                git::is_branch(progress, &self.url, working_directory.as_ref(), &ref_name);
            if is_branch {
                logger(progress.console.clone(), self.url.clone()).info(
                    format!(
                        "{} is a branch - workspace is not reproducible",
                        self.spaces_key
                    )
                    .as_str(),
                );
                workspace.write().set_is_not_reproducible();
            }
        }

        if let Some(member) = member {
            logger(progress.console.clone(), self.url.clone()).debug("Adding member to workspace");
            workspace.write().add_member(member);
        }

        Ok(())
    }
}
