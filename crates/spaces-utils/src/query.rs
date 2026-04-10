use crate::{changes::glob, inspect, markdown, rule, targets};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use clap::Subcommand;
use indexmap::IndexMap;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Clap types
// ---------------------------------------------------------------------------

#[derive(clap::ValueEnum, Debug, Clone, Default)]
pub enum Format {
    #[default]
    Yaml,
    Json,
}

#[derive(clap::ValueEnum, Debug, Clone)]
pub enum ExportFormat {
    Markdown,
    Stardoc,
}

impl ExportFormat {
    fn infer_from_path(path: &str) -> Self {
        if path.ends_with(".star") || path.ends_with(".bzl") {
            ExportFormat::Stardoc
        } else {
            ExportFormat::Markdown
        }
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum QueryCommand {
    #[command(about = r"List rules in the workspace.
  - `spaces query rules`: show rules that have `help` entries
  - `spaces query rules --filter=**/my-pkg:*`: filter by glob pattern
  - `spaces query rules --has-help`: show only rules with help populated
  - `spaces --verbosity=message query rules`: show all rules")]
    Rules {
        /// Filter rules with a glob pattern (e.g. `--filter=**/my-target`)
        #[arg(long)]
        filter: Option<Arc<str>>,
        /// Only show rules with the help entry populated
        #[arg(long)]
        has_help: bool,
        /// Output format
        #[arg(long, value_enum, default_value_t = Format::Yaml)]
        format: Format,
    },
    #[command(about = r"Show details for a specific rule.
  - `spaces query rule //my-pkg:build`: show rule details in YAML
  - `spaces query rule //my-pkg:build --format=json`: show rule details in JSON")]
    Rule {
        /// The name of the rule to inspect (e.g. `//my-pkg:build`)
        name: Arc<str>,
        /// Output format
        #[arg(long, value_enum, default_value_t = Format::Yaml)]
        format: Format,
    },
    #[command(about = r"Search for rules using fuzzy matching.
  - `spaces query search build`: return top 10 matches for 'build'
  - `spaces query search build test`: return top 10 matches across all terms")]
    Search {
        /// One or more search terms; a rule matches if any term matches
        #[arg(required = true, num_args = 1..)]
        query: Vec<Arc<str>>,
    },
    #[command(
        about = r"Print the command to reproduce the current workspace checkout.
  - `spaces query checkout`: print the checkout command
  - `spaces query checkout --force`: print even if a repo is dirty"
    )]
    Checkout {
        /// Warn if a repo is dirty but still print the checkout command
        #[arg(long)]
        force: bool,
    },
    #[command(about = r"Export workspace documentation to a file.
  - `spaces query export ./docs/rules.md`: export as markdown
  - `spaces query export ./api.star --format=stardoc`: export as stardoc
  - Format is inferred from the file extension when not specified (.md → markdown, .star/.bzl → stardoc)")]
    Export {
        /// Output file path
        path: Arc<str>,
        /// Export format (inferred from file extension when omitted)
        #[arg(long, value_enum)]
        format: Option<ExportFormat>,
    },
}

// ---------------------------------------------------------------------------
// Data types passed in from the evaluator
// ---------------------------------------------------------------------------

/// Pre-computed display data for a single rule. Built by the evaluator from
/// `task::Task` so that `query.rs` has no dependency on the `spaces` crate.
#[derive(Debug)]
pub struct QueryRule {
    /// The full rule — gives access to name, help, deps (raw), targets, etc.
    pub rule: rule::Rule,
    /// Pre-computed source path (from `labels::get_source_from_label`).
    pub source: String,
    /// Rule-name deps plus workspace-expanded glob file paths.
    pub expanded_deps: Vec<Arc<str>>,
    /// Pre-computed executor markdown fragment, used by `Export`.
    pub executor_markdown: Option<String>,
    /// Full task serialised as YAML, used by `Rule --format=yaml`.
    pub serialized_yaml: String,
    /// Full task serialised as JSON, used by `Rule --format=json`.
    pub serialized_json: String,
}

/// Everything `execute()` needs, assembled by the evaluator.
#[derive(Debug)]
pub struct QueryContext {
    /// Rules in the Checkout phase.
    pub checkout_rules: Vec<QueryRule>,
    /// Rules in the Run phase.
    pub run_rules: Vec<QueryRule>,
    /// Checkout-phase Git tasks, used by the `Checkout` subcommand.
    pub checkout_git_tasks: Vec<inspect::GitTask>,
    /// Workspace-relative path where `spaces` was invoked; used to compute a
    /// default filter when none is supplied.
    pub relative_invoked_path: Arc<str>,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// YAML/JSON-serialisable summary of a single rule for the `rules` list view.
#[derive(Serialize)]
struct RuleInfo {
    source: String,
    help: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    deps: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    targets: Option<Vec<Arc<str>>>,
}

/// Extract target path strings from a `rule::Rule`.
fn extract_targets(r: &rule::Rule) -> Vec<Arc<str>> {
    r.targets
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|t| match t {
            targets::Target::File(f) => f.clone(),
            targets::Target::Directory(d) => d.clone(),
        })
        .collect()
}

