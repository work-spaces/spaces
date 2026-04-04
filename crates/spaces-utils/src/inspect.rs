use crate::{git, logger};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::collections::HashSet;
use std::sync::Arc;

pub fn logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "inspect".into())
}

pub struct GitTask {
    pub url: Arc<str>,
    pub rule_name: Arc<str>,
    pub spaces_key: Arc<str>,
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
    ) -> anyhow::Result<()> {
        let mut progress = console::Progress::new(console, "inspect-checkout".into(), None);
        let mut locks = Vec::new();
        let mut checkout_repo = None;
        for git_task in checkout_rules {
            let rule_name = git_task.rule_name.clone();
            let dir_name: Arc<str> = git_task.spaces_key.clone();
            if rule_name.starts_with("//checkout:") {
                checkout_repo = Some((dir_name.clone(), git_task.url.clone()));
            }

            let repo = git::Repository::new(git_task.url.clone(), dir_name.clone());
            if repo.is_dirty(&mut progress_bar) {
                if self.force {
                    logger(progress.console.clone()).warning(&format!(
                        "[{}] {} is dirty - checkout command may not be reproducible.",
                        git_task.url, rule_name
                    ));
                } else {
                    return Err(format_error!(
                        "[{}] {} is dirty - cannot inspect checkout with dirty repo. Commit and push changes.",
                        git_task.url,
                        rule_name
                    ));
                }
            }

            let commit_hash = repo
                .get_commit_hash(&mut progress_bar)
                .context(format_context!("Failed to get commit for {rule_name}"))?;

            if let Some(commit_description) = repo.get_commit_tag(&mut progress_bar).or(commit_hash)
            {
                locks.push((dir_name, commit_description))
            }
        }

        if let Some((checkout_dir_name, url)) = checkout_repo {
            let mut workspace_name: String = String::from(checkout_dir_name.as_ref());
            let mut command = format!(
                r#"spaces checkout-repo --url={url} \
  --rule-name={checkout_dir_name} \
"#
            );
            for (dir_name, commit) in locks.iter() {
                if dir_name == &checkout_dir_name {
                    command.push_str(&format!("  --rev={commit} \\"));
                    workspace_name.push_str(&format!("-{commit}"));
                } else {
                    command.push_str(&format!("  --lock={dir_name}={commit} \\"));
                }
                command.push('\n');
            }

            let workspace_name = workspace_name.replace("/", "-");
            command.push_str(&format!("  --name={workspace_name}\n"));

            printer.raw("\n")?;
            printer.raw(&command)?;
            printer.raw("\n")?;
            Ok(())
        } else {
            Err(format_error!(
                "Workspace was not created using spaces checkout-repo"
            ))
        }
    }
}
