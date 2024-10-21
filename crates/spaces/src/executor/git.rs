use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Git {
    pub url: String,
    pub spaces_key: String,
    pub worktree_path: String,
    pub checkout: git::Checkout,
    pub clone: git::Clone,
}

impl Git {
    fn execute_worktree_clone(
        &self,
        name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let bare_repo = git::BareRepository::new(
            &mut progress,
            workspace::get_store_path().as_str(),
            &self.spaces_key,
            &self.url,
        )
        .context(format_context!("Failed to create bare repository"))?;

        let worktree = bare_repo
            .add_worktree(&mut progress, &self.worktree_path)
            .context(format_context!("{name} - Failed to add worktree"))?;

        match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                worktree
                    .switch_new_branch(&mut progress, branch_name, &self.checkout)
                    .context(format_context!("{name} - Failed to checkout new branch"))?;
            }
            _ => {
                worktree
                    .checkout(&mut progress, &self.checkout)
                    .context(format_context!("{name} - Failed to switch branch"))?;
            }
        }

        Ok(())
    }

    fn execute_default_clone(
        &self,
        name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let clone_options = printer::ExecuteOptions {
            arguments: vec![
                "clone".to_string(),
                self.url.clone(),
                self.spaces_key.clone(),
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

        let mut checkout_options = printer::ExecuteOptions {
            working_directory: Some(self.spaces_key.clone()),
            ..Default::default()
        };

        match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                checkout_options.arguments.push("switch".to_string());
                checkout_options.arguments.push(branch_name.clone());

                // TODO: switch to a new branch
            }
            git::Checkout::Revision(branch_name) => {
                checkout_options.arguments.push("checkout".to_string());
                checkout_options.arguments.push(branch_name.clone());
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

    pub fn execute(&self, _name: &str, progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        match self.clone {
            git::Clone::Worktree => {
                self.execute_worktree_clone(_name, progress)
                    .context(format_context!("spaces clone failed"))?;
            }
            git::Clone::Default => {
                self.execute_default_clone(_name, progress)
                    .context(format_context!("default clone failed"))?;
            }
        }
        Ok(())
    }
}
