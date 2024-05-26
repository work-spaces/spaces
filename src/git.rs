use crate::{
    config::Printer,
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
    pub fn new(printer: &mut Printer, spaces_key: &str, url: &str) -> anyhow::Result<(Self, Vec<printer::ExecuteLater>)> {
        let mut options = printer::ExecuteOptions::default();

        let (relative_bare_store_path, name_dot_git) = Self::url_to_relative_path_and_name(url)
            .with_context(|| format!("Failed to parse {spaces_key} url: {url}"))?;

        let bare_store_path = printer
            .context()
            .get_bare_store_path(relative_bare_store_path.as_str());

        if !printer.is_dry_run {
            std::fs::create_dir_all(&bare_store_path)?;
        }

        let full_path = format!("{}{}", bare_store_path, name_dot_git);

        if std::path::Path::new(&full_path).exists() {
            options.working_directory = Some(full_path.clone());
            options.arguments = vec!["fetch".to_string()];
        } else {
            options.working_directory = Some(bare_store_path.clone());
            printer.info("barestore", &bare_store_path)?;
            if !printer.is_dry_run {
                std::fs::create_dir_all(&bare_store_path)?;
            }
            options.arguments = vec![
                "clone".to_string(),
                "--bare".to_string(),
                "--filter=blob:none".to_string(),
                url.to_string(),
            ];
        }

        //printer.execute_process("git", &options)?;

        let execute_later = vec![printer::ExecuteLater::new("git", options)];

        Ok((Self {
            url: url.to_owned(),
            full_path,
            spaces_key: spaces_key.to_owned(),
            name_dot_git,
        }, execute_later))
        
    }

    pub fn add_worktree(&self, printer: &mut Printer, path: &str) -> anyhow::Result<(Worktree, Vec<printer::ExecuteLater>)> {
        let result = Worktree::new(printer, self, path)
            .with_context(|| format!("Adding working to {} at {path}", self.url))?;
        Ok(result)
    }

    fn url_to_relative_path_and_name(url: &str) -> anyhow::Result<(String, String)> {
        let repo_url = url::Url::parse(url)
            .with_context(|| format!("Failed to parse bare store url {url}"))?;

        let host = repo_url
            .host_str()
            .ok_or(anyhow::anyhow!("No host found in url {}", url))?;
        let scheme = repo_url.scheme();
        let path_segments = repo_url
            .path_segments()
            .ok_or(anyhow::anyhow!("No path found in url {}", url))?;

        let mut path = String::new();
        let mut repo_name = String::new();
        let count = path_segments.clone().count();
        if count > 1 {
            path.push_str("/");
            for (index, segment) in path_segments.enumerate() {
                if index == count - 1 {
                    repo_name = segment.to_string();
                    break;
                }
                path.push_str(segment);
                path.push_str("/");
            }
        } else {
            path.push_str("/");
        }

        let bare_store = format!("{scheme}/{host}{path}");
        repo_name.push_str(".git");

        Ok((bare_store, repo_name))
    }
}

pub struct Worktree {
    pub full_path: String,
    pub repository: BareRepository,
}

impl Worktree {
    fn new(printer: &mut Printer, repository: &BareRepository, path: &str) -> anyhow::Result<(Self, Vec<printer::ExecuteLater>)> {
        let mut options = printer::ExecuteOptions::default();

        if !std::path::Path::new(&path).is_absolute() {
            return Err(anyhow::anyhow!(
                "Path to worktree must be an absolute path: {}",
                path
            ));
        }

        if !printer.is_dry_run {
            std::fs::create_dir_all(&path)?;
        }

        let mut execute_later = Vec::new();

        options.working_directory = Some(repository.full_path.clone());
        options.arguments = vec!["worktree".to_string(), "prune".to_string()];
        //printer.execute_process("git", &options)?;

        execute_later.push(printer::ExecuteLater::new("git", options.clone()));

        let full_path = format!("{}/{}", path, repository.spaces_key);
        if !printer.is_dry_run && !std::path::Path::new(&full_path).exists() {
            options.arguments = vec![
                "worktree".to_string(),
                "add".to_string(),
                "--detach".to_string(),
                full_path.to_string(),
            ];
            //printer.execute_process("git", &options)?;
            execute_later.push(printer::ExecuteLater::new("git", options.clone()));
        } else {
            printer.info("worktree", &full_path)?;
        }

        Ok((Self {
            full_path,
            repository: repository.clone(),
        }, execute_later))
    }

