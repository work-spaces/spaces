use crate::{changes::glob, inspect, markdown, rule, targets};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use clap::Subcommand;
use console::style::{Attribute, Attributes, Color, ContentStyle, StyledContent};
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
  - `spaces query rules`: show run rules
  - `spaces query rules --filter=**/my-pkg:*`: filter by glob pattern
  - `spaces query rules --has-help`: show only rules with help populated
  - `spaces query rules --checkout`: include checkout-phase rules
  - `spaces query rules --deps`: include expanded deps and targets in output
  - `spaces query rules --raw`: emit full task YAML per rule")]
    Rules {
        /// Filter rules with a glob pattern (e.g. `--filter=**/my-target`)
        #[arg(long)]
        filter: Option<Arc<str>>,
        /// Only show rules with the help entry populated
        #[arg(long)]
        has_help: bool,
        /// Include checkout-phase rules in output
        #[arg(long)]
        checkout: bool,
        /// Include expanded deps and targets in output
        #[arg(long)]
        deps: bool,
        /// Emit full task YAML per rule instead of the summary map (cannot be combined with --format)
        #[arg(long, conflicts_with = "format")]
        raw: bool,
        /// Output format
        #[arg(long, value_enum, default_value_t = Format::Yaml)]
        format: Format,
    },
    #[command(about = r"Show details for a specific rule.
  - `spaces query rule //my-pkg:build`: show rule details in YAML
  - `spaces query rule //my-pkg:build --format=json`: show rule details in JSON
  - `spaces query rule //my-pkg:build --deps`: include expanded deps in output")]
    Rule {
        /// The name of the rule to inspect (e.g. `//my-pkg:build`)
        name: Arc<str>,
        /// Include expanded deps in output
        #[arg(long)]
        deps: bool,
        /// Output format
        #[arg(long, value_enum, default_value_t = Format::Yaml)]
        format: Format,
    },
    #[command(about = r"Search for rules using fuzzy matching.
  - `spaces query search build`: return top 10 matches for 'build'
  - `spaces query search build test`: return top 10 matches across all terms
  - `spaces query search build --deps`: include expanded deps and targets in results
  - `spaces query search build --limit=20`: return top 20 matches")]
    Search {
        /// One or more search terms; a rule matches if any term matches
        #[arg(required = true, num_args = 1..)]
        query: Vec<Arc<str>>,
        /// Include expanded deps and targets in results
        #[arg(long)]
        deps: bool,
        /// Maximum number of results to show
        #[arg(long, default_value_t = 10)]
        limit: usize,
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
        let expanded: Vec<String> = if expr.starts_with('-') || expr.starts_with('+') {
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
    deps: bool,
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

        let (rule_deps, rule_targets) = if deps {
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
                deps: rule_deps,
                targets: rule_targets,
            },
        );
    }

    map
}

/// Serialise a `HashMap<Arc<str>, RuleInfo>` to the requested format string (JSON only).
fn serialise_rule_map_json(map: &HashMap<Arc<str>, RuleInfo>) -> String {
    let mut s = serde_json::to_string_pretty(map).unwrap_or_default();
    s.push('\n');
    s
}

fn name_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::Cyan),
        background_color: None,
        underline_color: None,
        attributes: Attributes::from(Attribute::Bold),
    }
}

fn key_style() -> ContentStyle {
    ContentStyle {
        foreground_color: Some(Color::DarkGrey),
        background_color: None,
        underline_color: None,
        attributes: Attributes::default(),
    }
}

fn make_name_line(name: &str) -> console::Line {
    let mut line = console::Line::default();
    line.push(console::Span::new_styled_lossy(StyledContent::new(
        name_style(),
        name.to_owned(),
    )));
    line
}

fn make_kv_line(key: &str, value: &str) -> console::Line {
    let mut line = console::Line::default();
    line.push(console::Span::new_styled_lossy(StyledContent::new(
        key_style(),
        format!("  {key:<8}"),
    )));
    line.push(console::Span::new_unstyled_lossy(value));
    line
}

