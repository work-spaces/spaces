use crate::{
    context,
    context::{anyhow_error, format_error_context},
    manifest::{self, Dependency},
};
use anyhow::Context;
use serde::Serialize;

#[derive(Clone, Serialize, Debug)]
pub struct BareRepository {
    pub url: String,
    pub full_path: String,
    pub spaces_key: String,
    pub name_dot_git: String,
}

impl BareRepository {
    fn configure_repository(
        progress_bar: &mut printer::MultiProgressBar,
        full_path: &str,
    ) -> anyhow::Result<()> {
        let options_git_config = printer::ExecuteOptions {
            working_directory: Some(full_path.to_string()),
            arguments: vec![
                "config".to_string(),
                "remote.origin.fetch".to_string(),
                "\"+refs/heads/*:refs/remotes/origin/*\"".to_string(),
            ],
            ..Default::default()
        };

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

        progress_bar
            .execute_process("git", &options_git_config)
            .with_context(|| {
                format_error_context!(
                    "failed to run {}",
                    options_git_config.get_full_command_in_working_directory("git")
                )
            })?;

        progress_bar
            .execute_process("git", &options_git_config_auto_push)
            .with_context(|| {
                format_error_context!(
                    "failed to run {}",
                    options_git_config_auto_push.get_full_command_in_working_directory("git")
                )
            })?;
        Ok(())
    }

    pub fn new(
        context: std::sync::Arc<context::Context>,
        progress_bar: &mut printer::MultiProgressBar,
        spaces_key: &str,
        url: &str,
    ) -> anyhow::Result<Self> {
        let mut options = printer::ExecuteOptions::default();

        let (relative_bare_store_path, name_dot_git) = Self::url_to_relative_path_and_name(url)
            .with_context(|| format_error_context!("Failed to parse {spaces_key} url: {url}"))?;

        let bare_store_path = context.get_bare_store_path(relative_bare_store_path.as_str());

        if !context.is_dry_run {
            std::fs::create_dir_all(&bare_store_path)?;
        }

        let full_path = format!("{}{}", bare_store_path, name_dot_git);

        if std::path::Path::new(&full_path).exists() {
            // config to fetch all heads/refs
            // This will grab newly created branches

            Self::configure_repository(progress_bar, full_path.as_str()).with_context(|| {
                format_error_context!("failed to configure {full_path} before fetch")
            })?;

            options.working_directory = Some(full_path.clone());
            options.arguments = vec!["fetch".to_string()];

            progress_bar
                .execute_process("git", &options)
                .with_context(|| {
                    format_error_context!(
                        "failed to run {}",
                        options.get_full_command_in_working_directory("git")
                    )
                })?;
        } else {
            options.working_directory = Some(bare_store_path.clone());
            if !context.is_dry_run {
                std::fs::create_dir_all(&bare_store_path)?;
            }
            options.arguments = vec![
                "clone".to_string(),
                "--bare".to_string(),
                "--filter=blob:none".to_string(),
                url.to_string(),
            ];

            Self::configure_repository(progress_bar, full_path.as_str()).with_context(|| {
                format_error_context!("failed to configure {full_path} after bare clone")
            })?;
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
            .with_context(|| format_error_context!("Adding working to {} at {path}", self.url))?;
        Ok(result)
    }

    fn url_to_relative_path_and_name(url: &str) -> anyhow::Result<(String, String)> {
        let repo_url = url::Url::parse(url)
            .with_context(|| format_error_context!("Failed to parse bare store url {url}"))?;

        let host = repo_url
            .host_str()
            .ok_or(anyhow_error!("No host found in url {}", url))?;
        let scheme = repo_url.scheme();
        let path_segments = repo_url
            .path_segments()
            .ok_or(anyhow_error!("No path found in url {}", url))?;

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

    pub fn get_workspace_name_from_url(url: &str) -> anyhow::Result<String> {
        let (_, repo_name) = Self::url_to_relative_path_and_name(url)?;

        repo_name.strip_suffix(".git").ok_or(anyhow_error!(
            "Failed to extract a workspace name from  {url}",
        )).map(|e| e.to_string())

    }


}

pub struct Worktree {
    pub full_path: String,
    pub repository: BareRepository,
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
            return Err(anyhow_error!(
                "Path to worktree must be an absolute path: {}",
                path
            ));
        }

        if !context.is_dry_run {
            std::fs::create_dir_all(path)?;
        }

        options.working_directory = Some(repository.full_path.clone());
        options.arguments = vec!["worktree".to_string(), "prune".to_string()];
        progress_bar
            .execute_process("git", &options)
            .with_context(|| {
                format_error_context!(
                    "failed to run {}",
                    options.get_full_command_in_working_directory("git")
                )
            })?;

        let full_path = format!("{}/{}", path, repository.spaces_key);
        if !context.is_dry_run && !std::path::Path::new(&full_path).exists() {
            options.arguments = vec![
                "worktree".to_string(),
                "add".to_string(),
                "--detach".to_string(),
                full_path.to_string(),
            ];

            progress_bar
                .execute_process("git", &options)
                .with_context(|| {
                    format_error_context!(
                        "failed to run {}",
                        options.get_full_command_in_working_directory("git")
                    )
                })?;
        }

        Ok(Self {
            full_path,
            repository: repository.clone(),
        })
    }