    pub fn get_deps(&self) -> anyhow::Result<Option<manifest::Deps>> {
        manifest::Deps::new(&self.full_path)
    }

    pub fn checkout(
        &self,
        printer: &mut Printer,
        dependency: &manifest::Dependency,
    ) -> anyhow::Result<Vec<printer::ExecuteLater>> {
        let mut options = printer::ExecuteOptions::default();
        let mut execute_later = Vec::new();

        options.working_directory = Some(self.full_path.clone());

        let checkout = dependency.get_checkout()?;
        match checkout {
            manifest::Checkout::ReadOnly(value) => {
                options.arguments = vec!["checkout".to_string(), value.clone()];
            }
            manifest::Checkout::ReadOnlyBranch(value) => {
                options.arguments = vec!["checkout".to_string(), value.clone()];
            }
            manifest::Checkout::Develop(value) => {
                return Err(anyhow::anyhow!(
                    "Internal Error: cannot call checkout() with `Checkout::Develop` {}",
                    value
                ));
            }
            manifest::Checkout::Artifact(artifact) => {
                return Err(anyhow::anyhow!(
                    "Artifact checkout is not yet supported {}",
                    artifact
                ));
            }
        }

        //printer.execute_process("git", &options)?;
        execute_later.push(printer::ExecuteLater::new("git", options));
        Ok(execute_later)
    }

    pub fn checkout_detached_head(&self, printer: &mut Printer) -> anyhow::Result<Vec<printer::ExecuteLater>> {
        let mut options = printer::ExecuteOptions::default();

        options.working_directory = Some(self.full_path.clone());
        options.arguments = vec![
            "checkout".to_string(),
            "--detach".to_string(),
            "HEAD".to_string(),
        ];
        Ok(vec![printer::ExecuteLater::new("git", options)])
    }

    pub fn switch_new_branch(
        &self,
        printer: &mut Printer,
        dependency: &Dependency,
    ) -> anyhow::Result<Vec<printer::ExecuteLater>> {
        let mut execute_later = Vec::new();
        if let (Some(checkout), Some(dev)) = (dependency.checkout.as_ref(), dependency.dev.as_ref())
        {
            let mut original_checkout_dependency = dependency.clone();
            original_checkout_dependency.checkout = None;
            execute_later.extend(self.checkout(printer, &original_checkout_dependency)?);


            if *checkout == manifest::CheckoutOption::Develop {
                let mut options = printer::ExecuteOptions::default();

                options.working_directory = Some(self.full_path.clone());
                options.arguments = vec!["pull".to_string()];
                //printer.execute_process("git", &options)?;
                execute_later.push(printer::ExecuteLater::new("git", options.clone()));

                options.arguments = vec!["switch".to_string(), "-c".to_string(), dev.clone()];

                //printer.execute_process("git", &options)?;
                execute_later.push(printer::ExecuteLater::new("git", options));
            } else {
                return Err(anyhow::anyhow!(
                    "No `dev` found for dependency {}",
                    dependency.git
                ));
            }
        } else {
            if dependency.checkout.is_none() {
                return Err(anyhow::anyhow!(
                    "No `checkout` found for dependency {}",
                    dependency.git
                ));
            }

            return Err(anyhow::anyhow!(
                "No `dev` found for dependency {}",
                dependency.git
            ));
        }

        Ok(execute_later)
    }
}
