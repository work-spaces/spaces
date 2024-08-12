use std::ops::DerefMut;

use crate::{
    context,
    manifest::{self, Dependency},
};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::Serialize;

fn execute_git_command(
    context: std::sync::Arc<context::Context>,
    url: &str,
    progress_bar: &mut printer::MultiProgressBar,
    options: printer::ExecuteOptions,
) -> anyhow::Result<()> {
    let mut is_ready = false;
    while !is_ready {
        {
            let mut active_repos_lock = context.active_repository.lock().unwrap();
            let active_repos = active_repos_lock.deref_mut();
            if active_repos.contains(url) {
                is_ready = false;
            } else {
                active_repos.insert(url.to_string());
                is_ready = true;
            }
        }
        if !is_ready {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    let mut options = options.clone();

    let log_file_name = format!(
        "git_{}.log",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    let log_file_path = format!("{}/{log_file_name}", context.get_log_directory());

    options.log_file_path = Some(log_file_path);

    let full_command = options.get_full_command_in_working_directory("git");
    progress_bar
        .execute_process("git", options)
        .context(format_context!("{full_command}"))?;

    {
        let mut active_repos_lock = context.active_repository.lock().unwrap();
        active_repos_lock.remove(url);
    }

    Ok(())
}

#[derive(Clone, Serialize, Debug)]
pub struct BareRepository {
    pub url: String,
    pub full_path: String,
    pub spaces_key: String,
    pub name_dot_git: String,
}

impl BareRepository {
    pub fn new(
        context: std::sync::Arc<context::Context>,
        progress_bar: &mut printer::MultiProgressBar,
        spaces_key: &str,
        url: &str,
    ) -> anyhow::Result<Self> {
        let mut options = printer::ExecuteOptions::default();

        let (relative_bare_store_path, name_dot_git) = Self::url_to_relative_path_and_name(url)
            .context(format_context!("Failed to parse {spaces_key} url: {url}"))?;

        let bare_store_path = context.get_bare_store_path(relative_bare_store_path.as_str());

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
                    "+refs/heads/*:refs/remotes/origin/*".to_string(),
                ],
                ..Default::default()
            };

            execute_git_command(context.clone(), url, progress_bar, options_git_config)
                .context(format_context!(""))?;

            options.working_directory = Some(full_path.clone());
            options.arguments = vec!["fetch".to_string()];

            execute_git_command(context.clone(), url, progress_bar, options)
                .context(format_context!(""))?;
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

            execute_git_command(context.clone(), url, progress_bar, options)
                .context(format_context!(""))?;

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

            execute_git_command(
                context.clone(),
                url,
                progress_bar,
                options_git_config_auto_push,
            )
            .context(format_context!(""))?;
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
        context: std::sync::Arc<context::Context>,
        progress_bar: &mut printer::MultiProgressBar,
        path: &str,
    ) -> anyhow::Result<Worktree> {
        let result = Worktree::new(context, progress_bar, self, path)
            .context(format_context!("Adding working to {} at {path}", self.url))?;
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
        context: std::sync::Arc<context::Context>,
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

        execute_git_command(
            context.clone(),
            &repository.url,
            progress_bar,
            options.clone(),
        )
        .context(format_context!(""))?;

        let full_path = format!("{}/{}", path, repository.spaces_key);
        if !std::path::Path::new(&full_path).exists() {
            options.arguments = vec![
                "worktree".to_string(),
                "add".to_string(),
                "--detach".to_string(),
                full_path.to_string(),
            ];

            execute_git_command(context.clone(), &repository.url, progress_bar, options)
                .context(format_context!(""))?;
        }

        Ok(Self {
            full_path,
            url: repository.url.clone(),
        })
    }

    pub fn get_deps(&self) -> anyhow::Result<Option<manifest::Deps>> {
        manifest::Deps::new(&self.full_path)
    }

    pub fn checkout(
        &self,
        context: std::sync::Arc<context::Context>,
        progress_bar: &mut printer::MultiProgressBar,
        dependency: &manifest::Dependency,
    ) -> anyhow::Result<manifest::Checkout> {
        let mut options = printer::ExecuteOptions {
            working_directory: Some(self.full_path.clone()),
            ..Default::default()
        };

        options.arguments = vec![
            "fetch".to_string(),
            "origin".to_string(),
            "+refs/heads/*:refs/heads/*".to_string(),
        ];

        execute_git_command(context.clone(), &self.url, progress_bar, options.clone())
            .context(format_context!("fetching {}", self.url))?;

        let checkout = dependency.get_checkout().context(format_context!(
            "failed to get checkout type for {dependency:?}"
        ))?;

        match &checkout {
            manifest::Checkout::Revision(value) => {
                options.arguments = vec!["checkout".to_string(), value.clone()];
            }
            manifest::Checkout::BranchHead(value) => {
                options.arguments = vec!["checkout".to_string(), value.clone()];
            }
            manifest::Checkout::NewBranch(value) => {
                options.arguments = vec!["checkout".to_string(), value.clone()];
            }
            manifest::Checkout::Artifact(artifact) => {
                return Err(format_error!(
                    "Artifact checkout is not yet supported {}",
                    artifact
                ));
            }
        }

        execute_git_command(context.clone(), &self.url, progress_bar, options)
            .context(format_context!("checkout {checkout:?}"))?;

        Ok(checkout)
    }

    pub fn checkout_detached_head(
        &self,
        context: std::sync::Arc<context::Context>,
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

        execute_git_command(context.clone(), &self.url, progress_bar, options)
            .context(format_context!("detech head"))?;

        Ok(())
    }

    pub fn switch_new_branch(
        &self,
        context: std::sync::Arc<context::Context>,
        branch_template: &str,
        progress_bar: &mut printer::MultiProgressBar,
        dependency: &Dependency,
    ) -> anyhow::Result<()> {
        self.checkout(context.clone(), progress_bar, dependency)
            .context(format_context!("switch new branch {}", dependency.git))?;

        if dependency.checkout == manifest::CheckoutOption::NewBranch {
            let dev_branch = context
                .template_model
                .render_template_string(branch_template)
                .context(format_context!("{branch_template}"))?;

            let options = printer::ExecuteOptions {
                working_directory: Some(self.full_path.clone()),
                arguments: vec!["switch".to_string(), "-c".to_string(), dev_branch.clone()],
                ..Default::default()
            };

            execute_git_command(context.clone(), &self.url, progress_bar, options)
                .context(format_context!("switch new branch"))?;
        }

        Ok(())
    }
}
