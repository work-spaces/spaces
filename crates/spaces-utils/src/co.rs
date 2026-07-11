use crate::{git, search, suggest};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use console::style::StyledContent;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
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
            "Source: {checkout_display}\n Failed to find `{checkout}` or any similar entries."
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

#[derive(Debug, clap::Subcommand, Clone)]
pub enum QueryCoCommand {
    #[command(
        about = r"Search checkout entries using keyword matching across full entry content.
  - `spaces query-co search spaces`: search by entry name/help/url/etc
  - `spaces query-co search github main`: search with multiple keywords
  - `spaces query-co search build --limit=20`: show top 20 matches
  - `spaces query-co search spaces --format=json`: output structured JSON"
    )]
    Search {
        /// One or more keywords to search across checkout entry fields
        #[arg(required = true, num_args = 1..)]
        keywords: Vec<Arc<str>>,
        /// Maximum number of results to show
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Output format
        #[arg(long, value_enum, default_value_t = console::Format::Pretty)]
        format: console::Format,
    },
    #[command(
        about = r"List checkout entries, optionally filtered across full entry content.
  - `spaces query-co list`: list all entries from co.spaces.toml
  - `spaces query-co list --filter=github`: filter by glob-like expression
  - `spaces query-co list --filter='*deprecated*'`: include matches for wildcard filter text
  - `spaces query-co list --format=yaml`: output structured YAML"
    )]
    List {
        /// Filter entries with a glob-like expression
        #[arg(long)]
        filter: Option<Arc<str>>,
        /// Output format
        #[arg(long, value_enum, default_value_t = console::Format::Pretty)]
        format: console::Format,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryCoEntryKind {
    Workflow,
    Repo,
}

impl QueryCoEntryKind {
    fn as_str(self) -> &'static str {
        match self {
            QueryCoEntryKind::Workflow => "Workflow",
            QueryCoEntryKind::Repo => "Repo",
        }
    }
}

#[derive(Debug, Clone)]
struct QueryCoEntry {
    name: Arc<str>,
    kind: QueryCoEntryKind,
    workflow: Option<Arc<str>>,
    script: Vec<Arc<str>>,
    url: Option<Arc<str>>,
    rev: Option<Arc<str>>,
    rule_name: Option<Arc<str>>,
    clone_mode: Option<String>,
    help: Option<Arc<str>>,
    env: Vec<Arc<str>>,
    store: Vec<(String, String)>,
    new_branch: Vec<Arc<str>>,
    searchable_fields: Vec<String>,
}

#[derive(Debug, Clone)]
struct ScoredQueryCoEntry {
    score: isize,
    entry: QueryCoEntry,
}

#[derive(Debug, Serialize)]
struct QueryCoRenderableEntry {
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workflow: Option<Arc<str>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    script: Vec<Arc<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<Arc<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rev: Option<Arc<str>>,
    #[serde(rename = "rule-name", skip_serializing_if = "Option::is_none")]
    rule_name: Option<Arc<str>>,
    #[serde(rename = "clone", skip_serializing_if = "Option::is_none")]
    clone_mode: Option<String>,
    #[serde(rename = "new-branch", skip_serializing_if = "Vec::is_empty")]
    new_branch: Vec<Arc<str>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    env: Vec<Arc<str>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    store: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<Arc<str>>,
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
    pub store_for_docstring: Option<Vec<Arc<str>>>,
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