fn emit_styled_rule(
    console: &console::Console,
    name: &str,
    source: &str,
    help: &str,
    deps: Option<&Vec<Arc<str>>>,
    targets: Option<&Vec<Arc<str>>>,
) {
    console.emit_line(make_name_line(name));
    console.emit_line(make_kv_line("source", source));
    console.emit_line(make_kv_line("help", help));
    if let Some(deps) = deps {
        for dep in deps {
            console.emit_line(make_kv_line("dep", dep.as_ref()));
        }
    }
    if let Some(targets) = targets {
        for target in targets {
            console.emit_line(make_kv_line("target", target.as_ref()));
        }
    }
    console.emit_line(console::Line::default());
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
                checkout,
                deps,
                raw,
                format,
            } => {
                let (globs, strip_prefix) = match filter {
                    Some(f) => (build_filter_globs(f.as_ref()), None),
                    None => default_globs(ctx.relative_invoked_path.as_ref()),
                };

                if *raw {
                    let glob_filter = glob::Globs::new_with_includes(&globs);
                    let rules = if *checkout {
                        ctx.checkout_rules
                            .iter()
                            .chain(ctx.run_rules.iter())
                            .collect::<Vec<_>>()
                    } else {
                        ctx.run_rules.iter().collect::<Vec<_>>()
                    };
                    for qr in rules {
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
                        console.write(&format!("# {raw_name}\n{}", qr.serialized_yaml))?;
                    }
                    return Ok(());
                }

                let mut map: HashMap<Arc<str>, RuleInfo> = HashMap::new();
                if *checkout {
                    map.extend(collect_rule_infos(
                        &ctx.checkout_rules,
                        &globs,
                        strip_prefix.as_ref(),
                        *has_help,
                        *deps,
                    ));
                }
                map.extend(collect_rule_infos(
                    &ctx.run_rules,
                    &globs,
                    strip_prefix.as_ref(),
                    *has_help,
                    *deps,
                ));

                if map.is_empty() {
                    console.error("No Results", "No matching rules available")?;
                } else {
                    match format {
                        Format::Yaml => {
                            let mut names: Vec<&Arc<str>> = map.keys().collect();
                            names.sort();
                            for name in names {
                                let info = &map[name];
                                emit_styled_rule(
                                    &console,
                                    name.as_ref(),
                                    &info.source,
                                    &info.help,
                                    info.deps.as_ref(),
                                    info.targets.as_ref(),
                                );
                            }
                        }
                        Format::Json => {
                            console.write(&serialise_rule_map_json(&map))?;
                        }
                    }
                }
                Ok(())
            }

            // ------------------------------------------------------------------
            QueryCommand::Rule { name, deps, format } => {
                let qr = ctx
                    .checkout_rules
                    .iter()
                    .chain(ctx.run_rules.iter())
                    .find(|qr| qr.rule.name.as_ref() == name.as_ref())
                    .ok_or_else(|| format_error!("Rule not found: {name}"))?;

                let output = if *deps {
                    match format {
                        Format::Yaml => {
                            let mut value: serde_yaml::Value =
                                serde_yaml::from_str(&qr.serialized_yaml)
                                    .context(format_context!("Failed to parse task YAML"))?;
                            if let Some(map) = value.as_mapping_mut() {
                                map.insert(
                                    serde_yaml::Value::String("expanded_deps".into()),
                                    serde_yaml::to_value(&qr.expanded_deps).context(
                                        format_context!("Failed to serialize expanded_deps"),
                                    )?,
                                );
                            }
                            serde_yaml::to_string(&value)
                                .context(format_context!("Failed to serialize task YAML"))?
                        }
                        Format::Json => {
                            let mut value: serde_json::Value =
                                serde_json::from_str(&qr.serialized_json)
                                    .context(format_context!("Failed to parse task JSON"))?;
                            if let Some(map) = value.as_object_mut() {
                                map.insert(
                                    "expanded_deps".into(),
                                    serde_json::to_value(&qr.expanded_deps).context(
                                        format_context!("Failed to serialize expanded_deps"),
                                    )?,
                                );
                            }
                            let mut s = serde_json::to_string_pretty(&value)
                                .context(format_context!("Failed to serialize task JSON"))?;
                            s.push('\n');
                            s
                        }
                    }
                } else {
                    match format {
                        Format::Yaml => qr.serialized_yaml.clone(),
                        Format::Json => qr.serialized_json.clone(),
                    }
                };
                console.raw(&output)?;
                Ok(())
            }

            // ------------------------------------------------------------------
            QueryCommand::Search { query, deps, limit } => {
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

                let mut scored: Vec<Scored> = Vec::new();

                for qr in ctx.checkout_rules.iter().chain(ctx.run_rules.iter()) {
                    let raw_name = qr.rule.name.as_ref();
                    // Score the rule against every term; keep the best match.
                    // Help-text matches are penalised (halved) so name matches rank higher.
                    let best_score = query
                        .iter()
                        .filter_map(|q| {
                            let name_score =
                                sublime_fuzzy::best_match(q.as_ref(), raw_name).map(|m| m.score());
                            let help_score = qr
                                .rule
                                .help
                                .as_deref()
                                .and_then(|h| sublime_fuzzy::best_match(q.as_ref(), h))
                                .map(|m| m.score() / 2);
                            name_score.max(help_score).or(name_score).or(help_score)
                        })
                        .max();

                    if let Some(score) = best_score {
                        let (rule_deps, rule_targets) = if *deps {
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
                                deps: rule_deps,
                                targets: rule_targets,
                            },
                        });
                    }
                }

                scored.sort_by(|a, b| b.score.cmp(&a.score));
                let top: IndexMap<Arc<str>, ScoredInfo> = scored
                    .into_iter()
                    .take(*limit)
                    .map(|s| (s.name, s.info))
                    .collect();

                if top.is_empty() {
                    console.error("No Results", "No matching rules found")?;
                } else {
                    for (name, info) in &top {
                        emit_styled_rule(
                            &console,
                            name.as_ref(),
                            &info.source,
                            &info.help,
                            info.deps.as_ref(),
                            info.targets.as_ref(),
                        );
                    }
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
