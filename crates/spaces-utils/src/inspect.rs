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
        assign_from_arg_env: &[(Arc<str>, Arc<str>)],
        command_line_store: &[(Arc<str>, Arc<str>)],
    ) -> anyhow::Result<()> {
        let mut progress = console::Progress::new(console.clone(), "inspect-checkout", None, None);
        let mut locks = Vec::new();
        let mut checkout_repo = None;
        for git_task in checkout_rules {
            let rule_name = git_task.rule_name.clone();
            let dir_name: Arc<str> = git_task.spaces_key.clone();
            if rule_name.starts_with("//checkout:") {
                checkout_repo = Some((dir_name.clone(), git_task.url.clone()));
            }

            let repo = git::Repository::new(git_task.url.clone(), dir_name.clone());
            if repo.is_dirty(&mut progress, git::IgnoreSubmodules::No) {
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
                .get_commit_hash(&mut progress)
                .context(format_context!("Failed to get commit for {rule_name}"))?;

            if let Some(commit_description) = repo.get_commit_tag(&mut progress).or(commit_hash) {
                locks.push((dir_name, commit_description))
            }
        }

        if let Some((checkout_dir_name, url)) = checkout_repo {
            let mut workspace_name: String = String::from(checkout_dir_name.as_ref());
            let mut command = format!(
                "spaces checkout-repo --url={url} \\\n  --rule-name={rule_name} \\\n",
                url = shell_quote(&url),
                rule_name = shell_quote(&checkout_dir_name),
            );
            for (dir_name, commit) in locks.iter() {
                if dir_name == &checkout_dir_name {
                    command.push_str(&format!("  --rev={} \\", shell_quote(commit)));
                    workspace_name.push_str(&format!("-{commit}"));
                } else {
                    command.push_str(&format!(
                        "  --lock={} \\",
                        shell_quote(&format!("{dir_name}={commit}"))
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

            let workspace_name = workspace_name.replace("/", "-");
            command.push_str(&format!("  --name={}\n", shell_quote(&workspace_name)));

            console.raw("\n")?;
            console.raw(&command)?;
            console.raw("\n")?;
            Ok(())
        } else {
            Err(format_error!(
                "Workspace was not created using spaces checkout-repo"
            ))
        }
    }
}
