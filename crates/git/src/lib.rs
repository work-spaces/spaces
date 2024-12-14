use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckoutOption {
    Revision,
    NewBranch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Checkout {
    Revision(String),
    NewBranch(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub enum Clone {
    Default,
    Worktree,
    Shallow,
    Blobless,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repo {
    pub url: String,
    pub checkout: CheckoutOption,
    pub rev: String,
    pub clone: Option<Clone>,
    pub is_evaluate_spaces_modules: Option<bool>,
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
    pub commit: String,
    pub tag: Option<String>,
    pub description: String,
}

struct State {
    active_repos: HashSet<String>,
    log_directory: Option<String>,
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

    progress_bar.log(
        printer::Level::Debug,
        format!("Wait for git repo {url}").as_str(),
    );
    while !is_ready {
        {
            let mut state_lock = get_state().write().unwrap();
            let state = state_lock.deref_mut();

            if state.active_repos.contains(url) {
                is_ready = false;
            } else {
                state.active_repos.insert(url.to_string());
                is_ready = true;
            }
            log_file_path = state
                .log_directory
                .as_ref()
                .map(|e| format!("{e}/{log_file_name}"));
        }
        if !is_ready {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    let mut options = options.clone();

    options.log_file_path = log_file_path;
    options
        .environment
        .push(("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()));

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
) -> anyhow::Result<Option<String>> {
    let options = printer::ExecuteOptions {
        working_directory: Some(directory.to_string()),
        arguments: vec![
            "show".to_string(),
            "-s".to_string(),
            "--format=%H".to_string(),
        ],
        is_return_stdout: true,
        ..Default::default()
    };

    let commit_hash = execute_git_command(url, progress_bar, options).context(format_context!(
        "Failed to get commit hash from {directory}"
    ))?;

    let commit_hash = commit_hash.map(|e| e.trim().to_string());
    Ok(commit_hash)
}

pub fn is_branch(
    url: &str,
    directory: &str,
    ref_name: &str,
    progress_bar: &mut printer::MultiProgressBar,
) -> bool {
    let options = printer::ExecuteOptions {
        working_directory: Some(directory.to_string()),
        arguments: vec![
            "show-ref".to_string(),
            "--verify".to_string(),
            "--quiet".to_string(),
            format!("refs/heads/{}", ref_name).to_string(),
        ],
        ..Default::default()
    };
    execute_git_command(url, progress_bar, options).is_ok()
}

pub fn get_commit_tag(
    url: &str,
    directory: &str,
    progress_bar: &mut printer::MultiProgressBar,
) -> Option<String> {
    let options = printer::ExecuteOptions {
        working_directory: Some(directory.to_string()),
        arguments: vec![
            "describe".to_string(),
            "--exact-match".to_string(),
            "HEAD".to_string(),
        ],
        is_return_stdout: true,
        ..Default::default()
    };

    if let Ok(Some(stdout)) = execute_git_command(url, progress_bar, options) {
        let stdout_trimmed = stdout.trim();
        Some(stdout_trimmed.to_string())
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
        working_directory: Some(directory.to_string()),
        arguments: vec![
            "log".to_string(),
            "--oneline".to_string(),
            "--decorate=short".to_string(),
            "--no-color".to_string(),
            "--reverse".to_string(),
            "--pretty=format:\"%H;%D;%s\"".to_string(),
            branch.to_string(),
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
                let tag = if let Some(tag) = parts[1].strip_prefix("tag: ") {
                    Some(tag.to_string())
                } else {
                    None
                };

                log_entries.push(LogEntry {
                    commit: parts[0].to_string(),
                    tag: tag,
                    description: parts[2].to_string(),
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
    pub url: String,
    pub full_path: String,
    pub spaces_key: String,
    pub name_dot_git: String,
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

        let bare_store_path = format!("{bare_store_path}/{relative_bare_store_path}");

        std::fs::create_dir_all(&bare_store_path)
            .context(format_context!("failed to creat dir {bare_store_path}"))?;

        let full_path = format!("{}{}", bare_store_path, name_dot_git);

        if std::path::Path::new(&full_path).exists() {
            // config to fetch all heads/refs
            // This will grab newly created branches

            let options_git_config = printer::ExecuteOptions {
                working_directory: Some(full_path.to_string()),
                arguments: vec![
                    "config".to_string(),
                    "remote.origin.fetch".to_string(),
                    "refs/heads/*:refs/heads/*".to_string(),
                ],
                ..Default::default()
            };

            execute_git_command(url, progress_bar, options_git_config)
                .context(format_context!("while setting git options"))?;
        } else {
            options.working_directory = Some(bare_store_path.clone());

            std::fs::create_dir_all(&bare_store_path)
                .context(format_context!("failed to create dir {bare_store_path}"))?;

            options.arguments = vec![
                "clone".to_string(),
                "--bare".to_string(),
                "--filter=blob:none".to_string(),
                url.to_string(),
            ];

            execute_git_command(url, progress_bar, options)
                .context(format_context!("while creating bare repo"))?;

            let options_git_config_auto_push = printer::ExecuteOptions {
                working_directory: Some(full_path.to_string()),
                arguments: vec![
                    "config".to_string(),
                    "--add".to_string(),
                    "--bool".to_string(),
                    "push.autoSetupRemote".to_string(),
                    "true".to_string(),
                ],
                ..Default::default()
            };

            execute_git_command(url, progress_bar, options_git_config_auto_push)
                .context(format_context!("while configuring auto-push"))?;
        }

        Ok(Self {
            url: url.to_owned(),
            full_path,
            spaces_key: spaces_key.to_owned(),
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

    fn url_to_relative_path_and_name(url: &str) -> anyhow::Result<(String, String)> {
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

        Ok((bare_store, repo_name))
    }

    #[allow(dead_code)]
    pub fn get_workspace_name_from_url(url: &str) -> anyhow::Result<String> {
        let (_, repo_name) = Self::url_to_relative_path_and_name(url)?;

        repo_name
            .strip_suffix(".git")
            .ok_or(format_error!(
                "Failed to extract a workspace name from  {url}",
            ))
            .map(|e| e.to_string())
    }
}

pub struct Worktree {
    pub full_path: String,
    pub url: String,
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
        options.arguments = vec!["worktree".to_string(), "prune".to_string()];

        execute_git_command(&repository.url, progress_bar, options.clone())
            .context(format_context!("while pruning worktree"))?;

        let full_path = format!("{}/{}", path, repository.spaces_key);
        if !std::path::Path::new(&full_path).exists() {
            options.arguments = vec![
                "worktree".to_string(),
                "add".to_string(),
                "--detach".to_string(),
                full_path.to_string(),
            ];

            execute_git_command(&repository.url, progress_bar, options)
                .context(format_context!("while adding detached worktree"))?;
        }

        Ok(Self {
            full_path,
            url: repository.url.clone(),
        })
    }

    pub fn get_spaces_star(&self) -> anyhow::Result<Option<String>> {
        //check for spaces.star in full_path and return Some string if the file exists
        let star_file = format!("{}/spaces.star", self.full_path);
        if std::path::Path::new(&star_file).exists() {
            Ok(Some(star_file))
        } else {
            Ok(None)
        }
    }

    pub fn checkout(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
        revision: &str,
    ) -> anyhow::Result<()> {
        let mut options = printer::ExecuteOptions {
            working_directory: Some(self.full_path.clone()),
            ..Default::default()
        };
        options.arguments = vec!["fetch".to_string(), "origin".to_string(), revision.to_owned()];

        execute_git_command(&self.url, progress_bar, options.clone())
            .context(format_context!("while fetching existing bare repository"))?;

        options.arguments = vec![
            "checkout".to_string(),
            "--detach".to_string(),
            revision.to_owned(),
        ];

        execute_git_command(&self.url, progress_bar, options)
            .context(format_context!("checkout {revision:?}"))?;

        Ok(())
    }

    pub fn checkout_detached_head(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let options = printer::ExecuteOptions {
            working_directory: Some(self.full_path.clone()),
            arguments: vec![
                "checkout".to_string(),
                "--detach".to_string(),
                "HEAD".to_string(),
            ],
            ..Default::default()
        };

        execute_git_command(&self.url, progress_bar, options)
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

        let options = printer::ExecuteOptions {
            working_directory: Some(self.full_path.clone()),
            arguments: vec![
                "switch".to_string(),
                "-c".to_string(),
                dev_branch.to_string(),
            ],
            ..Default::default()
        };

        execute_git_command(&self.url, progress_bar, options)
            .context(format_context!("switch new branch"))?;

        Ok(())
    }
}
