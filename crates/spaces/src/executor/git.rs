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
    fn execute_spaces_clone(
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
        let mut options = printer::ExecuteOptions {
            arguments: vec![
                "clone".to_string(),
                self.url.clone(),
                self.spaces_key.clone(),
            ],
            ..Default::default()
        };

        progress
            .execute_process("git", options.clone())
            .context(format_context!(
                "{name} - Failed to clone repository {}",
                self.spaces_key
            ))?;

        match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                options.arguments.push("switch".to_string());
                options.arguments.push(branch_name.clone());

                // TODO: switch to a new branch
            }
            git::Checkout::Revision(branch_name) => {
                options.arguments.push(branch_name.clone());
            }
        }

        progress
            .execute_process("git", options.clone())
            .context(format_context!(
                "{name} - Failed to clone repository {}",
                self.spaces_key
            ))?;

        Ok(())
    }

    pub fn execute(&self, _name: &str, progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        match self.clone {
            git::Clone::Spaces => {
                self.execute_spaces_clone(_name, progress)
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