/// Build annotated glob expressions from a `--filter` value.
/// Mirrors the expansion logic in `arguments.rs`.
fn build_filter_globs(filter: &str) -> HashSet<Arc<str>> {
    let mut globs = HashSet::new();
    for expr in filter.split(',') {
        let expanded: Vec<String> =
            if expr.starts_with('-') || expr.starts_with('+') {
                vec![expr.to_string()]
            } else if expr.contains('*') {
                vec![format!("+{expr}")]
            } else {
                vec![
                    format!("+**/*:*{expr}*"),
                    format!("+**/*{expr}*:*"),
                    format!("+**/{expr}*:*"),
                    format!("+**/*{expr}*/*:*"),
                ]
            };
        for e in expanded {
            globs.insert(e.into());
        }
    }
    globs
}

/// Returns the default (globs, strip_prefix) pair derived from the workspace
/// invocation path when no explicit `--filter` is provided.
fn default_globs(relative_invoked_path: &str) -> (HashSet<Arc<str>>, Option<Arc<str>>) {
    if relative_invoked_path.is_empty() {
        (HashSet::new(), None)
    } else {
        let mut globs = HashSet::new();
        globs.insert(format!("+{relative_invoked_path}**").into());
        let strip_prefix = Some(format!("//{relative_invoked_path}").into());
        (globs, strip_prefix)
    }
}

/// Collect and optionally filter a set of `QueryRule`s into `(name, RuleInfo)`
/// pairs, applying glob filtering, `has_help`, and name prefix-stripping.
fn collect_rule_infos(
    rules: &[QueryRule],
    globs: &HashSet<Arc<str>>,
    strip_prefix: Option<&Arc<str>>,
    has_help: bool,
    show_expanded: bool,
) -> HashMap<Arc<str>, RuleInfo> {
    let glob_filter = glob::Globs::new_with_includes(globs);
    let mut map: HashMap<Arc<str>, RuleInfo> = HashMap::new();

    for qr in rules {
        let raw_name = qr.rule.name.as_ref();

        if !globs.is_empty()
            && !glob_filter.is_match(raw_name.strip_prefix("//").unwrap_or(raw_name))
        {
            continue;
        }

        if has_help && qr.rule.help.is_none() {
            continue;
        }

        let display_name: Arc<str> = match strip_prefix {
            Some(prefix) => {
                let stripped = raw_name
                    .strip_prefix(prefix.as_ref())
                    .map(|s| s.strip_prefix('/').unwrap_or(s))
                    .unwrap_or(raw_name);
                stripped.into()
            }
            None => raw_name.into(),
        };

        let help = qr
            .rule
            .help
            .as_deref()
            .unwrap_or("<Not Provided>")
            .to_string();

        let (deps, targets) = if show_expanded {
            (
                Some(qr.expanded_deps.clone()),
                Some(extract_targets(&qr.rule)),
            )
        } else {
            (None, None)
        };

        map.insert(
            display_name,
            RuleInfo {
                source: qr.source.clone(),
                help,
                deps,
                targets,
            },
        );
    }

    map
}

/// Serialise a `HashMap<Arc<str>, RuleInfo>` to the requested format string.
fn serialise_rule_map(map: &HashMap<Arc<str>, RuleInfo>, format: &Format) -> String {
    match format {
        Format::Yaml => serde_yaml::to_string(map).unwrap_or_default(),
        Format::Json => {
            let mut s = serde_json::to_string_pretty(map).unwrap_or_default();
            s.push('\n');
            s
        }
    }
}

// ---------------------------------------------------------------------------
// QueryCommand::execute
// ---------------------------------------------------------------------------

