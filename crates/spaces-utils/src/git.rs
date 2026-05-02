use crate::logger;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use strum::Display;

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

#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Display, clap::ValueEnum)]
pub enum Clone {
    Default,
    Worktree,
    Shallow,
    Blobless,
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy, Default, Display)]
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

impl SparseCheckout {
    pub fn get_hash_string(&self) -> Arc<str> {
        let mut sparse_string = self.mode.to_string();
        for item in self.list.iter() {
            sparse_string.push_str(item);
        }
        // do a blake3 hash of sparse_string for the suffix
        let hash = blake3::hash(sparse_string.as_bytes());
        hash.to_string().into()
    }
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
    pub working_directory: Option<Arc<str>>,
}

impl Repo {
    pub fn get_checkout(&self) -> Checkout {
        match &self.checkout {
            CheckoutOption::Revision => Checkout::Revision(self.rev.clone()),
            CheckoutOption::NewBranch => Checkout::NewBranch(self.rev.clone()),
        }
    }

    pub fn uses_bare_repository(&self) -> bool {
        // Returns true for Default or Blobless clone modes (or when unspecified,
        // since the executor defaults to Clone::Default).
        // These modes use bare repositories in the store with git --reference
        // to create workspace clones that share objects via git alternates.
        // This is more efficient than COW and works across filesystems.
        //
        // NOTE: This logic must stay in sync with the `match self.clone` in
        // executor/git.rs `execute()`. When clone is None the executor picks
        // Clone::Default, so we must treat None the same way here.
        matches!(
            &self.clone,
            None | Some(Clone::Default) | Some(Clone::Blobless)
        )
    }
}

pub struct LogEntry {
    pub commit: Arc<str>,
    pub tag: Option<Arc<str>>,
    pub description: Arc<str>,
}

pub struct ResolveRevision {
    pub latest_semver_tag: Option<Arc<str>>,
    pub commit: Arc<str>,
}

struct State {
    active_repos: HashSet<Arc<str>>,
    log_directory: Option<Arc<str>>,
}

/// Maximum number of retry attempts for transient git network errors.
const GIT_MAX_RETRIES: u32 = 3;
/// Initial backoff duration in milliseconds before the first retry.
const GIT_INITIAL_BACKOFF_MS: u64 = 1000;
/// Multiplier applied to the backoff duration after each retry.
const GIT_BACKOFF_MULTIPLIER: u64 = 2;

/// Computes the backoff duration for a given retry attempt, with deterministic jitter.
fn git_backoff_duration(attempt: u32) -> std::time::Duration {
    let base_ms = GIT_INITIAL_BACKOFF_MS * GIT_BACKOFF_MULTIPLIER.saturating_pow(attempt);
    let jitter_ms = base_ms / 4;
    let effective_ms = if attempt.is_multiple_of(2) {
        base_ms.saturating_sub(jitter_ms)
    } else {
        base_ms.saturating_add(jitter_ms)
    };
    std::time::Duration::from_millis(effective_ms)
}

