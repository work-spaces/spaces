use crate::{git, suggest};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub const CO_FILE_NAME: &str = "co.spaces.toml";
pub const CO_ENV_NAME: &str = "CO_SPACES_TOML";

pub fn get_checkout_not_found_error(
    checkout: Arc<str>,
    checkout_map: &HashMap<Arc<str>, Checkout>,
    checkout_file_path: &std::path::Path,
) -> anyhow::Error {
    let checkout_names = checkout_map.keys().cloned().collect::<Vec<Arc<str>>>();
    let suggestions = suggest::get_suggestions(checkout.clone(), &checkout_names)
        .iter()
        .take(10)
        .map(|(_, suggestion)| suggestion.to_string())
        .collect::<Vec<String>>();

    let checkout_display = checkout_file_path.display();
    if suggestions.is_empty() {
        format_error!(
            "Source: {checkout_display}\n Failed to find {checkout}` or any similar entries."
        )
    } else {
        format_error!(
            "Source: {checkout_display}\n Failed to find `{checkout}`. Did you mean?\n  {}",
            suggestions.join("\n  ")
        )
    }
}

#[derive(Debug, clap::Args)]
pub struct CoArgs {
    /// The name of the checkout entry (e.g. `spaces-dev` or `ninja-build` from above).
    pub checkout: Arc<str>,
    /// The name of the workspace to create.
    pub name: Arc<str>,
    /// Do not delete the workspace directory if checkout fails.
    #[arg(long)]
    pub keep_workspace_on_failure: bool,
    /// Override the checkout-repo revision in co.spaces.toml
    #[arg(long)]
    pub rev: Option<Arc<str>>,
    /// Override the checkout-repo rule-name in co.spaces.toml
    #[arg(long)]
    pub rule_name: Option<Arc<str>>,
    /// Override the checkout-repo url in co.spaces.toml
    #[arg(long)]
    pub url: Option<Arc<str>>,
    /// Additional env values to augment co.spaces.toml
    #[arg(long)]
    pub env: Vec<Arc<str>>,
    /// Additional store values to augment co.spaces.toml. Use `--store=KEY=VALUE`.
    #[arg(long)]
    pub store: Vec<Arc<str>>,
    /// Additional new-branch values to augment co.spaces.toml
    #[arg(long)]
    pub new_branch: Vec<Arc<str>>,
    /// Prevent a specific env entry from co.spaces.toml from being applied. Use `--no-env=NAME`.
    #[arg(long)]
    pub no_env: Vec<Arc<str>>,
    /// Prevent a specific store entry from co.spaces.toml from being applied. Use `--no-store=NAME`.
    #[arg(long)]
    pub no_store: Vec<Arc<str>>,
    /// Prevent a specific new-branch entry from co.spaces.toml from being applied. Use `--no-new-branch=PATH`.
    #[arg(long)]
    pub no_new_branch: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Override locks set in the rules.
  Use `--lock=REPO=REV`. Can be used multiple times."#
    )]
    pub lock: Vec<Arc<str>>,
    /// The workspaces lock rev's will override the rule rev for repos
    #[arg(long)]
    pub locked: bool,
}

pub struct CheckoutRepoArgs {
    pub rule_name: Option<Arc<str>>,
    pub url: Arc<str>,
    pub rev: Arc<str>,
    pub clone: Option<git::Clone>,
}

pub struct CheckoutWorkflowArgs {
    pub script: Vec<Arc<str>>,
    pub workflow: Option<Arc<str>>,
    pub wf: Option<Arc<str>>,
}

pub struct CheckoutArgs {
    pub env: Vec<Arc<str>>,
    pub store: Vec<Arc<str>>,
    pub new_branch: Vec<Arc<str>>,
    pub create_lock_file: bool,
    pub force_install_tools: bool,
    pub keep_workspace_on_failure: bool,
    pub lock: Vec<Arc<str>>,
}