impl QueryCommand {
    pub fn execute(&self, console: console::Console, ctx: &QueryContext) -> anyhow::Result<()> {
        match self {
            // ------------------------------------------------------------------
            QueryCommand::Rules {
                filter,
                has_help,
                format,
            } => {
                let (globs, strip_prefix) = match filter {
                    Some(f) => (build_filter_globs(f.as_ref()), None),
                    None => default_globs(ctx.relative_invoked_path.as_ref()),
                };

                let console_level = console.get_level();
                let show_expanded = console_level <= console::Level::Message;

                // Debug mode: emit full task YAML per rule directly.
                if console_level == console::Level::Debug {
                    let glob_filter = glob::Globs::new_with_includes(&globs);
                    for qr in ctx
                        .checkout_rules
                        .iter()
                        .chain(ctx.run_rules.iter())
                    {
                        let raw_name = qr.rule.name.as_ref();
                        if !globs.is_empty()
                            && !glob_filter
                                .is_match(raw_name.strip_prefix("//").unwrap_or(raw_name))
                        {
                            continue;
                        }
                        if *has_help && qr.rule.help.is_none() {
                            continue;
                        }
                        console.debug(raw_name, &qr.serialized_yaml)?;
                    }
                    return Ok(());
                }

                // Only include checkout rules at message verbosity or below.
                let mut map: HashMap<Arc<str>, RuleInfo> = HashMap::new();
                if console_level <= console::Level::Message {
                    map.extend(collect_rule_infos(
                        &ctx.checkout_rules,
                        &globs,
                        strip_prefix.as_ref(),
                        *has_help,
                        show_expanded,
                    ));
                }
                map.extend(collect_rule_infos(
                    &ctx.run_rules,
                    &globs,
                    strip_prefix.as_ref(),
                    *has_help,
                    show_expanded,
                ));

                if map.is_empty() {
                    console.error("No Results", "No matching rules available")?;
                } else {
                    console.write(&serialise_rule_map(&map, format))?;
                }
                Ok(())
            }

            // ------------------------------------------------------------------
            QueryCommand::Rule { name, format } => {
                let qr = ctx
                    .checkout_rules
                    .iter()
                    .chain(ctx.run_rules.iter())
                    .find(|qr| qr.rule.name.as_ref() == name.as_ref())
                    .ok_or_else(|| format_error!("Rule not found: {name}"))?;

                let output = match format {
                    Format::Yaml => qr.serialized_yaml.clone(),
                    Format::Json => qr.serialized_json.clone(),
                };
                console.raw(&output)?;
                Ok(())
            }

            // ------------------------------------------------------------------
            QueryCommand::Search { query } => {
                #[derive(Serialize)]
                struct ScoredInfo {
                    source: String,
                    help: String,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    deps: Option<Vec<Arc<str>>>,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    targets: Option<Vec<Arc<str>>>,
                }

                struct Scored {
                    score: isize,
                    name: Arc<str>,
                    info: ScoredInfo,
                }

                let console_level = console.get_level();
                let show_expanded = console_level <= console::Level::Message;
                let mut scored: Vec<Scored> = Vec::new();

                for qr in ctx.checkout_rules.iter().chain(ctx.run_rules.iter()) {
                    let raw_name = qr.rule.name.as_ref();
                    // Score the rule against every term; keep the best match.
                    let best_score = query
                        .iter()
                        .filter_map(|q| sublime_fuzzy::best_match(q.as_ref(), raw_name))
                        .map(|m| m.score())
                        .max();

                    if let Some(score) = best_score {
                        let (deps, targets) = if show_expanded {
                            (
                                Some(qr.expanded_deps.clone()),
                                Some(extract_targets(&qr.rule)),
                            )
                        } else {
                            (None, None)
                        };
                        scored.push(Scored {
                            score,
                            name: qr.rule.name.clone(),
                            info: ScoredInfo {
                                source: qr.source.clone(),
                                help: qr
                                    .rule
                                    .help
                                    .as_deref()
                                    .unwrap_or("<Not Provided>")
                                    .to_string(),
                                deps,
                                targets,
                            },
                        });
                    }
                }

                scored.sort_by(|a, b| b.score.cmp(&a.score));
                let top: IndexMap<Arc<str>, ScoredInfo> = scored
                    .into_iter()
                    .take(10)
                    .map(|s| (s.name, s.info))
                    .collect();

                if top.is_empty() {
                    console.error("No Results", "No matching rules found")?;
                } else {
                    let yaml = serde_yaml::to_string(&top).unwrap_or_default();
                    console.write(&yaml)?;
                }
                Ok(())
            }

            // ------------------------------------------------------------------
            QueryCommand::Checkout { force } => {
                let options = inspect::Options {
                    force: *force,
                    ..Default::default()
                };
                options
                    .execute_inspect_checkout(console, ctx.checkout_git_tasks.as_slice())
                    .context(format_context!("while printing checkout command"))
            }

            // ------------------------------------------------------------------
            QueryCommand::Export { path, format } => {
                let effective_format = match format {
                    Some(f) => f.clone(),
                    None => ExportFormat::infer_from_path(path.as_ref()),
                };

                match effective_format {
                    ExportFormat::Markdown => {
                        let checkout_pairs: Vec<(&rule::Rule, Option<String>)> = ctx
                            .checkout_rules
                            .iter()
                            .map(|qr| (&qr.rule, qr.executor_markdown.clone()))
                            .collect();
                        let run_pairs: Vec<(&rule::Rule, Option<String>)> = ctx
                            .run_rules
                            .iter()
                            .map(|qr| (&qr.rule, qr.executor_markdown.clone()))
                            .collect();

                        let file_console = console::Console::new_file(path.as_ref())
                            .context(format_context!("Failed to create file {path}"))?;
                        let mut md = markdown::Markdown::new(file_console);
                        rule::Rule::print_markdown_section(
                            &mut md,
                            "Checkout Rules",
                            &checkout_pairs,
                            false,
                            false,
                        )?;
                        rule::Rule::print_markdown_section(
                            &mut md,
                            "Run Rules",
                            &run_pairs,
                            true,
                            true,
                        )?;
                        Ok(())
                    }
                    ExportFormat::Stardoc => {
                        todo!("stardoc export")
                    }
                }
            }
        }
    }
}