/// Returns true if the git error is a transient network error worth retrying.
///
/// Checks for:
/// - Exit code 74 (EX_IOERR from sysexits.h — I/O error, commonly network)
/// - Exit code 128 with network-related stderr messages
/// - Common network/transport error patterns in the error message
fn is_retryable_git_error(error: &anyhow::Error) -> bool {
    let error_string = format!("{error:#}");
    let lower_error = error_string.to_lowercase();

    // Check for exit code 74 (EX_IOERR) — commonly a network I/O error
    if error_string.contains("exit code: 74") {
        return true;
    }

    // Exit code 128 is git's generic fatal error — retry only if the message
    // indicates a network/transport issue.
    let is_exit_128 = error_string.contains("exit code: 128");

    // Network-related patterns found in git stderr output
    let network_patterns = [
        "could not resolve host",
        "unable to access",
        "connection refused",
        "connection timed out",
        "connection reset by peer",
        "ssl_error",
        "ssl_connect",
        "openssl",
        "gnutls",
        "failed to connect",
        "couldn't connect to server",
        "the requested url returned error: 5", // 5xx server errors
        "the requested url returned error: 429", // rate limiting
        "couldn't connect to host",
        "failed to connect to",
        "network is unreachable",
        "no route to host",
        "operation timed out",
        "name or service not known",
        "temporary failure in name resolution",
        "early eof",
        "index-pack failed",
        "rpc failed",
        "unexpected disconnect",
        "transfer closed",
        "the remote end hung up unexpectedly",
    ];

    if is_exit_128 {
        for pattern in &network_patterns {
            if lower_error.contains(pattern) {
                return true;
            }
        }
    }

    // Also check for network patterns regardless of exit code (e.g. exit code 56, etc.)
    for pattern in &[
        "the remote end hung up unexpectedly",
        "early eof",
        "rpc failed",
        "unexpected disconnect",
        "transfer closed",
    ] {
        if lower_error.contains(pattern) {
            return true;
        }
    }

    false
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

fn url_logger(console: console::Console, url: &str) -> logger::Logger {
    logger::Logger::new(console, url.into())
}

pub fn execute_git_command(
    progress: &mut console::Progress,
    url: &str,
    options: console::ExecuteOptions,
) -> anyhow::Result<Option<String>> {
    use std::ops::DerefMut;

    for attempt in 0..=GIT_MAX_RETRIES {
        if attempt > 0 {
            let wait = git_backoff_duration(attempt - 1);
            url_logger(progress.console.clone(), url).debug(
                format!("Retry attempt {attempt}/{GIT_MAX_RETRIES} after {wait:?}").as_str(),
            );
            std::thread::sleep(wait);
        }

        let mut is_ready = false;

        let log_file_name = format!(
            "git_{}.log",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );

        let mut log_file_path = None;

        url_logger(progress.console.clone(), url).debug("Waiting for lock");

        while !is_ready {
            {
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
            }

            if !is_ready {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }

        let mut attempt_options = options.clone();

        attempt_options.log_file_path = log_file_path;
        attempt_options
            .environment
            .push(("GIT_TERMINAL_PROMPT".into(), "0".into()));

        if let Some(directory) = attempt_options.working_directory.as_ref() {
            url_logger(progress.console.clone(), url).debug(format!("cwd: {directory}").as_str());
        }
        url_logger(progress.console.clone(), url)
            .debug(format!("git {}", attempt_options.arguments.join(" ")).as_str());

        let full_command = attempt_options.get_full_command_in_working_directory("git");
        let result = progress
            .execute_process("git", attempt_options)
            .context(format_context!("{full_command}"));

        {
            let mut state_lock = get_state().write().unwrap();
            let state = state_lock.deref_mut();
            state.active_repos.remove(url);
        }
        url_logger(progress.console.clone(), url).trace("Released");

        match result {
            Ok(value) => {
                if attempt > 0 {
                    url_logger(progress.console.clone(), url)
                        .info(format!("Succeeded after {attempt} retry(ies)").as_str());
                }
                return Ok(value.stdout);
            }
            Err(err) => {
                if attempt < GIT_MAX_RETRIES && is_retryable_git_error(&err) {
                    url_logger(progress.console.clone(), url).debug(
                        format!(
                            "Transient network error (attempt {}/{}): {err:#}",
                            attempt + 1,
                            GIT_MAX_RETRIES + 1
                        )
                        .as_str(),
                    );
                    continue;
                }
                return Err(err);
            }
        }
    }

    Err(format_error!(
        "git command for {url} failed after {GIT_MAX_RETRIES} retries"
    ))
}

pub fn get_commit_hash(
    progress_bar: &mut console::Progress,
    url: &str,
    directory: &str,
) -> anyhow::Result<Option<Arc<str>>> {
    let options = console::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec!["rev-parse".into(), "HEAD".into()],
        is_return_stdout: true,
        ..Default::default()
    };

    let commit_hash = execute_git_command(progress_bar, url, options).context(format_context!(
        "Failed to get commit hash from {directory}"
    ))?;

    let commit_hash = commit_hash.map(|e| e.trim().into());
    Ok(commit_hash)
}

pub fn is_head_branch(progress_bar: &mut console::Progress, url: &str, directory: &str) -> bool {
    let options = console::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec!["symbolic-ref".into(), "--quiet".into(), "HEAD".into()],
        ..Default::default()
    };
    execute_git_command(progress_bar, url, options).is_ok()
}

pub fn is_branch(
    progress_bar: &mut console::Progress,
    url: &str,
    directory: &str,
    ref_name: &str,
) -> bool {
    let options = console::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec![
            "show-ref".into(),
            "--verify".into(),
            "--quiet".into(),
            format!("refs/heads/{ref_name}").into(),
        ],
        ..Default::default()
    };
    execute_git_command(progress_bar, url, options).is_ok()
}