    let store_values = checkout_args
        .store_for_docstring
        .as_ref()
        .unwrap_or(&checkout_args.store);
    for store_val in store_values {
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
pub struct CheckoutDerivedRepo {
    #[serde(alias = "derive-from")]
    pub derive_from: Arc<str>,
    pub url: Option<Arc<str>>,
    #[serde(alias = "rule-name")]
    pub rule_name: Option<Arc<str>>,
    pub rev: Option<Arc<str>>,
    #[serde(alias = "new-branch")]
    pub new_branch: Option<Vec<Arc<str>>>,
    pub clone: Option<git::Clone>,
    pub env: Option<Vec<Arc<str>>>,
    pub store: Option<HashMap<Arc<str>, toml::Value>>,
    #[serde(alias = "create-lock-file")]
    pub create_lock_file: Option<bool>,
    pub help: Option<Arc<str>>,
}

impl CheckoutDerivedRepo {
    fn resolve(self, base: CheckoutRepo) -> CheckoutRepo {
        CheckoutRepo {
            url: self.url.unwrap_or(base.url),
            rule_name: self.rule_name.or(base.rule_name),
            rev: self.rev.unwrap_or(base.rev),
            new_branch: self.new_branch.or(base.new_branch),
            clone: self.clone.or(base.clone),
            env: self.env.or(base.env),
            store: self.store.or(base.store),
            create_lock_file: self.create_lock_file.or(base.create_lock_file),
            help: self.help.or(base.help),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Checkout {
    Workflow(CheckoutWorkflow),
    Repo(CheckoutRepo),
    RepoDerived(CheckoutDerivedRepo),
}

impl Checkout {
    fn resolve_checkout_map(
        checkout_map: HashMap<Arc<str>, Checkout>,
    ) -> Result<HashMap<Arc<str>, Self>, String> {
        let mut resolved_checkout_map = HashMap::with_capacity(checkout_map.len());
        let mut resolved_repo_map: HashMap<Arc<str>, CheckoutRepo> = HashMap::new();

        for (checkout_name, checkout) in &checkout_map {
            let checkout = match checkout {
                Checkout::Workflow(workflow) => Checkout::Workflow(workflow.clone()),
                Checkout::Repo(_) | Checkout::RepoDerived(_) => Checkout::Repo(Self::resolve_repo(
                    checkout_name,
                    &checkout_map,
                    &mut resolved_repo_map,
                    &mut Vec::new(),
                )?),
            };

            resolved_checkout_map.insert(checkout_name.clone(), checkout);
        }

        Ok(resolved_checkout_map)
    }

    fn resolve_repo(
        checkout_name: &Arc<str>,
        checkout_map: &HashMap<Arc<str>, Checkout>,
        resolved_repo_map: &mut HashMap<Arc<str>, CheckoutRepo>,
        derive_stack: &mut Vec<Arc<str>>,
    ) -> Result<CheckoutRepo, String> {
        if let Some(resolved_repo) = resolved_repo_map.get(checkout_name) {
            return Ok(resolved_repo.clone());
        }

        if let Some(start_index) = derive_stack.iter().position(|entry| entry == checkout_name) {
            let mut cycle = derive_stack[start_index..]
                .iter()
                .map(|entry| entry.to_string())
                .collect::<Vec<_>>();
            cycle.push(checkout_name.to_string());
            return Err(format!("found `derive-from` cycle: {}", cycle.join(" -> ")));
        }

        let checkout = match checkout_map.get(checkout_name) {
            Some(checkout) => checkout.clone(),
            None => {
                return Err(format!("checkout entry `{checkout_name}` was not found"));
            }
        };

        derive_stack.push(checkout_name.clone());
        let resolved_repo: Result<CheckoutRepo, String> = match checkout {
            Checkout::Repo(repo) => Ok(repo),
            Checkout::RepoDerived(derived_repo) => {
                let base_checkout_name = derived_repo.derive_from.clone();
                let base_repo = Self::resolve_repo(
                    &base_checkout_name,
                    checkout_map,
                    resolved_repo_map,
                    derive_stack,
                )
                .map_err(|err| {
                    format!(
                        "while resolving `derive-from`\n  for `{checkout_name}`\n  from `{base_checkout_name}`\n  {err:?}"
                    )
                })?;
                Ok(derived_repo.resolve(base_repo))
            }
            Checkout::Workflow(_) => Err(format!(
                "checkout entry `{checkout_name}` is a Workflow and cannot be used with `derive-from`"
            )),
        };
        derive_stack.pop();

        let resolved_repo = resolved_repo?;
        resolved_repo_map.insert(checkout_name.clone(), resolved_repo.clone());
        Ok(resolved_repo)
    }

    fn parse_checkout_file(file_path: &std::path::Path) -> anyhow::Result<HashMap<Arc<str>, Self>> {
        let contents = std::fs::read_to_string(file_path).map_err(|err| {
            format_error!(
                "Failed to open {} while loading `co` shortcuts\n{err:?}",
                file_path.display()
            )
        })?;

        toml::from_str(&contents).map_err(|err| {
            format_error!("while parsing toml file {}\n{err:#}", file_path.display())
        })
    }

    fn load_checkout_file(file_path: &std::path::Path) -> anyhow::Result<HashMap<Arc<str>, Self>> {
        let checkout_map = Self::parse_checkout_file(file_path)?;

        Self::resolve_checkout_map(checkout_map).map_err(|err| {
            format_error!(
                "while resolving derived checkout repos in {}\n{err}",
                file_path.display()
            )
        })
    }

    fn load_checkout_directory(
        directory_path: &std::path::Path,
    ) -> anyhow::Result<HashMap<Arc<str>, Self>> {
        let mut manifest_paths = std::fs::read_dir(directory_path)
            .map_err(|err| {
                format_error!(
                    "while reading entries in {CO_ENV_NAME} directory {}\n{err:?}",
                    directory_path.display()
                )
            })?
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|err| format_error!("while reading directory entry\n{err:?}"))
            })
            .collect::<anyhow::Result<Vec<std::path::PathBuf>>>()?;

        manifest_paths.retain(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".co.spaces.toml"))
        });
        manifest_paths.sort();

        if manifest_paths.is_empty() {
            return Err(format_error!(
                "No files ending in `.co.spaces.toml` were found in {}",
                directory_path.display()
            ));
        }

        let mut checkout_map = HashMap::new();
        let mut checkout_source_map: HashMap<Arc<str>, std::path::PathBuf> = HashMap::new();

        for manifest_path in manifest_paths {
            let entries = Self::parse_checkout_file(&manifest_path)?;
            for (name, checkout) in entries {
                if let Some(existing_source) =
                    checkout_source_map.insert(name.clone(), manifest_path.clone())
                {
                    return Err(format_error!(
                        "Found duplicate checkout entry `{name}` while aggregating {}\nFirst defined in {}\nDuplicate defined in {}",
                        directory_path.display(),
                        existing_source.display(),
                        manifest_path.display(),
                    ));
                }

                checkout_map.insert(name, checkout);
            }
        }

