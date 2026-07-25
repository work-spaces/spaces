use crate::{git, logger};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::collections::HashSet;
use std::sync::Arc;

pub fn logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "inspect".into())
}

/// POSIX-shell quote a value so it can be safely interpolated into a generated
/// command line. Values that only contain a conservative set of "safe"
/// characters are returned unchanged; everything else is wrapped in single
/// quotes (with any embedded single quotes escaped as `'\''`).
fn shell_quote(value: &str) -> std::borrow::Cow<'_, str> {
    let is_safe = !value.is_empty()
        && value.bytes().all(|b| {
            matches!(b,
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
                | b'_' | b'-' | b'.' | b'/' | b':' | b'=' | b'@' | b'+' | b','
            )
        });
    if is_safe {
        std::borrow::Cow::Borrowed(value)
    } else {
        let mut quoted = String::with_capacity(value.len() + 2);
        quoted.push('\'');
        for ch in value.chars() {
            if ch == '\'' {
                quoted.push_str("'\\''");
            } else {
                quoted.push(ch);
            }
        }
        quoted.push('\'');
        std::borrow::Cow::Owned(quoted)
    }
}

#[derive(Debug)]
pub struct GitTask {
    pub url: Arc<str>,
    pub spaces_key: Arc<str>,
    pub is_checkout_repo: bool,
}

#[derive(Debug, Clone)]
pub struct RepoLock {
    pub url: Arc<str>,
    pub repo_path: Arc<str>,
    pub commit: Arc<str>,
    pub is_checkout_repo: bool,
}

pub fn select_lock_value(
    commit_tag: Option<Arc<str>>,
    commit_hash: Option<Arc<str>>,
    allow_tags: bool,
) -> Option<Arc<str>> {
    if allow_tags {
        commit_tag.or(commit_hash)
    } else {
        commit_hash
    }
}

pub fn collect_repo_locks(
    console: &console::Console,
    checkout_git_tasks: &[GitTask],
    force_dirty: bool,
    allow_tags: bool,
    query_name: &str,
) -> anyhow::Result<Vec<RepoLock>> {
    let count = checkout_git_tasks.len();
    let progress_name = query_name.replace(' ', "-");
    let mut progress = console::Progress::new(
        console.clone(),
        progress_name.as_str(),
        Some(count as u64),
        None,
    );
    let mut locks = Vec::new();

    for git_task in checkout_git_tasks {
        let dir_name = git_task.spaces_key.clone();
        let repo_name = format!("//{dir_name}");
        progress.set_message(format!("{repo_name} - checking").as_str());

        let mut repo_progress =
            console::Progress::new(console.clone(), repo_name.as_str(), None, None);
        repo_progress.set_message("checking local/remote status");

        let repo = git::Repository::new(git_task.url.clone(), dir_name.clone());
        if repo.is_dirty(&mut repo_progress, git::IgnoreSubmodules::No) {
            if force_dirty {
                logger(repo_progress.console.clone()).warning(&format!(
                    "[{}] {} is dirty\n  {query_name} may not be reproducible.",
                    git_task.url, repo_name
                ));
            } else {
                return Err(format_error!(
                    "[{}] {} is dirty\n  Cannot run {query_name} with dirty repo.\n  Commit and push changes.",
                    git_task.url,
                    repo_name
                ));
            }
        }

        match repo.has_local_commits_not_on_remotes(&mut progress) {
            Ok(true) => {
                return Err(format_error!(
                    "[{}] {} has local commits.\n  Fetch and push commits before running {query_name}.",
                    git_task.url,
                    repo_name
                ));
            }
            Ok(false) => {}
            Err(error) => {
                return Err(error).context(format_context!(
                    "[{}] {} failed while checking local commits.",
                    git_task.url,
                    repo_name
                ));
            }
        }

        let commit_hash = repo
            .get_commit_hash(&mut progress)
            .context(format_context!("Failed to get commit for {repo_name}"))?;

        let commit_tag = if allow_tags {
            repo.get_commit_tag(&mut progress)
        } else {
            None
        };

        let commit = select_lock_value(commit_tag, commit_hash, allow_tags).ok_or_else(|| {
            format_error!(
                "[{}] {} failed to resolve the current commit.",
                git_task.url,
                repo_name
            )
        })?;

        locks.push(RepoLock {
            url: git_task.url.clone(),
            repo_path: dir_name,
            commit,
            is_checkout_repo: git_task.is_checkout_repo,
        });
    }

    locks.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));

    Ok(locks)
}

pub fn build_checkout_command(
    locks: &[RepoLock],
    assign_from_arg_env: &[(Arc<str>, Arc<str>)],
    command_line_store: &[(Arc<str>, Arc<str>)],
) -> anyhow::Result<String> {
    let workspace_dir_name: Arc<str> = std::env::current_dir()
        .context(format_context!("Failed to get current directory"))?
        .file_name()
        .ok_or_else(|| format_error!("Failed to derive workspace directory name"))?
        .to_string_lossy()
        .to_string()
        .into();

    let checkout_repo = locks
        .iter()
        .find(|entry| entry.is_checkout_repo)
        .ok_or_else(|| format_error!("Failed to find checkout repo from checkout rules"))?;

    let mut workspace_name: String = workspace_dir_name.to_string();
    let mut command = format!(
        "spaces checkout-repo --url={url} \\\n  --rule-name={rule_name} \\\n",
        url = shell_quote(checkout_repo.url.as_ref()),
        rule_name = shell_quote(checkout_repo.repo_path.as_ref()),
    );

    for lock in locks {
        if lock.is_checkout_repo {
            command.push_str(&format!("  --rev={} \\", shell_quote(lock.commit.as_ref())));
            workspace_name.push_str(&format!("-{}", lock.commit));
        } else {
            command.push_str(&format!(
                "  --lock={} \\",
                shell_quote(&format!("{}={}", lock.repo_path, lock.commit))
            ));
        }
        command.push('\n');
    }

    for (name, value) in assign_from_arg_env {
        command.push_str(&format!(
            "  --env={} \\\n",
            shell_quote(&format!("{name}={value}"))
        ));
    }
    for (key, value) in command_line_store {
        command.push_str(&format!(
            "  --store={} \\\n",
            shell_quote(&format!("{key}={value}"))
        ));
    }

    let workspace_name = workspace_name.replace('/', "-");
    command.push_str(&format!("  --name={}\n", shell_quote(&workspace_name)));

    Ok(command)
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub target: Option<Arc<str>>,
    pub filter_globs: HashSet<Arc<str>>,
    pub has_help: bool,
    pub markdown: Option<Arc<str>>,
    pub stardoc: Option<Arc<str>>,
    pub fuzzy: Option<Arc<str>>,
    pub details: bool,
    pub json: bool,
    pub checkout: bool,
    pub force: bool,
}

impl Options {
    pub fn execute_inspect_checkout(
        &self,
        console: console::Console,
        checkout_rules: &[GitTask],
        assign_from_arg_env: &[(Arc<str>, Arc<str>)],
        command_line_store: &[(Arc<str>, Arc<str>)],
    ) -> anyhow::Result<()> {
        let locks =
            collect_repo_locks(&console, checkout_rules, self.force, true, "query checkout")?;

        let command = build_checkout_command(&locks, assign_from_arg_env, command_line_store)?;

        console.raw("\n")?;
        console.raw(&command)?;
        console.raw("\n")?;
        Ok(())
    }
}