pub fn is_current_branch(
    progress_bar: &mut console::Progress,
    url: &str,
    directory: &str,
    ref_name: &str,
) -> bool {
    let options = console::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec!["branch".into(), "--show-current".into()],
        is_return_stdout: true,
        ..Default::default()
    };
    let branch = execute_git_command(progress_bar, url, options)
        .unwrap_or(None)
        .map(|s| s.trim().to_string());

    if let Some(branch) = branch {
        branch == ref_name
    } else {
        false
    }
}

pub fn is_currently_on_a_branch(
    progress_bar: &mut console::Progress,
    url: &str,
    directory: &str,
) -> bool {
    let options = console::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec![
            "symbolic-ref".into(),
            "--short".into(),
            "-q".into(),
            "HEAD".into(),
        ],
        is_return_stdout: true,
        ..Default::default()
    };
    execute_git_command(progress_bar, url, options).is_ok()
}

pub fn is_dirty(progress_bar: &mut console::Progress, url: &str, directory: &str) -> bool {
    let options = console::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec!["status".into(), "--porcelain".into()],
        is_return_stdout: true,
        ..Default::default()
    };
    let output = execute_git_command(progress_bar, url, options).unwrap_or(None);
    if let Some(output) = output {
        !output.is_empty()
    } else {
        false
    }
}

pub fn get_latest_tag(
    progress_bar: &mut console::Progress,
    url: &str,
    directory: &str,
) -> anyhow::Result<Option<Arc<str>>> {
    if !is_head_branch(progress_bar, url, directory) {
        return Ok(get_commit_tag(progress_bar, url, directory));
    }

    let logs = get_branch_log(progress_bar, url, directory, "HEAD")
        .context(format_context!("Failed to read git logs for {}", directory))?;

    for log in logs.iter().rev() {
        if let Some(tag) = log.tag.as_ref() {
            return Ok(Some(tag.clone()));
        }
    }

    Ok(None)
}

pub fn get_commit_tag(
    progress_bar: &mut console::Progress,
    url: &str,
    directory: &str,
) -> Option<Arc<str>> {
    let options = console::ExecuteOptions {
        working_directory: Some(directory.into()),
        arguments: vec!["describe".into(), "--exact-match".into(), "HEAD".into()],
        is_return_stdout: true,
        ..Default::default()
    };

    if let Ok(Some(stdout)) = execute_git_command(progress_bar, url, options) {
        let stdout_trimmed = stdout.trim();
        Some(stdout_trimmed.into())
    } else {
        None
    }
}

pub fn is_bare_repo_healthy(progress_bar: &mut console::Progress, path: &str) -> bool {
    let options = console::ExecuteOptions {
        working_directory: Some(path.into()),
        arguments: vec!["fsck".into(), "--no-progress".into()],
        ..Default::default()
    };
    execute_git_command(progress_bar, path, options).is_ok()
}

pub fn run_bare_repo_maintenance(progress_bar: &mut console::Progress, path: &str) -> bool {
    let options = console::ExecuteOptions {
        working_directory: Some(path.into()),
        arguments: vec!["gc".into()],
        ..Default::default()
    };
    execute_git_command(progress_bar, path, options).is_ok()
}

pub fn fetch_bare_repo(progress_bar: &mut console::Progress, path: &str) -> bool {
    let options = console::ExecuteOptions {
        working_directory: Some(path.into()),
        arguments: vec!["fetch".into(), "--prune".into()],
        ..Default::default()
    };
    execute_git_command(progress_bar, path, options).is_ok()
}