pub fn build_checkout_command_docstring(
    name: &str,
    clone: git::Clone,
    repo_args: &CheckoutRepoArgs,
    checkout_args: &CheckoutArgs,
) -> String {
    let mut command_parts: Vec<String> = vec![
        format!("  --name={name}"),
        format!("  --url={}", repo_args.url),
        format!("  --rev={}", repo_args.rev),
    ];

    if let Some(rule_name) = repo_args.rule_name.as_deref() {
        command_parts.push(format!("  --rule-name={rule_name}"));
    }

    command_parts.push(format!("  --clone={clone}"));

    for env_val in &checkout_args.env {
        command_parts.push(format!("  --env={env_val}"));
    }

    for store_val in &checkout_args.store {
        command_parts.push(format!("  --store={store_val}"));
    }

    for branch in &checkout_args.new_branch {
        command_parts.push(format!("  --new-branch={branch}"));
    }

    for lock_val in &checkout_args.lock {
        command_parts.push(format!("  --lock={lock_val}"));
    }

    if checkout_args.create_lock_file {
        command_parts.push("  --create-lock-file".to_string());
    }

    format!(
        "\"\"\"\nspaces checkout-repo \\\n{}\n\"\"\"\n",
        command_parts.join(" \\\n")
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutWorkflow {
    pub workflow: Option<Arc<str>>,
    pub script: Option<Vec<Arc<str>>>,
    pub env: Option<Vec<Arc<str>>>,
    pub store: Option<HashMap<Arc<str>, toml::Value>>,
    #[serde(alias = "new-branch")]
    pub new_branch: Option<Vec<Arc<str>>>,
    #[serde(alias = "create-lock-file")]
    pub create_lock_file: Option<bool>,
    pub help: Option<Arc<str>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutRepo {
    pub url: Arc<str>,
    #[serde(alias = "rule-name")]
    pub rule_name: Option<Arc<str>>,
    pub rev: Arc<str>,
    #[serde(alias = "new-branch")]
    pub new_branch: Option<Vec<Arc<str>>>,
    pub clone: Option<git::Clone>,
    pub env: Option<Vec<Arc<str>>>,
    pub store: Option<HashMap<Arc<str>, toml::Value>>,
    #[serde(alias = "create-lock-file")]
    pub create_lock_file: Option<bool>,
    pub help: Option<Arc<str>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Checkout {
    Workflow(CheckoutWorkflow),
    Repo(CheckoutRepo),
}

impl Checkout {
    pub fn load() -> anyhow::Result<(HashMap<Arc<str>, Self>, std::path::PathBuf)> {
        let co_file_path = std::path::Path::new(CO_FILE_NAME);
        let effective_path = if co_file_path.exists() {
            co_file_path.to_owned()
        } else {
            let env_path = std::env::var(CO_ENV_NAME).context(format_context!(
                "{} does not exist in the current directory and {} is not set in ENV",
                CO_FILE_NAME,
                CO_ENV_NAME
            ))?;
            env_path.into()
        };

        let contents = std::fs::read_to_string(effective_path.clone()).context(format_context!(
            "Failed to open {} while loading `co` shortcuts",
            effective_path.display()
        ))?;

        let checkout = toml::from_str(&contents).context(format_context!(
            "Failed to parse toml file {}",
            effective_path.display()
        ))?;
        Ok((checkout, effective_path))
    }

    pub fn apply_overrides(&mut self, args: &CoArgs) -> anyhow::Result<()> {
        // Apply additive overrides
        match self {
            Checkout::Repo(repo) => {
                if let Some(rule_name) = args.rule_name.clone() {
                    repo.rule_name = Some(rule_name);
                }
                if let Some(url) = args.url.clone() {
                    repo.url = url;
                }
                if let Some(rev) = args.rev.clone() {
                    repo.rev = rev;
                }
                for entry in args.env.iter().cloned() {
                    repo.env.get_or_insert_default().push(entry);
                }
                for entry in args.store.iter() {
                    if let Some((key, value)) = entry.split_once('=') {
                        repo.store
                            .get_or_insert_default()
                            .insert(key.into(), toml::Value::String(value.to_string()));
                    } else {
                        return Err(format_error!(
                            "invalid store entry: {}. Use --store=<key>=<value>",
                            entry
                        ));
                    }
                }
                for entry in args.new_branch.iter().cloned() {
                    repo.new_branch.get_or_insert_default().push(entry);
                }
            }
            Checkout::Workflow(workflow) => {
                if args.rule_name.is_some() {
                    return Err(format_error!(
                        "--rule-name can only be used with CheckoutRepo"
                    ));
                }
                if args.url.is_some() {
                    return Err(format_error!("--url can only be used with CheckoutRepo"));
                }
                if args.rev.is_some() {
                    return Err(format_error!("--rev can only be used with CheckoutRepo"));
                }
                for entry in args.env.iter().cloned() {
                    workflow.env.get_or_insert_default().push(entry);
                }
                for entry in args.store.iter() {
                    if let Some((key, value)) = entry.split_once('=') {
                        workflow
                            .store
                            .get_or_insert_default()
                            .insert(key.into(), toml::Value::String(value.to_string()));
                    } else {
                        return Err(format_error!(
                            "invalid store entry: {}. Use --store=<key>=<value>",
                            entry
                        ));
                    }
                }
            }
        }

        // Validate --no-* exclusions exist in the config
        let (checkout_env, checkout_store, checkout_new_branch) = match self {
            Checkout::Repo(repo) => (
                repo.env.clone(),
                repo.store.clone(),
                repo.new_branch.clone(),
            ),
            Checkout::Workflow(workflow) => (
                workflow.env.clone(),
                workflow.store.clone(),
                workflow.new_branch.clone(),
            ),
        };
        for name in &args.no_env {
            let exists = checkout_env.as_ref().is_some_and(|list| {
                list.iter().any(|e| {
                    let key = e.split_once('=').map(|(k, _)| k).unwrap_or(e);
                    key == name.as_ref()
                })
            });
            if !exists {
                return Err(format_error!(
                    "--no-env={} does not exist in the config",
                    name
                ));
            }
        }
        for name in &args.no_store {
            let exists = checkout_store
                .as_ref()
                .is_some_and(|map| map.contains_key(name.as_ref()));
            if !exists {
                return Err(format_error!(
                    "--no-store={} does not exist in the config",
                    name
                ));
            }
        }
        for path in &args.no_new_branch {
            let exists = checkout_new_branch
                .as_ref()
                .is_some_and(|list| list.iter().any(|e| e.as_ref() == path.as_ref()));
            if !exists {
                return Err(format_error!(
                    "--no-new-branch={} does not exist in the config",
                    path
                ));
            }
        }

        // Apply exclusions
        match self {
            Checkout::Repo(repo) => {
                if let Some(env_list) = repo.env.as_mut() {
                    env_list.retain(|e| {
                        let key = e.split_once('=').map(|(k, _)| k).unwrap_or(e);
                        !args.no_env.iter().any(|n| n.as_ref() == key)
                    });
                }
                if let Some(store_map) = repo.store.as_mut() {
                    store_map
                        .retain(|k, _| !args.no_store.iter().any(|n| n.as_ref() == k.as_ref()));
                }
                if let Some(nb_list) = repo.new_branch.as_mut() {
                    nb_list
                        .retain(|e| !args.no_new_branch.iter().any(|n| n.as_ref() == e.as_ref()));
                }
            }
            Checkout::Workflow(workflow) => {
                if let Some(env_list) = workflow.env.as_mut() {
                    env_list.retain(|e| {
                        let key = e.split_once('=').map(|(k, _)| k).unwrap_or(e);
                        !args.no_env.iter().any(|n| n.as_ref() == key)
                    });
                }
                if let Some(store_map) = workflow.store.as_mut() {
                    store_map
                        .retain(|k, _| !args.no_store.iter().any(|n| n.as_ref() == k.as_ref()));
                }
                if let Some(nb_list) = workflow.new_branch.as_mut() {
                    nb_list
                        .retain(|e| !args.no_new_branch.iter().any(|n| n.as_ref() == e.as_ref()));
                }
            }
        }

        Ok(())
    }
}
