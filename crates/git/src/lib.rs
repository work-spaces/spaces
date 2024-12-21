use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckoutOption {
    Revision,
    NewBranch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Checkout {
    Revision(Arc<str>),
    NewBranch(Arc<str>),
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub enum Clone {
    Default,
    Worktree,
    Shallow,
    Blobless,
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy, Default)]
pub enum SparseCheckoutMode {
    #[default]
    Cone,
    NoCone,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SparseCheckout {
    pub mode: SparseCheckoutMode,
    pub list: Vec<Arc<str>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repo {
    pub url: Arc<str>,
    pub checkout: CheckoutOption,
    pub rev: Arc<str>,
    pub clone: Option<Clone>,
    pub is_evaluate_spaces_modules: Option<bool>,
    pub sparse_checkout: Option<SparseCheckout>,
}

impl Repo {
    pub fn get_checkout(&self) -> Checkout {
        match &self.checkout {
            CheckoutOption::Revision => Checkout::Revision(self.rev.clone()),
            CheckoutOption::NewBranch => Checkout::NewBranch(self.rev.clone()),
        }
    }
}

pub struct LogEntry {
    pub commit: Arc<str>,
    pub tag: Option<Arc<str>>,
    pub description: Arc<str>,
}

struct State {
    active_repos: HashSet<Arc<str>>,
    log_directory: Option<Arc<str>>,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(RwLock::new(State {
        active_repos: HashSet::new(),
        log_directory: None,
    }));
    STATE.get()
}

pub fn execute_git_command(
    url: &str,
    progress_bar: &mut printer::MultiProgressBar,
    options: printer::ExecuteOptions,
) -> anyhow::Result<Option<String>> {
    let mut is_ready = false;
    use std::ops::DerefMut;

    let log_file_name = format!(
        "git_{}.log",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    let mut log_file_path = None;

    while !is_ready {
        let mut state_lock = get_state().write().unwrap();
        let state = state_lock.deref_mut();

        if state.active_repos.contains(url) {
            is_ready = false;
        } else {
            state.active_repos.insert(url.into());
            is_ready = true;
        }
        log_file_path = state
            .log_directory
            .as_ref()
            .map(|e| format!("{e}/{log_file_name}").into());

        if !is_ready {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    let mut options = options.clone();

    options.log_file_path = log_file_path;
    options
        .environment
        .push(("GIT_TERMINAL_PROMPT".into(), "0".into()));

    progress_bar.log(
        printer::Level::Debug,
        format!("Execute Command for {url}").as_str(),
    );

    let full_command = options.get_full_command_in_working_directory("git");
    let result = progress_bar
        .execute_process("git", options)
        .context(format_context!("{full_command}"));

    {
        let mut state_lock = get_state().write().unwrap();
        let state = state_lock.deref_mut();
        state.active_repos.remove(url);
    }
    progress_bar.log(
        printer::Level::Debug,
        format!("git repo released {url}").as_str(),
    );

    result
}

pub fn get_commit_hash(
    url: &str,
    directory: &str,
    progress_bar: &mut printer::MultiProgressBar,
) -> anyhow::Result<Option<Arc<str>>> {
    let options = printer::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec!["show".into(), "-s".into(), "--format=%H".into()],
        is_return_stdout: true,
        ..Default::default()
    };

    let commit_hash = execute_git_command(url, progress_bar, options).context(format_context!(
        "Failed to get commit hash from {directory}"
    ))?;

    let commit_hash = commit_hash.map(|e| e.trim().into());
    Ok(commit_hash)
}

pub fn is_branch(
    url: &str,
    directory: &str,
    ref_name: &str,
    progress_bar: &mut printer::MultiProgressBar,
) -> bool {
    let options = printer::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec![
            "show-ref".into(),
            "--verify".into(),
            "--quiet".into(),
            format!("refs/heads/{}", ref_name).into(),
        ],
        ..Default::default()
    };
    execute_git_command(url, progress_bar, options).is_ok()
}

pub fn get_commit_tag(
    url: &str,
    directory: &str,
    progress_bar: &mut printer::MultiProgressBar,
) -> Option<Arc<str>> {
    let options = printer::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec!["describe".into(), "--exact-match".into(), "HEAD".into()],
        is_return_stdout: true,
        ..Default::default()
    };

    if let Ok(Some(stdout)) = execute_git_command(url, progress_bar, options) {
        let stdout_trimmed = stdout.trim();
        Some(stdout_trimmed.into())
    } else {
        None
    }
}

pub fn get_branch_log(
    url: &str,
    directory: &str,
    branch: &str,
    progress_bar: &mut printer::MultiProgressBar,
) -> anyhow::Result<Vec<LogEntry>> {
    let options = printer::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec![
            "log".into(),
            "--oneline".into(),
            "--decorate=short".into(),
            "--no-color".into(),
            "--reverse".into(),
            "--pretty=format:\"%H;%D;%s\"".into(),
            branch.into(),
        ],
        is_return_stdout: true,
        ..Default::default()
    };

    let stdout_option = execute_git_command(url, progress_bar, options)
        .context(format_context!("Failed to get branch log from {directory}"))?;

    if let Some(stdout) = stdout_option {
        let mut log_entries = Vec::new();
        for line in stdout.lines() {
            let line = line.trim_matches('"');
            let parts: Vec<&str> = line.split(';').collect();
            if parts.len() == 3 {
                let tag = parts[1].strip_prefix("tag: ").map(|tag| tag.into());

                log_entries.push(LogEntry {
                    commit: parts[0].into(),
                    tag,
                    description: parts[2].into(),
                });
            }
        }
        Ok(log_entries)
    } else {
        Ok(Vec::new())
    }
}

#[derive(Clone, Debug)]
pub struct BareRepository {
    pub url: Arc<str>,
    pub full_path: Arc<str>,
    pub spaces_key: Arc<str>,
    pub name_dot_git: Arc<str>,
}

impl BareRepository {
    pub fn new(
        progress_bar: &mut printer::MultiProgressBar,
        bare_store_path: &str,
        spaces_key: &str,
        url: &str,
    ) -> anyhow::Result<Self> {
        let mut options = printer::ExecuteOptions::default();

        let (relative_bare_store_path, name_dot_git) = Self::url_to_relative_path_and_name(url)
            .context(format_context!("Failed to parse {spaces_key} url: {url}"))?;

        let bare_store_path: Arc<str> =
            format!("{bare_store_path}/{relative_bare_store_path}").into();

        std::fs::create_dir_all(bare_store_path.as_ref())
            .context(format_context!("failed to creat dir {bare_store_path}"))?;

        let full_path: Arc<str> = format!("{}{}", bare_store_path, name_dot_git).into();

        if !std::path::Path::new(full_path.as_ref()).exists() {
            options.working_directory = Some(bare_store_path);

            options.arguments = vec![
                "clone".into(),
                "--bare".into(),
                "--filter=blob:none".into(),
                url.into(),
            ];

            execute_git_command(url, progress_bar, options)
                .context(format_context!("while creating bare repo"))?;

            let options_git_config_auto_push = printer::ExecuteOptions {
                working_directory: Some(full_path.clone()),
                arguments: vec![
                    "config".into(),
                    "--add".into(),
                    "--bool".into(),
                    "push.autoSetupRemote".into(),
                    "true".into(),
                ],
                ..Default::default()
            };

            execute_git_command(url, progress_bar, options_git_config_auto_push)
                .context(format_context!("while configuring auto-push"))?;

            let options_git_config = printer::ExecuteOptions {
                working_directory: Some(full_path.clone()),
                arguments: vec![
                    "config".into(),
                    "remote.origin.fetch".into(),
                    "refs/heads/*:refs/remotes/origin/*".into(),
                ],
                ..Default::default()
            };

            execute_git_command(url, progress_bar, options_git_config)
                .context(format_context!("while setting git options"))?;
        }

        Ok(Self {
            url: url.into(),
            full_path,
            spaces_key: spaces_key.into(),
            name_dot_git,
        })
    }

    pub fn add_worktree(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
        path: &str,
    ) -> anyhow::Result<Worktree> {
        let result = Worktree::new(progress_bar, self, path)
            .context(format_context!("Adding worktree to {} at {path}", self.url))?;
        Ok(result)
    }

    pub fn url_to_relative_path_and_name(url: &str) -> anyhow::Result<(Arc<str>, Arc<str>)> {
        let repo_url = url::Url::parse(url)
            .context(format_context!("Failed to parse bare store url {url}"))?;

        let host = repo_url
            .host_str()
            .ok_or(format_error!("No host found in url {}", url))?;
        let scheme = repo_url.scheme();
        let path_segments = repo_url
            .path_segments()
            .ok_or(format_error!("No path found in url {}", url))?;

        let mut path = String::new();
        let mut repo_name = String::new();
        let count = path_segments.clone().count();
        if count > 1 {
            path.push('/');
            for (index, segment) in path_segments.enumerate() {
                if index == count - 1 {
                    repo_name = segment.to_string();
                    break;
                }
                path.push_str(segment);
                path.push('/');
            }
        } else {
            path.push('/');
        }

        let bare_store = format!("{scheme}/{host}{path}");
        repo_name.push_str(".git");

        Ok((bare_store.into(), repo_name.into()))
    }

    #[allow(dead_code)]
    pub fn get_workspace_name_from_url(url: &str) -> anyhow::Result<Arc<str>> {
        let (_, repo_name) = Self::url_to_relative_path_and_name(url)?;

        repo_name
            .strip_suffix(".git")
            .ok_or(format_error!(
                "Failed to extract a workspace name from  {url}",
            ))
            .map(|e| e.into())
    }
}

pub struct Worktree {
    pub full_path: Arc<str>,
    pub url: Arc<str>,
}

impl Worktree {
    fn new(
        progress_bar: &mut printer::MultiProgressBar,
        repository: &BareRepository,
        path: &str,
    ) -> anyhow::Result<Self> {
        let mut options = printer::ExecuteOptions::default();

        if !std::path::Path::new(&path).is_absolute() {
            return Err(format_error!(
                "Path to worktree must be an absolute path: {}",
                path
            ));
        }

        std::fs::create_dir_all(path).context(format_context!("failed to create dir {path}"))?;

        options.working_directory = Some(repository.full_path.clone());
        options.arguments = vec!["worktree".into(), "prune".into()];

        execute_git_command(&repository.url, progress_bar, options.clone())
            .context(format_context!("while pruning worktree"))?;

        let full_path: Arc<str> = format!("{}/{}", path, repository.spaces_key).into();
        if !std::path::Path::new(full_path.as_ref()).exists() {
            options.arguments = vec![
                "worktree".into(),
                "add".into(),
                "--detach".into(),
                full_path.clone(),
            ];

            execute_git_command(&repository.url, progress_bar, options)
                .context(format_context!("while adding detached worktree"))?;
        }

        Ok(Self {
            full_path,
            url: repository.url.clone(),
        })
    }

    pub fn to_repository(&self) -> Repository {
        Repository::new(self.url.clone(), self.full_path.clone())
    }

    pub fn get_spaces_star(&self) -> anyhow::Result<Option<Arc<str>>> {
        //check for spaces.star in full_path and return Some string if the file exists
        let star_file = format!("{}/spaces.star", self.full_path);
        if std::path::Path::new(&star_file).exists() {
            Ok(Some(star_file.into()))
        } else {
            Ok(None)
        }
    }

    pub fn checkout(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
        revision: &str,
    ) -> anyhow::Result<()> {
        let repo = self.to_repository();
        let arguments = vec!["fetch".into(), "origin".into(), revision.into()];
        repo.execute(progress_bar, arguments)
            .context(format_context!("while fetching existing bare repository"))?;
        let arguments = vec!["checkout".into(), "--detach".into(), revision.into()];
        repo.execute(progress_bar, arguments)
            .context(format_context!("checkout {revision:?}"))?;

        Ok(())
    }

    pub fn checkout_detached_head(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let repo = self.to_repository();
        let arguments = vec!["checkout".into(), "--detach".into(), "HEAD".into()];
        repo.execute(progress_bar, arguments)
            .context(format_context!("detech head"))?;

        Ok(())
    }

    pub fn switch_new_branch(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
        dev_branch: &str,
        revision: &str,
    ) -> anyhow::Result<()> {
        self.checkout(progress_bar, revision)
            .context(format_context!("switch new branch {:?}", revision))?;

        let repo = self.to_repository();
        let arguments = vec!["switch".into(), "-c".into(), dev_branch.into()];

        repo.execute(progress_bar, arguments)
            .context(format_context!("switch new branch"))?;

        Ok(())
    }
}

pub struct Repository {
    pub full_path: Arc<str>,
    pub url: Arc<str>,
}

impl Repository {
    pub fn new(url: Arc<str>, full_path: Arc<str>) -> Self {
        Self { url, full_path }
    }

    pub fn new_clone(
        progress: &mut printer::MultiProgressBar,
        url: Arc<str>,
        working_directory: Arc<str>,
        clone_name: Arc<str>,
        arguments: Vec<Arc<str>>,
    ) -> anyhow::Result<Self> {
        let clone_path = std::path::Path::new(clone_name.as_ref());
        if clone_path.exists() {
            progress.log(
                printer::Level::Info,
                format!("{} already exists", clone_name).as_str(),
            );
        } else {
            progress.log(
                printer::Level::Message,
                format!("{}: git {}", url, arguments.join(" ")).as_str(),
            );

            let clone_options = printer::ExecuteOptions {
                arguments,
                working_directory: Some(working_directory.clone()),
                ..Default::default()
            };

            progress
                .execute_process("git", clone_options)
                .context(format_context!("Failed to clone repository {}", clone_name))?;
        }
        let full_path: Arc<str> = format!("{working_directory}/{clone_name}").into();

        Ok(Self::new(url, full_path))
    }

    pub fn resolve_revision(
        &self,
        progress: &mut printer::MultiProgressBar,
        revision: &str,
    ) -> anyhow::Result<Arc<str>> {
        let mut result = revision.to_string();
        let parts = revision.split(':').collect::<Vec<&str>>();
        if parts.len() == 2 {
            let branch = parts[0];
            let semver = parts[1];
            let logs =
                get_branch_log(&self.url, self.full_path.as_ref(), branch, progress).context(
                    format_context!("Failed to get branch log for {}", self.full_path),
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
        Ok(result.into())
    }

    pub fn execute(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
        args: Vec<Arc<str>>,
    ) -> anyhow::Result<()> {
        let options = printer::ExecuteOptions {
            working_directory: Some(self.full_path.clone()),
            arguments: args,
            ..Default::default()
        };

        progress_bar.log(
            printer::Level::Message,
            format!("{}: git {}", self.url, options.arguments.join(" ")).as_str(),
        );

        execute_git_command(&self.url, progress_bar, options)
            .context(format_context!("while executing git command"))?;
        Ok(())
    }

    pub fn setup_sparse_checkout(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
        sparse_checkout: &SparseCheckout,
    ) -> anyhow::Result<()> {
        let mode_arg = match sparse_checkout.mode {
            SparseCheckoutMode::Cone => "--cone",
            SparseCheckoutMode::NoCone => "--no-cone",
        };

        self.execute(
            progress_bar,
            vec!["sparse-checkout".into(), "init".into(), mode_arg.into()],
        )
        .context(format_context!(
            "Failed to init sparse checkout in {}",
            self.full_path
        ))?;

        let mut arguments = vec!["sparse-checkout".into(), "set".into()];

        arguments.extend(sparse_checkout.list.iter().map(|e| e.clone()));

        self.execute(progress_bar, arguments)
            .context(format_context!(
                "Failed to set sparse checkout in {}",
                self.full_path
            ))?;

        Ok(())
    }

    pub fn checkout(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
        checkout: &Checkout,
    ) -> anyhow::Result<()> {
        let mut checkout_args = Vec::new();
        match checkout {
            Checkout::NewBranch(branch_name) => {
                checkout_args.push("switch".into());
                checkout_args.push("-c".into());
                checkout_args.push(branch_name.clone());
                // TODO: switch to a new branch
            }
            Checkout::Revision(revision) => {
                // if revision of the format "branch:semver" then get the tags on the branch
                let revision = self
                    .resolve_revision(progress_bar, revision)
                    .context(format_context!("failed to resolve revision"))?;

                checkout_args.push("checkout".into());
                checkout_args.push(revision.clone().into());
            }
        }

        self.execute(progress_bar, checkout_args)
            .context(format_context!("while checking out {}", self.full_path))?;

        Ok(())
    }
}