fn get_branch_log(
    progress_bar: &mut console::Progress,
    url: &str,
    directory: &str,
    branch: &str,
) -> anyhow::Result<Vec<LogEntry>> {
    let options = console::ExecuteOptions {
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

    let stdout_option = execute_git_command(progress_bar, url, options)
        .context(format_context!("Failed to get branch log from {directory}"))?;

    if let Some(stdout) = stdout_option {
        let mut log_entries = Vec::new();
        for line in stdout.lines() {
            let line = line.trim_matches('"');
            let parts: Vec<&str> = line.split(';').collect();
            if parts.len() == 3 {
                let sections = parts[1].split(", ").collect::<Vec<&str>>();
                for section in sections {
                    if section.starts_with("tag: ") {
                        let tag = section.strip_prefix("tag: ").map(|tag| tag.into());
                        log_entries.push(LogEntry {
                            commit: parts[0].into(),
                            tag,
                            description: parts[2].into(),
                        });
                    }
                }
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
        progress_bar: &mut console::Progress,
        bare_store_path: &str,
        spaces_key: &str,
        url: &str,
    ) -> anyhow::Result<Self> {
        let mut options = console::ExecuteOptions::default();

        let (relative_bare_store_path, name_dot_git) = Self::url_to_relative_path_and_name(url)
            .context(format_context!("Failed to parse {spaces_key} url: {url}"))?;

        let bare_store_path: Arc<str> =
            format!("{bare_store_path}/{relative_bare_store_path}").into();

        std::fs::create_dir_all(bare_store_path.as_ref())
            .context(format_context!("failed to creat dir {bare_store_path}"))?;

        let full_path: Arc<str> = format!("{bare_store_path}{name_dot_git}").into();

        if !std::path::Path::new(full_path.as_ref()).exists() {
            options.working_directory = Some(bare_store_path);

            options.arguments = vec!["clone".into(), "--bare".into(), url.into()];

            execute_git_command(progress_bar, url, options)
                .context(format_context!("while creating bare repo"))?;

            let options_git_config_auto_push = console::ExecuteOptions {
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

            execute_git_command(progress_bar, url, options_git_config_auto_push)
                .context(format_context!("while configuring auto-push"))?;

            let options_git_config = console::ExecuteOptions {
                working_directory: Some(full_path.clone()),
                arguments: vec![
                    "config".into(),
                    "remote.origin.fetch".into(),
                    "refs/heads/*:refs/remotes/origin/*".into(),
                ],
                ..Default::default()
            };

            execute_git_command(progress_bar, url, options_git_config)
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
        progress_bar: &mut console::Progress,
        path: &str,
    ) -> anyhow::Result<Worktree> {
        let result = Worktree::new(progress_bar, self, path)
            .context(format_context!("Adding worktree to {} at {path}", self.url))?;
        Ok(result)
    }

    pub fn add_worktree_on_branch(
        &self,
        progress_bar: &mut console::Progress,
        path: &str,
        branch: &str,
    ) -> anyhow::Result<Worktree> {
        let result =
            Worktree::new_on_branch(progress_bar, self, path, branch).context(format_context!(
                "Adding worktree on branch {branch} to {} at {path}",
                self.url
            ))?;
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
        if !repo_name.ends_with(".git") {
            repo_name.push_str(".git");
        }

        Ok((bare_store.into(), repo_name.into()))
    }
}

pub struct Worktree {
    pub full_path: Arc<str>,
    pub url: Arc<str>,
}

impl Worktree {
    fn new(
        progress_bar: &mut console::Progress,
        repository: &BareRepository,
        path: &str,
    ) -> anyhow::Result<Self> {
        let mut options = console::ExecuteOptions::default();

        if !std::path::Path::new(&path).is_absolute() {
            return Err(format_error!(
                "Path to worktree must be an absolute path: {}",
                path
            ));
        }

        std::fs::create_dir_all(path).context(format_context!("failed to create dir {path}"))?;

        options.working_directory = Some(repository.full_path.clone());
        options.arguments = vec!["worktree".into(), "prune".into()];

        execute_git_command(progress_bar, &repository.url, options.clone())
            .context(format_context!("while pruning worktree"))?;

        let full_path: Arc<str> = format!("{}/{}", path, repository.spaces_key).into();
        if !std::path::Path::new(full_path.as_ref()).exists() {
            options.arguments = vec![
                "worktree".into(),
                "add".into(),
                "--detach".into(),
                full_path.clone(),
            ];

            execute_git_command(progress_bar, &repository.url, options)
                .context(format_context!("while adding detached worktree"))?;
        }

        Ok(Self {
            full_path,
            url: repository.url.clone(),
        })
    }

    fn new_on_branch(
        progress_bar: &mut console::Progress,
        repository: &BareRepository,
        path: &str,
        branch: &str,
    ) -> anyhow::Result<Self> {
        let mut options = console::ExecuteOptions::default();

        if !std::path::Path::new(&path).is_absolute() {
            return Err(format_error!(
                "Path to worktree must be an absolute path: {}",
                path
            ));
        }

        std::fs::create_dir_all(path).context(format_context!("failed to create dir {path}"))?;

        options.working_directory = Some(repository.full_path.clone());
        options.arguments = vec!["worktree".into(), "prune".into()];

        execute_git_command(progress_bar, &repository.url, options.clone())
            .context(format_context!("while pruning worktree"))?;

        let full_path: Arc<str> = format!("{}/{}", path, repository.spaces_key).into();
        if !std::path::Path::new(full_path.as_ref()).exists() {
            // Create worktree on the existing branch
            // In a bare repository, branches are stored as refs/heads/branch
            options.arguments = vec![
                "worktree".into(),
                "add".into(),
                full_path.clone(),
                branch.into(),
            ];

            execute_git_command(progress_bar, &repository.url, options)
                .context(format_context!("while adding worktree on branch {branch}"))?;
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
        progress_bar: &mut console::Progress,
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
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<()> {
        let repo = self.to_repository();
        let arguments = vec!["checkout".into(), "--detach".into(), "HEAD".into()];
        repo.execute(progress_bar, arguments)
            .context(format_context!("detech head"))?;

        Ok(())
    }

    pub fn switch_new_branch(
        &self,
        progress_bar: &mut console::Progress,
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

    pub fn switch_to_branch(
        &self,
        progress_bar: &mut console::Progress,
        branch: &str,
    ) -> anyhow::Result<()> {
        let repo = self.to_repository();

        // Fetch all refs from origin
        let arguments = vec!["fetch".into(), "origin".into()];
        repo.execute(progress_bar, arguments)
            .context(format_context!("while fetching from origin"))?;

        // Checkout the branch with --track to set up tracking
        let arguments = vec![
            "checkout".into(),
            "--track".into(),
            "-B".into(),
            branch.into(),
            format!("origin/{branch}").into(),
        ];
        repo.execute(progress_bar, arguments)
            .context(format_context!("while switching to branch {branch}"))?;

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
        progress: &mut console::Progress,
        url: Arc<str>,
        working_directory: Arc<str>,
        clone_name: Arc<str>,
        arguments: Vec<Arc<str>>,
    ) -> anyhow::Result<Self> {
        url_logger(progress.console.clone(), url.as_ref())
            .message(format!("git {}", arguments.join(" ")).as_str());

        let clone_options = console::ExecuteOptions {
            arguments,
            working_directory: Some(working_directory.clone()),
            ..Default::default()
        };

        progress
            .execute_process("git", clone_options)
            .context(format_context!("Failed to clone repository {}", clone_name))?;

        let full_path: Arc<str> = format!("{working_directory}/{clone_name}").into();

        Ok(Self::new(url, full_path))
    }

    pub fn resolve_revision(
        &self,
        progress: &mut console::Progress,
        revision: &str,
    ) -> anyhow::Result<ResolveRevision> {
        let mut result = ResolveRevision {
            commit: revision.into(),
            latest_semver_tag: None,
        };
        let parts = revision.split(':').collect::<Vec<&str>>();
        if parts.len() == 2 {
            let branch = parts[0];
            let semver = parts[1];
            let logs =
                get_branch_log(progress, &self.url, self.full_path.as_ref(), branch).context(
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
                    url_logger(progress.console.clone(), self.url.as_ref())
                        .debug(format!("Found tag:{tag}").as_str());
                    let stripped_tag = tag.trim_matches('v');
                    if let Ok(version) = semver::Version::parse(stripped_tag) {
                        if required.matches(&version) {
                            url_logger(progress.console.clone(), self.url.as_ref()).debug(
                                format!(
                                    "Found tag {stripped_tag} for branch {branch} that satisfies semver requirement"
                                )
                                .as_str(),
                            );
                            is_semver_satisfied = true;
                            result.latest_semver_tag = Some(tag.clone());
                        } else if is_semver_satisfied {
                            url_logger(progress.console.clone(), self.url.as_ref()).debug(
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
                result.commit = commit;
            }
        } else if parts.len() != 1 {
            return Err(format_error!(
                "Invalid revision format. Use `<branch>:<semver requirement>`"
            ));
        }
        url_logger(progress.console.clone(), self.url.as_ref()).message(
            format!(
                "Resolved revision {} to latest tag:{:?}, commit:{}",
                revision, result.latest_semver_tag, result.commit
            )
            .as_str(),
        );
        Ok(result)
    }

    pub fn execute(
        &self,
        progress: &mut console::Progress,
        args: Vec<Arc<str>>,
    ) -> anyhow::Result<()> {
        let options = console::ExecuteOptions {
            working_directory: Some(self.full_path.clone()),
            arguments: args,
            ..Default::default()
        };

        url_logger(progress.console.clone(), self.url.as_ref())
            .debug(format!("git {}", options.arguments.join(" ")).as_str());

        execute_git_command(progress, &self.url, options)
            .context(format_context!("while executing git command"))?;
        Ok(())
    }

    pub fn setup_sparse_checkout(
        &self,
        progress_bar: &mut console::Progress,
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

        arguments.extend(sparse_checkout.list.iter().cloned());

        self.execute(progress_bar, arguments)
            .context(format_context!(
                "Failed to set sparse checkout in {}",
                self.full_path
            ))?;

        Ok(())
    }

    pub fn is_remote_branch_tracked(&self, progress_bar: &mut console::Progress) -> bool {
        self.execute(
            progress_bar,
            vec![
                "rev-parse".into(),
                "--abbrev-ref".into(),
                "--symbolic-full-name".into(),
                "@{u}".into(),
            ],
        )
        .is_ok()
    }

    pub fn pull(&self, progress_bar: &mut console::Progress) -> anyhow::Result<()> {
        self.execute(progress_bar, vec!["pull".into()])
            .context(format_context!("while pulling from {}", self.full_path))?;
        Ok(())
    }

    pub fn reset_hard_origin_branch(
        &self,
        progress_bar: &mut console::Progress,
        branch: &str,
    ) -> anyhow::Result<()> {
        self.execute(
            progress_bar,
            vec![
                "reset".into(),
                "--hard".into(),
                format!("origin/{branch}").into(),
            ],
        )
        .context(format_context!(
            "while resetting to origin from {}",
            self.full_path
        ))?;
        Ok(())
    }

    pub fn fetch(&self, progress_bar: &mut console::Progress) -> anyhow::Result<()> {
        self.execute(progress_bar, vec!["fetch".into()])
            .context(format_context!("while fetching from {}", self.full_path))?;
        Ok(())
    }

    pub fn is_branch(&self, progress_bar: &mut console::Progress, ref_name: &str) -> bool {
        is_branch(progress_bar, &self.url, &self.full_path, ref_name)
    }

    pub fn is_current_branch(&self, progress_bar: &mut console::Progress, ref_name: &str) -> bool {
        is_current_branch(progress_bar, &self.url, &self.full_path, ref_name)
    }

    pub fn is_currently_on_a_branch(&self, progress_bar: &mut console::Progress) -> bool {
        is_currently_on_a_branch(progress_bar, &self.url, &self.full_path)
    }

    pub fn is_dirty(&self, progress_bar: &mut console::Progress) -> bool {
        is_dirty(progress_bar, &self.url, &self.full_path)
    }

    pub fn get_commit_tag(&self, progress_bar: &mut console::Progress) -> Option<Arc<str>> {
        get_commit_tag(progress_bar, &self.url, &self.full_path)
    }

    pub fn get_commit_hash(
        &self,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<Option<Arc<str>>> {
        get_commit_hash(progress_bar, &self.url, &self.full_path)
    }

    pub fn is_head_branch(&self, progress_bar: &mut console::Progress) -> bool {
        is_head_branch(progress_bar, &self.url, &self.full_path)
    }

    pub fn checkout(
        &self,
        progress_bar: &mut console::Progress,
        checkout: &Checkout,
    ) -> anyhow::Result<ResolveRevision> {
        let mut checkout_args = Vec::new();
        let revision = match checkout {
            Checkout::NewBranch(branch_name) => {
                checkout_args.push("switch".into());
                checkout_args.push("-c".into());
                checkout_args.push(branch_name.clone());
                // TODO: switch to a new branch
                ResolveRevision {
                    commit: branch_name.clone(),
                    latest_semver_tag: None,
                }
            }
            Checkout::Revision(revision) => {
                // if revision of the format "branch:semver" then get the tags on the branch
                let revision = self
                    .resolve_revision(progress_bar, revision)
                    .context(format_context!("failed to resolve revision"))?;

                checkout_args.push("checkout".into());
                checkout_args.push(revision.commit.clone());
                revision
            }
        };

        self.execute(progress_bar, checkout_args)
            .context(format_context!("while checking out {}", self.full_path))?;

        Ok(revision)
    }
}