        Self::resolve_checkout_map(checkout_map).map_err(|err| {
            format_error!(
                "while resolving derived checkout repos while aggregating {}\n{err}",
                directory_path.display()
            )
        })
    }

    pub fn load() -> anyhow::Result<(HashMap<Arc<str>, Self>, std::path::PathBuf)> {
        let co_file_path = std::path::Path::new(CO_FILE_NAME);
        let effective_path = if co_file_path.exists() {
            co_file_path.to_owned()
        } else {
            let env_path = std::env::var(CO_ENV_NAME).map_err(|err| {
                format_error!(
                    "{CO_FILE_NAME} does not exist in the current directory\n And {CO_ENV_NAME} is not set in ENV\n{err:?}",
                )
            })?;
            env_path.into()
        };

        let checkout = if effective_path.is_dir() {
            Self::load_checkout_directory(&effective_path)?
        } else {
            Self::load_checkout_file(&effective_path)?
        };

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
            Checkout::RepoDerived(_) => {
                return Err(format_error!(
                    "Internal Error: RepoDerived should be resolved before applying overrides"
                ));
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
            Checkout::RepoDerived(_) => {
                return Err(format_error!(
                    "Internal Error: RepoDerived should be resolved before applying overrides"
                ));
            }
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
            Checkout::RepoDerived(_) => {
                return Err(format_error!(
                    "Internal Error: RepoDerived should be resolved before applying overrides"
                ));
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

impl QueryCoCommand {
    pub fn execute(&self, console: console::Console) -> anyhow::Result<()> {
        let (checkout_map, checkout_file_path) = Checkout::load().context(format_context!(
            "Failed to load checkout entries from {}",
            CO_FILE_NAME
        ))?;

        let entries = normalize_query_co_entries(&checkout_map);

        match self {
            QueryCoCommand::Search {
                keywords,
                limit,
                format,
            } => {
                if *limit == 0 {
                    return Err(format_error!("--limit must be greater than 0"));
                }

                let results = search_query_co_entries(&entries, keywords, *limit);
                if results.is_empty() {
                    console.error(
                        "No Results",
                        format!(
                            "No checkout entries in {} matched keywords: {}",
                            checkout_file_path.display(),
                            keywords
                                .iter()
                                .map(|k| k.as_ref())
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                        .as_str(),
                    )?;
                    return Ok(());
                }

                match format {
                    console::Format::Pretty => {
                        for scored in &results {
                            emit_pretty_query_co_entry(&console, &scored.entry, Some(keywords));
                        }
                    }
                    console::Format::Yaml => {
                        let rendered =
                            build_query_co_output_map(results.iter().map(|scored| &scored.entry));
                        console.write(&serialise_query_co_entries_yaml(&rendered).context(
                            format_context!(
                                "Internal Error: while serializing query-co search results for YAML"
                            ),
                        )?)?;
                    }
                    console::Format::Json => {
                        let rendered =
                            build_query_co_output_map(results.iter().map(|scored| &scored.entry));
                        console.write(&serialise_query_co_entries_json(&rendered).context(
                            format_context!(
                                "Internal Error: while serializing query-co search results for JSON"
                            ),
                        )?)?;
                    }
                }

                Ok(())
            }
            QueryCoCommand::List { filter, format } => {
                let filtered = select_entries_for_list(&entries, filter.as_deref());
                if filtered.is_empty() {
                    let message = if let Some(filter) = filter {
                        format!(
                            "No checkout entries in {} matched filter: {}",
                            checkout_file_path.display(),
                            filter
                        )
                    } else {
                        format!(
                            "No checkout entries were found in {}",
                            checkout_file_path.display()
                        )
                    };

                    console.error("No Results", message.as_str())?;
                    return Ok(());
                }

                match format {
                    console::Format::Pretty => {
                        for entry in &filtered {
                            emit_pretty_query_co_entry(&console, entry, None);
                        }
                    }
                    console::Format::Yaml => {
                        let rendered = build_query_co_output_map(filtered.iter());
                        console.write(&serialise_query_co_entries_yaml(&rendered).context(
                            format_context!(
                                "Internal Error: while serializing query-co list results for YAML"
                            ),
                        )?)?;
                    }
                    console::Format::Json => {
                        let rendered = build_query_co_output_map(filtered.iter());
                        console.write(&serialise_query_co_entries_json(&rendered).context(
                            format_context!(
                                "Internal Error: while serializing query-co list results for JSON"
                            ),
                        )?)?;
                    }
                }

                Ok(())
            }
        }
    }
}

fn query_co_entry_to_renderable(entry: &QueryCoEntry) -> QueryCoRenderableEntry {
    let store = entry
        .store
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();

    QueryCoRenderableEntry {
        kind: entry.kind.as_str().to_string(),
        workflow: entry.workflow.clone(),
        script: entry.script.clone(),
        url: entry.url.clone(),
        rev: entry.rev.clone(),
        rule_name: entry.rule_name.clone(),
        clone_mode: entry.clone_mode.clone(),
        new_branch: entry.new_branch.clone(),
        env: entry.env.clone(),
        store,
        help: entry.help.clone(),
    }
}

fn build_query_co_output_map<'a>(
    entries: impl IntoIterator<Item = &'a QueryCoEntry>,
) -> BTreeMap<Arc<str>, QueryCoRenderableEntry> {
    entries
        .into_iter()
        .map(|entry| (entry.name.clone(), query_co_entry_to_renderable(entry)))
        .collect()
}

fn serialise_query_co_entries_json(
    entries: &BTreeMap<Arc<str>, QueryCoRenderableEntry>,
) -> anyhow::Result<String> {
    let mut output = serde_json::to_string_pretty(entries).context(format_context!(
        "Internal Error: failed to serialize query-co entries for JSON"
    ))?;
    output.push('\n');
    Ok(output)
}

fn serialise_query_co_entries_yaml(
    entries: &BTreeMap<Arc<str>, QueryCoRenderableEntry>,
) -> anyhow::Result<String> {
    serde_yaml::to_string(entries).context(format_context!(
        "Internal Error: failed to serialize query-co entries for YAML"
    ))
}

fn make_highlighted_line(value: &str, highlight_terms: Option<&[Arc<str>]>) -> console::Line {
    let mut line = console::Line::default();
    for (chunk, highlighted) in search::highlight_chunks(value, highlight_terms) {
        if chunk.is_empty() {
            continue;
        }

        if highlighted {
            line.push(console::Span::new_styled_lossy(StyledContent::new(
                console::warning_style(),
                chunk,
            )));
        } else {
            line.push(console::Span::new_unstyled_lossy(chunk));
        }
    }
    line
}

fn make_primary_highlighted_line(
    value: &str,
    highlight_terms: Option<&[Arc<str>]>,
) -> console::Line {
    let mut line = console::Line::default();
    for (chunk, highlighted) in search::highlight_chunks(value, highlight_terms) {
        if chunk.is_empty() {
            continue;
        }

        let style = if highlighted {
            console::warning_style()
        } else {
            console::primary_style()
        };

        line.push(console::Span::new_styled_lossy(StyledContent::new(
            style, chunk,
        )));
    }
    line
}

fn emit_pretty_query_co_entry(
    console: &console::Console,
    entry: &QueryCoEntry,
    highlight_terms: Option<&[Arc<str>]>,
) {
    let heading = format!("{} ({})", entry.name, entry.kind.as_str());
    let mut container = console::bootstrap::Container::new();
    container
        .add(console::bootstrap::Header::h3(heading).variant(console::bootstrap::Variant::Primary));

    let mut details = console::bootstrap::DescriptionList::new()
        .variant(console::bootstrap::Variant::Info)
        .compact(true);

    if highlight_terms.is_some_and(|terms| !terms.is_empty()) {
        details = details.item(
            "name",
            make_primary_highlighted_line(entry.name.as_ref(), highlight_terms),
        );
    }

    if let Some(url) = entry.url.as_deref() {
        if highlight_terms.is_some_and(|terms| !terms.is_empty()) {
            details = details.item("url", make_highlighted_line(url, highlight_terms));
        } else {
            details = details.item("url", console::bootstrap::Link::new(url).render());
        }
    }
    if let Some(rev) = entry.rev.as_deref() {
        details = details.item("rev", make_highlighted_line(rev, highlight_terms));
    }
    if let Some(rule_name) = entry.rule_name.as_deref() {
        details = details.item(
            "rule-name",
            make_highlighted_line(rule_name, highlight_terms),
        );
    }
    if let Some(clone_mode) = entry.clone_mode.as_deref() {
        details = details.item("clone", make_highlighted_line(clone_mode, highlight_terms));
    }
    if let Some(workflow) = entry.workflow.as_deref() {
        details = details.item("workflow", make_highlighted_line(workflow, highlight_terms));
    }

    if !entry.script.is_empty() {
        let mut script_list = console::bootstrap::List::unordered();
        for script_entry in &entry.script {
            script_list = script_list.item(make_highlighted_line(
                script_entry.as_ref(),
                highlight_terms,
            ));
        }
        details = details.item("script", script_list.render());
    }

    if !entry.new_branch.is_empty() {
        let mut new_branch_list = console::bootstrap::List::unordered();
        for new_branch_entry in &entry.new_branch {
            new_branch_list = new_branch_list.item(make_highlighted_line(
                new_branch_entry.as_ref(),
                highlight_terms,
            ));
        }
        details = details.item("new-branch", new_branch_list.render());
    }

    if !entry.env.is_empty() {
        let mut env_list = console::bootstrap::List::unordered();
        for env_entry in &entry.env {
            env_list = env_list.item(make_highlighted_line(env_entry.as_ref(), highlight_terms));
        }
        details = details.item("env", env_list.render());
    }

    if !entry.store.is_empty() {
        let mut store_list = console::bootstrap::List::unordered();
        for (key, value) in &entry.store {
            store_list = store_list.item(make_highlighted_line(
                format!("{key}={value}").as_str(),
                highlight_terms,
            ));
        }
        details = details.item("store", store_list.render());
    }

    if let Some(help) = entry.help.as_deref() {
        details = details.item("help", make_highlighted_line(help, highlight_terms));
    }

    container.add(details);
    container.add(console::bootstrap::VerticalSpacer::new(1));
    console.emit_container(&container);
}

fn normalize_query_co_entries(checkout_map: &HashMap<Arc<str>, Checkout>) -> Vec<QueryCoEntry> {
    let mut entries: Vec<QueryCoEntry> = checkout_map
        .iter()
        .map(|(name, checkout)| normalize_query_co_entry(name, checkout))
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn normalize_query_co_entry(name: &Arc<str>, checkout: &Checkout) -> QueryCoEntry {
    match checkout {
        Checkout::Workflow(workflow) => {
            let env = workflow.env.clone().unwrap_or_default();
            let store = normalize_store_entries(workflow.store.as_ref());
            let new_branch = workflow.new_branch.clone().unwrap_or_default();
            let script = workflow.script.clone().unwrap_or_default();

            let mut searchable_fields = Vec::new();
            push_searchable_field(&mut searchable_fields, name.as_ref());
            push_searchable_field(&mut searchable_fields, QueryCoEntryKind::Workflow.as_str());
            push_searchable_field(&mut searchable_fields, format!("name={name}"));

            if let Some(workflow_name) = workflow.workflow.as_deref() {
                push_searchable_field(&mut searchable_fields, workflow_name);
                push_searchable_field(&mut searchable_fields, format!("workflow={workflow_name}"));
            }

            for script_entry in &script {
                push_searchable_field(&mut searchable_fields, script_entry.as_ref());
                push_searchable_field(&mut searchable_fields, format!("script={script_entry}"));
            }

            if let Some(help) = workflow.help.as_deref() {
                push_searchable_field(&mut searchable_fields, help);
                push_searchable_field(&mut searchable_fields, format!("help={help}"));
            }

            for env_entry in &env {
                push_searchable_field(&mut searchable_fields, env_entry.as_ref());
                push_searchable_field(&mut searchable_fields, format!("env={env_entry}"));
            }

            for (key, value) in &store {
                push_searchable_field(&mut searchable_fields, key.as_str());
                push_searchable_field(&mut searchable_fields, value.as_str());
                push_searchable_field(&mut searchable_fields, format!("{key}={value}"));
            }

            for new_branch_entry in &new_branch {
                push_searchable_field(&mut searchable_fields, new_branch_entry.as_ref());
                push_searchable_field(
                    &mut searchable_fields,
                    format!("new-branch={new_branch_entry}"),
                );
            }

            QueryCoEntry {
                name: name.clone(),
                kind: QueryCoEntryKind::Workflow,
                workflow: workflow.workflow.clone(),
                script,
                url: None,
                rev: None,
                rule_name: None,
                clone_mode: None,
                help: workflow.help.clone(),
                env,
                store,
                new_branch,
                searchable_fields,
            }
        }
        Checkout::Repo(repo) => {
            let env = repo.env.clone().unwrap_or_default();
            let store = normalize_store_entries(repo.store.as_ref());
            let new_branch = repo.new_branch.clone().unwrap_or_default();

            let mut searchable_fields = Vec::new();
            push_searchable_field(&mut searchable_fields, name.as_ref());
            push_searchable_field(&mut searchable_fields, QueryCoEntryKind::Repo.as_str());
            push_searchable_field(&mut searchable_fields, format!("name={name}"));
            push_searchable_field(&mut searchable_fields, repo.url.as_ref());
            push_searchable_field(&mut searchable_fields, format!("url={}", repo.url));
            push_searchable_field(&mut searchable_fields, repo.rev.as_ref());
            push_searchable_field(&mut searchable_fields, format!("rev={}", repo.rev));

            if let Some(rule_name) = repo.rule_name.as_deref() {
                push_searchable_field(&mut searchable_fields, rule_name);
                push_searchable_field(&mut searchable_fields, format!("rule-name={rule_name}"));
            }

            if let Some(clone_mode) = repo.clone {
                push_searchable_field(&mut searchable_fields, clone_mode.to_string());
                push_searchable_field(&mut searchable_fields, format!("clone={clone_mode}"));
            }

            if let Some(help) = repo.help.as_deref() {
                push_searchable_field(&mut searchable_fields, help);
                push_searchable_field(&mut searchable_fields, format!("help={help}"));
            }

            for env_entry in &env {
                push_searchable_field(&mut searchable_fields, env_entry.as_ref());
                push_searchable_field(&mut searchable_fields, format!("env={env_entry}"));
            }

            for (key, value) in &store {
                push_searchable_field(&mut searchable_fields, key.as_str());
                push_searchable_field(&mut searchable_fields, value.as_str());
                push_searchable_field(&mut searchable_fields, format!("{key}={value}"));
            }

            for new_branch_entry in &new_branch {
                push_searchable_field(&mut searchable_fields, new_branch_entry.as_ref());
                push_searchable_field(
                    &mut searchable_fields,
                    format!("new-branch={new_branch_entry}"),
                );
            }

            QueryCoEntry {
                name: name.clone(),
                kind: QueryCoEntryKind::Repo,
                workflow: None,
                script: vec![],
                url: Some(repo.url.clone()),
                rev: Some(repo.rev.clone()),
                rule_name: repo.rule_name.clone(),
                clone_mode: repo.clone.map(|clone_mode| clone_mode.to_string()),
                help: repo.help.clone(),
                env,
                store,
                new_branch,
                searchable_fields,
            }
        }
        Checkout::RepoDerived(repo) => {
            let env = repo.env.clone().unwrap_or_default();
            let store = normalize_store_entries(repo.store.as_ref());
            let new_branch = repo.new_branch.clone().unwrap_or_default();

            let mut searchable_fields = Vec::new();
            push_searchable_field(&mut searchable_fields, name.as_ref());
            push_searchable_field(&mut searchable_fields, QueryCoEntryKind::Repo.as_str());
            push_searchable_field(&mut searchable_fields, format!("name={name}"));
            push_searchable_field(
                &mut searchable_fields,
                format!("derive-from={}", repo.derive_from),
            );

            if let Some(url) = repo.url.as_deref() {
                push_searchable_field(&mut searchable_fields, url);
                push_searchable_field(&mut searchable_fields, format!("url={url}"));
            }
            if let Some(rev) = repo.rev.as_deref() {
                push_searchable_field(&mut searchable_fields, rev);
                push_searchable_field(&mut searchable_fields, format!("rev={rev}"));
            }

            if let Some(rule_name) = repo.rule_name.as_deref() {
                push_searchable_field(&mut searchable_fields, rule_name);
                push_searchable_field(&mut searchable_fields, format!("rule-name={rule_name}"));
            }

            if let Some(clone_mode) = repo.clone {
                push_searchable_field(&mut searchable_fields, clone_mode.to_string());
                push_searchable_field(&mut searchable_fields, format!("clone={clone_mode}"));
            }

            if let Some(help) = repo.help.as_deref() {
                push_searchable_field(&mut searchable_fields, help);
                push_searchable_field(&mut searchable_fields, format!("help={help}"));
            }

            for env_entry in &env {
                push_searchable_field(&mut searchable_fields, env_entry.as_ref());
                push_searchable_field(&mut searchable_fields, format!("env={env_entry}"));
            }

            for (key, value) in &store {
                push_searchable_field(&mut searchable_fields, key.as_str());
                push_searchable_field(&mut searchable_fields, value.as_str());
                push_searchable_field(&mut searchable_fields, format!("{key}={value}"));
            }

            for new_branch_entry in &new_branch {
                push_searchable_field(&mut searchable_fields, new_branch_entry.as_ref());
                push_searchable_field(
                    &mut searchable_fields,
                    format!("new-branch={new_branch_entry}"),
                );
            }

            QueryCoEntry {
                name: name.clone(),
                kind: QueryCoEntryKind::Repo,
                workflow: None,
                script: vec![],
                url: repo.url.clone(),
                rev: repo.rev.clone(),
                rule_name: repo.rule_name.clone(),
                clone_mode: repo.clone.map(|clone_mode| clone_mode.to_string()),
                help: repo.help.clone(),
                env,
                store,
                new_branch,
                searchable_fields,
            }
        }
    }
}

fn push_searchable_field(fields: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !value.trim().is_empty() {
        fields.push(value);
    }
}

fn normalize_store_entries(
    store: Option<&HashMap<Arc<str>, toml::Value>>,
) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = store
        .into_iter()
        .flatten()
        .map(|(key, value)| (key.to_string(), toml_value_to_plain_string(value)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn toml_value_to_plain_string(value: &toml::Value) -> String {
    match value {
        toml::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn select_entries_for_list(entries: &[QueryCoEntry], filter: Option<&str>) -> Vec<QueryCoEntry> {
    let globs = filter.map(search::build_filter_globs).unwrap_or_default();

    let mut filtered: Vec<QueryCoEntry> = entries
        .iter()
        .filter(|entry| entry_matches_filter(entry, &globs, filter))
        .cloned()
        .collect();

    filtered.sort_by(|a, b| a.name.cmp(&b.name));
    filtered
}

fn entry_matches_filter(
    entry: &QueryCoEntry,
    globs: &crate::changes::glob::Globs,
    raw_filter: Option<&str>,
) -> bool {
    if globs.is_empty() {
        if let Some(raw_filter) = raw_filter
            && let Some(fallback_match) = matches_simple_text_filter(entry, raw_filter)
        {
            return fallback_match;
        }
        return true;
    }

    let filter_fields = build_filter_fields(entry);

    if search::matches_filter_in_any_field(globs, filter_fields.iter().map(|field| field.as_str()))
    {
        return true;
    }

    if let Some(raw_filter) = raw_filter
        && let Some(fallback_match) = matches_simple_text_filter(entry, raw_filter)
    {
        return fallback_match;
    }

    false
}

fn build_filter_fields(entry: &QueryCoEntry) -> Vec<String> {
    let mut filter_fields = Vec::with_capacity(entry.searchable_fields.len() * 3);

    for field in &entry.searchable_fields {
        filter_fields.push(field.clone());
        filter_fields.push(format!("//{}:{field}", entry.name));

        let without_scheme = field
            .strip_prefix("https://")
            .or_else(|| field.strip_prefix("http://"))
            .unwrap_or(field);
        filter_fields.push(format!("//{}:{without_scheme}", entry.name));
    }

    filter_fields
}

fn matches_simple_text_filter(entry: &QueryCoEntry, raw_filter: &str) -> Option<bool> {
    let mut includes = Vec::new();

    for expression in raw_filter.split(',') {
        let expression = expression.trim();
        if expression.is_empty() {
            continue;
        }

        let value = expression
            .strip_prefix('+')
            .or_else(|| expression.strip_prefix('-'))
            .unwrap_or(expression);
        let value = value.trim().strip_prefix("//").unwrap_or(value.trim());
        if value.is_empty() {
            continue;
        }

        if value.contains('*') {
            return None;
        }

        includes.push(value.to_lowercase());
    }

    if includes.is_empty() {
        return Some(true);
    }

    let lowered_fields = entry
        .searchable_fields
        .iter()
        .map(|field| field.to_lowercase())
        .collect::<Vec<_>>();

    for include in includes {
        let include_match = lowered_fields.iter().any(|field| field.contains(&include));
        if !include_match {
            return Some(false);
        }
    }

    Some(true)
}

fn search_query_co_entries(
    entries: &[QueryCoEntry],
    keywords: &[Arc<str>],
    limit: usize,
) -> Vec<ScoredQueryCoEntry> {
    let mut scored: Vec<ScoredQueryCoEntry> = entries
        .iter()
        .filter_map(|entry| {
            score_entry_for_keywords(entry, keywords).map(|score| ScoredQueryCoEntry {
                score,
                entry: entry.clone(),
            })
        })
        .collect();

    // Select the strongest matches first, then render from least->most relevant
    // so the best result appears at the bottom.
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.entry.name.cmp(&b.entry.name))
    });

    let mut top_matches: Vec<ScoredQueryCoEntry> = scored.into_iter().take(limit).collect();
    top_matches.sort_by(|a, b| {
        a.score
            .cmp(&b.score)
            .then_with(|| a.entry.name.cmp(&b.entry.name))
    });

    top_matches
}

fn score_entry_for_keywords(entry: &QueryCoEntry, keywords: &[Arc<str>]) -> Option<isize> {
    let mut total_score: isize = 0;
    let mut matched_terms = 0usize;

    for keyword in keywords {
        let best_for_keyword = entry
            .searchable_fields
            .iter()
            .map(|field| search::score_match(keyword.as_ref(), field.as_str()))
            .max()
            .unwrap_or(0);

        if best_for_keyword > 0 {
            total_score += best_for_keyword;
            matched_terms += 1;
        }
    }

    if matched_terms == 0 {
        None
    } else {
        Some(total_score * matched_terms as isize)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Checkout, CheckoutArgs, CheckoutRepo, CheckoutRepoArgs, CheckoutWorkflow, QueryCoEntryKind,
        build_checkout_command_docstring, entry_matches_filter, normalize_query_co_entries,
        normalize_query_co_entry, score_entry_for_keywords, search_query_co_entries,
        select_entries_for_list,
    };
    use crate::search;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn arc(value: &str) -> Arc<str> {
        value.into()
    }

    fn repo_with_fields(
        name: &str,
        url: &str,
        help: &str,
        env: &[&str],
        store: &[(&str, toml::Value)],
        new_branch: &[&str],
        rule_name: Option<&str>,
    ) -> (Arc<str>, Checkout) {
        let mut store_map = HashMap::new();
        for (key, value) in store {
            store_map.insert(arc(key), value.clone());
        }

        (
            arc(name),
            Checkout::Repo(CheckoutRepo {
                url: arc(url),
                rule_name: rule_name.map(arc),
                rev: arc("main"),
                new_branch: Some(new_branch.iter().map(|entry| arc(entry)).collect()),
                clone: None,
                env: Some(env.iter().map(|entry| arc(entry)).collect()),
                store: Some(store_map),
                create_lock_file: None,
                help: Some(arc(help)),
            }),
        )
    }

    #[test]
    fn load_checkout_directory_aggregates_matching_manifests() {
        let temp_dir = tempdir().expect("failed to create tempdir");

        fs::write(
            temp_dir.path().join("alpha.co.spaces.toml"),
            r#"
[alpha.Repo]
url = "https://example.com/alpha"
rev = "main"
"#,
        )
        .expect("failed to write alpha manifest");

        fs::write(
            temp_dir.path().join("beta.co.spaces.toml"),
            r#"
[beta.Workflow]
script = ["echo beta"]
"#,
        )
        .expect("failed to write beta manifest");

        fs::write(
            temp_dir.path().join("ignored.toml"),
            "this is intentionally invalid and should be ignored",
        )
        .expect("failed to write ignored file");

        let checkout_map =
            Checkout::load_checkout_directory(temp_dir.path()).expect("failed to load manifests");

        assert_eq!(checkout_map.len(), 2);
        assert!(checkout_map.contains_key("alpha"));
        assert!(checkout_map.contains_key("beta"));
    }

    #[test]
    fn load_checkout_directory_errors_on_duplicate_entry_names() {
        let temp_dir = tempdir().expect("failed to create tempdir");

        fs::write(
            temp_dir.path().join("one.co.spaces.toml"),
            r#"
[shared.Repo]
url = "https://example.com/one"
rev = "main"
"#,
        )
        .expect("failed to write first manifest");

        fs::write(
            temp_dir.path().join("two.co.spaces.toml"),
            r#"
[shared.Repo]
url = "https://example.com/two"
rev = "main"
"#,
        )
        .expect("failed to write second manifest");

        let err = Checkout::load_checkout_directory(temp_dir.path())
            .expect_err("expected duplicate checkout entry error");
        let message = format!("{err:#}");

        assert!(message.contains("duplicate checkout entry `shared`"));
    }

    #[test]
    fn load_checkout_directory_resolves_derive_from_across_manifest_files() {
        let temp_dir = tempdir().expect("failed to create tempdir");

        fs::write(
            temp_dir.path().join("a.derived.co.spaces.toml"),
            r#"
[derived.RepoDerived]
derive-from = "base"
rev = "feature"
"#,
        )
        .expect("failed to write derived manifest");

        fs::write(
            temp_dir.path().join("b.base.co.spaces.toml"),
            r#"
[base.Repo]
url = "https://example.com/base"
rev = "main"
rule-name = "spaces"
"#,
        )
        .expect("failed to write base manifest");

        let checkout_map = Checkout::load_checkout_directory(temp_dir.path())
            .expect("failed to load directory with cross-file derive-from");

        let repo = match checkout_map.get("derived") {
            Some(Checkout::Repo(repo)) => repo,
            _ => panic!("expected derived checkout repo entry"),
        };

        assert_eq!(repo.url.as_ref(), "https://example.com/base");
        assert_eq!(repo.rev.as_ref(), "feature");
        assert_eq!(repo.rule_name.as_deref(), Some("spaces"));
    }

    #[test]
    fn checkout_repo_load_checkout_file_derive_from_inherits_and_overrides_fields() {
        let temp_dir = tempdir().expect("failed to create tempdir");
        let manifest_path = temp_dir.path().join("co.spaces.toml");

        fs::write(
            &manifest_path,
            r#"
[base.Repo]
url = "https://example.com/base"
rev = "main"
rule-name = "spaces"
new-branch = ["//spaces"]
clone = "Blobless"
env = ["BASE=yes"]
store = { REGION = "us" }
create-lock-file = true
help = "base help"

[demo.RepoDerived]
derive-from = "base"
rev = "feature"
env = ["CHILD=yes"]
"#,
        )
        .expect("failed to write checkout manifest");

        let checkout_map = Checkout::load_checkout_file(&manifest_path)
            .expect("failed to load checkout repo with derive-from");

        let repo = match checkout_map.get("demo") {
            Some(Checkout::Repo(repo)) => repo,
            _ => panic!("expected demo checkout repo entry"),
        };

        assert_eq!(repo.url.as_ref(), "https://example.com/base");
        assert_eq!(repo.rev.as_ref(), "feature");
        assert_eq!(repo.rule_name.as_deref(), Some("spaces"));
        assert_eq!(repo.clone, Some(crate::git::Clone::Blobless));
        assert_eq!(repo.env.as_deref(), Some([arc("CHILD=yes")].as_slice()));
        assert_eq!(
            repo.new_branch.as_deref(),
            Some([arc("//spaces")].as_slice())
        );
        assert_eq!(repo.create_lock_file, Some(true));
        assert_eq!(repo.help.as_deref(), Some("base help"));
        assert_eq!(
            repo.store
                .as_ref()
                .and_then(|store| store.get("REGION"))
                .and_then(|value| value.as_str()),
            Some("us")
        );
    }

    #[test]
    fn checkout_repo_load_checkout_file_repo_with_derive_from_errors() {
        let temp_dir = tempdir().expect("failed to create tempdir");
        let manifest_path = temp_dir.path().join("co.spaces.toml");

        fs::write(
            &manifest_path,
            r#"
[base.Repo]
url = "https://example.com/base"
rev = "main"
rule-name = "spaces"
env = ["BASE=yes"]

[demo.Repo]
derive-from = "base"
url = "https://example.com/demo"
rev = "feature"
"#,
        )
        .expect("failed to write checkout manifest");

        let err = Checkout::load_checkout_file(&manifest_path)
            .expect_err("expected Repo entry derive-from parse error");

        let message = format!("{err:#}");
        assert!(message.contains("unknown field `derive-from`"));
    }

    #[test]
    fn checkout_repo_load_checkout_file_derive_from_supports_chains() {
        let temp_dir = tempdir().expect("failed to create tempdir");
        let manifest_path = temp_dir.path().join("co.spaces.toml");

        fs::write(
            &manifest_path,
            r#"
[base.Repo]
url = "https://example.com/base"
rev = "main"
rule-name = "spaces"

[middle.RepoDerived]
derive-from = "base"
url = "https://example.com/middle"

[child.RepoDerived]
derive-from = "middle"
rev = "feature"
"#,
        )
        .expect("failed to write checkout manifest");

        let checkout_map =
            Checkout::load_checkout_file(&manifest_path).expect("failed to load checkout repos");

        let repo = match checkout_map.get("child") {
            Some(Checkout::Repo(repo)) => repo,
            _ => panic!("expected child checkout repo entry"),
        };

        assert_eq!(repo.url.as_ref(), "https://example.com/middle");
        assert_eq!(repo.rev.as_ref(), "feature");
        assert_eq!(repo.rule_name.as_deref(), Some("spaces"));
    }

    #[test]
    fn checkout_repo_load_checkout_file_derive_from_missing_entry_errors() {
        let temp_dir = tempdir().expect("failed to create tempdir");
        let manifest_path = temp_dir.path().join("co.spaces.toml");

        fs::write(
            &manifest_path,
            r#"
[demo.RepoDerived]
derive-from = "missing"
"#,
        )
        .expect("failed to write checkout manifest");

        let err = Checkout::load_checkout_file(&manifest_path)
            .expect_err("expected derive-from missing entry error");
        let message = format!("{err:#}");

        assert!(message.contains("checkout entry `missing` was not found"));
    }

    #[test]
    fn checkout_repo_load_checkout_file_derive_from_workflow_errors() {
        let temp_dir = tempdir().expect("failed to create tempdir");
        let manifest_path = temp_dir.path().join("co.spaces.toml");

        fs::write(
            &manifest_path,
            r#"
[wf.Workflow]
script = ["echo workflow"]

[demo.RepoDerived]
derive-from = "wf"
"#,
        )
        .expect("failed to write checkout manifest");

        let err = Checkout::load_checkout_file(&manifest_path)
            .expect_err("expected derive-from workflow type error");
        let message = format!("{err:#}");

        assert!(
            message.contains(
                "checkout entry `wf` is a Workflow and cannot be used with `derive-from`"
            )
        );
    }

    #[test]
    fn query_co_search_and_filter_cover_all_required_repo_fields() {
        let (name, checkout) = repo_with_fields(
            "name-token",
            "https://example.com/url-token",
            "help-token",
            &["ENV_TOKEN=value"],
            &[("STORE_TOKEN", toml::Value::String("store-token".into()))],
            &["new-branch-token"],
            Some("rule-name-token"),
        );

        let entry = normalize_query_co_entry(&name, &checkout);

        for keyword in [
            "name-token",
            "url-token",
            "help-token",
            "ENV_TOKEN",
            "STORE_TOKEN",
            "store-token",
            "new-branch-token",
            "rule-name-token",
        ] {
            let terms = vec![arc(keyword)];
            assert!(
                score_entry_for_keywords(&entry, &terms).is_some(),
                "expected keyword '{keyword}' to match searchable fields"
            );

            let globs = search::build_filter_globs(keyword);
            assert!(
                entry_matches_filter(&entry, &globs, Some(keyword)),
                "expected filter '{keyword}' to match searchable fields"
            );
        }
    }

    #[test]
    fn query_co_filter_matches_included_terms_only() {
        let (active_name, active_checkout) = repo_with_fields(
            "active",
            "https://github.com/work-spaces/active",
            "healthy",
            &[],
            &[],
            &[],
            None,
        );
        let (deprecated_name, deprecated_checkout) = repo_with_fields(
            "deprecated",
            "https://github.com/work-spaces/deprecated",
            "deprecated entry",
            &[],
            &[],
            &[],
            None,
        );

        let checkout_map = HashMap::from([
            (active_name, active_checkout),
            (deprecated_name, deprecated_checkout),
        ]);
        let entries = normalize_query_co_entries(&checkout_map);

        let filtered = select_entries_for_list(&entries, Some("-deprecated"));
        let names: Vec<&str> = filtered.iter().map(|entry| entry.name.as_ref()).collect();

        assert_eq!(names, vec!["deprecated"]);

        let wildcard_filtered = select_entries_for_list(&entries, Some("-*deprecated*"));
        let wildcard_names: Vec<&str> = wildcard_filtered
            .iter()
            .map(|entry| entry.name.as_ref())
            .collect();

        assert_eq!(wildcard_names, vec!["deprecated"]);
    }

    #[test]
    fn query_co_ranking_prefers_exact_prefix_then_substring() {
        let (exact_name, exact_checkout) = repo_with_fields(
            "build",
            "https://example.com/exact",
            "exact",
            &[],
            &[],
            &[],
            None,
        );
        let (prefix_name, prefix_checkout) = repo_with_fields(
            "builder",
            "https://example.com/prefix",
            "prefix",
            &[],
            &[],
            &[],
            None,
        );
        let (substring_name, substring_checkout) = repo_with_fields(
            "foo-build",
            "https://example.com/substring",
            "substring",
            &[],
            &[],
            &[],
            None,
        );

        let checkout_map = HashMap::from([
            (exact_name, exact_checkout),
            (prefix_name, prefix_checkout),
            (substring_name, substring_checkout),
        ]);
        let entries = normalize_query_co_entries(&checkout_map);

        let results = search_query_co_entries(&entries, &[arc("build")], 10);
        let ordered_names: Vec<&str> = results
            .iter()
            .map(|entry| entry.entry.name.as_ref())
            .collect();

        assert_eq!(ordered_names, vec!["foo-build", "builder", "build"]);
    }

    #[test]
    fn query_co_list_includes_repo_and_workflow_entries() {
        let (repo_name, repo_checkout) = repo_with_fields(
            "repo-entry",
            "https://example.com/repo",
            "repo help",
            &[],
            &[],
            &[],
            None,
        );

        let workflow_checkout = Checkout::Workflow(CheckoutWorkflow {
            workflow: Some(arc("workflows:demo")),
            script: None,
            env: None,
            store: None,
            new_branch: None,
            create_lock_file: None,
            help: Some(arc("workflow help")),
        });

        let checkout_map = HashMap::from([
            (repo_name, repo_checkout),
            (arc("wf-entry"), workflow_checkout),
        ]);
        let entries = normalize_query_co_entries(&checkout_map);
        let listed = select_entries_for_list(&entries, None);

        let kinds_by_name: HashMap<&str, QueryCoEntryKind> = listed
            .iter()
            .map(|entry| (entry.name.as_ref(), entry.kind))
            .collect();

        assert_eq!(
            kinds_by_name.get("repo-entry"),
            Some(&QueryCoEntryKind::Repo)
        );
        assert_eq!(
            kinds_by_name.get("wf-entry"),
            Some(&QueryCoEntryKind::Workflow)
        );
    }

    #[test]
    fn checkout_command_docstring_uses_store_for_docstring_when_present() {
        let docstring = build_checkout_command_docstring(
            "demo",
            crate::git::Clone::Default,
            &CheckoutRepoArgs {
                rule_name: None,
                url: arc("https://example.com/repo.git"),
                rev: arc("main"),
                clone: None,
            },
            &CheckoutArgs {
                env: vec![],
                store: vec![],
                store_for_docstring: Some(vec![arc("region=us"), arc("enabled=true")]),
                new_branch: vec![],
                create_lock_file: false,
                force_install_tools: false,
                keep_workspace_on_failure: false,
                lock: vec![],
            },
        );

        assert!(docstring.contains("--store=region=us"));
        assert!(docstring.contains("--store=enabled=true"));
    }
}