    pub fn get_deps(&self) -> anyhow::Result<Option<manifest::Deps>> {
        manifest::Deps::new(&self.full_path)
    }

    pub fn checkout(
        &self,
        _context: std::sync::Arc<context::Context>,
        progress_bar: &mut printer::MultiProgressBar,
        dependency: &manifest::Dependency,
    ) -> anyhow::Result<()> {
        let mut options = printer::ExecuteOptions {
            working_directory: Some(self.full_path.clone()),
            ..Default::default()
        };

        let checkout = dependency.get_checkout()?;
        match checkout {
            manifest::Checkout::ReadOnly(value) => {
                options.arguments = vec!["checkout".to_string(), value.clone()];
            }
            manifest::Checkout::ReadOnlyBranch(value) => {
                options.arguments = vec!["checkout".to_string(), value.clone()];
            }
            manifest::Checkout::Develop(value) => {
                return Err(anyhow_error!(
                    "Internal Error: cannot call checkout() with `Checkout::Develop` {}",
                    value
                ));
            }
            manifest::Checkout::Artifact(artifact) => {
                return Err(anyhow_error!(
                    "Artifact checkout is not yet supported {}",
                    artifact
                ));
            }
        }

        progress_bar
            .execute_process("git", &options)
            .with_context(|| {
                format_error_context!(
                    "failed to run {}",
                    options.get_full_command_in_working_directory("git")
                )
            })?;
        Ok(())
    }

    pub fn checkout_detached_head(
        &self,
        _context: std::sync::Arc<context::Context>,
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
        progress_bar
            .execute_process("git", &options)
            .with_context(|| {
                format_error_context!(
                    "failed to run {}",
                    options.get_full_command_in_working_directory("git")
                )
            })?;

        Ok(())
    }

    pub fn switch_new_branch(
        &self,
        context: std::sync::Arc<context::Context>,
        progress_bar: &mut printer::MultiProgressBar,
        dependency: &Dependency,
    ) -> anyhow::Result<()> {
        if let (Some(checkout), Some(dev)) = (dependency.checkout.as_ref(), dependency.dev.as_ref())
        {
            let mut original_checkout_dependency = dependency.clone();
            original_checkout_dependency.checkout = None;

            self.checkout(context, progress_bar, &original_checkout_dependency)?;

            if *checkout == manifest::CheckoutOption::Develop {
                let mut options = printer::ExecuteOptions {
                    working_directory: Some(self.full_path.clone()),
                    arguments: vec!["pull".to_string()],
                    ..Default::default()
                };
                progress_bar
                    .execute_process("git", &options)
                    .with_context(|| {
                        format_error_context!(
                            "failed to run {}",
                            options.get_full_command_in_working_directory("git")
                        )
                    })?;

                options.arguments = vec!["switch".to_string(), "-c".to_string(), dev.clone()];

                progress_bar
                    .execute_process("git", &options)
                    .with_context(|| {
                        format_error_context!(
                            "failed to run {}",
                            options.get_full_command_in_working_directory("git")
                        )
                    })?;
            } else {
                return Err(anyhow_error!(
                    "No `dev` found for dependency {}",
                    dependency.git
                ));
            }
        } else {
            if dependency.checkout.is_none() {
                return Err(anyhow_error!(
                    "No `checkout` found for dependency {}",
                    dependency.git
                ));
            }

            return Err(anyhow_error!(
                "No `dev` found for dependency {}",
                dependency.git
            ));
        }

        Ok(())
    }
}
