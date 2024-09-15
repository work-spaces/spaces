use crate::info;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Git {
    pub url: String,
    pub spaces_key: String,
    pub worktree_path: String,
    pub checkout: git::Checkout,
}

impl Git {

    pub fn execute(
        &self,
        _name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {

        let bare_repo = git::BareRepository::new(
            &mut progress,
            &info::get_store_path().as_str(),
            &self.spaces_key,
            &self.url,
        )
        .context(format_context!("Failed to create bare repository"))?;

        let worktree = bare_repo
            .add_worktree(&mut progress, &self.worktree_path)
            .context(format_context!("Failed to add worktree"))?;

        match &self.checkout {
            git::Checkout::NewBranch(branch_name) => {
                worktree
                    .switch_new_branch(&mut progress, branch_name, &self.checkout)
                    .context(format_context!("Failed to checkout new branch"))?;
            }
            _ => {
                worktree
                    .checkout(&mut progress, &self.checkout)
                    .context(format_context!("Failed to switch branch"))?;
            }
        }

        Ok(())
    }
}
