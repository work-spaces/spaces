use crate::{git, labels};
use anyhow_source_location::format_error;
use std::collections::HashSet;
use std::sync::Arc;

pub struct GitTask {
    pub url: Arc<str>,
    pub rule_name: Arc<str>,
}

#[derive(Debug, Clone)]
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
}

impl Options {
    pub fn execute_inspect_checkout(
        &self,
        printer: &mut printer::Printer,
        checkout_rules: &[GitTask],
    ) -> anyhow::Result<()> {
        let mut progress = printer::MultiProgress::new(printer);
        let mut locks = Vec::new();
        let mut checkout_repo = None;
        for git_task in checkout_rules {
            let rule_name = git_task.rule_name.clone();
            let dir_name: Arc<str> = labels::get_rule_name_from_label(rule_name.as_ref()).into();
            if rule_name.starts_with("//checkout:") {
                checkout_repo = Some((dir_name.clone(), git_task.url.clone()));
            }
            let mut progress_bar = progress.add_progress(&git_task.url, None, None);
            progress_bar.set_message("checking if repo is dirty");

            let repo = git::Repository::new(git_task.url.clone(), dir_name.clone());
            if repo.is_dirty(&mut progress_bar) {
                return Err(format_error!(
                    "[{}] {} is dirty - cannot inspect checkout with dirty repo. Commit and push changes.",
                    git_task.url,
                    rule_name
                ));
            }
            if let Some(commit_description) = repo
                .get_commit_tag(&mut progress_bar)
                .or_else(|| repo.get_commit_short_hash(&mut progress_bar))
            {
                locks.push((dir_name, commit_description))
            }
        }

        if let Some((checkout_dir_name, url)) = checkout_repo {
            let mut workspace_name = checkout_dir_name.replace("/", "-");
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

            command.push_str(&format!("  --name={workspace_name}\n"));

            printer.raw("\n")?;
            printer.raw(&command)?;
            printer.raw("\n")?;
        }
        Ok(())
    }
}
