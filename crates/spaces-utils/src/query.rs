use crate::{changes::glob, graph, inspect, markdown, rule, search, suggest, targets};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use clap::Subcommand;
use console::style::StyledContent;
use indexmap::IndexMap;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use termtree::Tree;

// ---------------------------------------------------------------------------
// Clap types
// ---------------------------------------------------------------------------

#[derive(clap::ValueEnum, Debug, Clone)]
pub enum ExportFormat {
    Markdown,
    Stardoc,
}

impl ExportFormat {
    fn infer_from_path(_path: &str) -> Self {
        ExportFormat::Markdown
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum QueryCommand {
    #[command(about = r"List rules in the workspace.
  - `spaces query rules`: show run rules
  - `spaces query rules --filter='**/my-pkg:*'`: filter by glob pattern
  - `spaces query rules --filter='//my-pkg/**'`: filter by label-style glob prefix
  - `spaces query rules --has-help`: show only rules with help populated
  - `spaces query rules --checkout`: include checkout-phase rules
  - `spaces query rules --deps`: include expanded deps and targets in output
  - `spaces query rules --raw`: emit full task YAML per rule")]
    Rules {
        /// Filter rules with a glob pattern (e.g. `--filter=**/my-target` or `--filter=//my-pkg/**`)
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
        #[arg(long, value_enum, default_value_t = console::Format::Pretty)]
        format: console::Format,
    },
    #[command(about = r"Show details for a specific rule.
  - `spaces query rule //my-pkg:build`: show rule details in YAML
  - `spaces query rule //my-pkg:build --format=json`: show rule details in JSON
  - `spaces query rule //my-pkg:build --deps`: include expanded deps in output
  - `spaces query rule //my-pkg:checkout --checkout`: search checkout-phase rules")]
    Rule {
        /// The name of the rule to inspect (e.g. `//my-pkg:build`)
        name: Arc<str>,
        /// Include expanded deps in output
        #[arg(long)]
        deps: bool,
        /// Include checkout-phase rules in search
        #[arg(long)]
        checkout: bool,
        /// Output format
        #[arg(long, value_enum, default_value_t = console::Format::Yaml)]
        format: console::Format,
    },
    #[command(about = r"Search for rules using fuzzy matching and filters.
  - `spaces query search build`: return top 10 matches for 'build'
  - `spaces query search build test`: return top 10 matches across all terms
  - `spaces query search //my-pkg`: filter rules starting with //my-pkg
  - `spaces query search some/path`: filter rules that contain some/path
  - `spaces query search :build`: filter rules containing :build
  - `spaces query search //pkg build`: filter by //pkg prefix, then fuzzy search 'build'
  - `spaces query search build --deps`: include expanded deps and targets in results
  - `spaces query search build --limit=20`: return top 20 matches
  - `spaces query search build --checkout`: include checkout-phase rules in search")]
    Search {
        /// One or more search terms and filters.
        /// - Terms starting with '//' filter by rule name prefix
        /// - Terms with '/' filter by source path substring
        /// - Terms with ':' filter by rule name substring
        /// - Other terms are fuzzy-matched against name and help text
        #[arg(required = true, num_args = 1..)]
        query: Vec<Arc<str>>,
        /// Include expanded deps and targets in results
        #[arg(long)]
        deps: bool,
        /// Include checkout-phase rules in search
        #[arg(long)]
        checkout: bool,
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
    #[command(about = r"Export workspace documentation.
  - `spaces query export ./docs/rules.md`: export rules as a markdown file
  - `spaces query export ./docs/rules.md --checkout`: include checkout-phase rules in markdown
  - `spaces query export ./docs/api --format=stardoc`: export starlark module docs to a directory
  - For stardoc, PATH is a base directory; one .md file is written per .star module (mirrors `spaces inspect --stardoc`)
  - Stardoc always requires --format=stardoc; omitting --format defaults to markdown")]
    Export {
        /// Output file path
        path: Arc<str>,
        /// Include checkout-phase rules in export (markdown format only; rejected for stardoc)
        #[arg(long)]
        checkout: bool,
        /// Export format (defaults to markdown when omitted; stardoc always requires --format=stardoc)
        #[arg(long, value_enum)]
        format: Option<ExportFormat>,
    },
    #[command(about = r"Show dependency graph for a specific rule.
  - `spaces query graph //my-pkg:build`: show dependency tree for the rule
  - `spaces query graph //my-pkg:build --format=json`: output as JSON
  - `spaces query graph //my-pkg:build --format=yaml`: output as YAML")]
    Graph {
        /// The name of the rule to show dependency tree for (e.g. `//my-pkg:build`)
        rule: Arc<str>,
        /// Output format
        #[arg(long, value_enum, default_value_t = console::Format::Pretty)]
        format: console::Format,
    },
}

// ---------------------------------------------------------------------------
// Data types passed in from the evaluator
// ---------------------------------------------------------------------------

/// Configuration for what expensive fields to compute when building QueryContext.
/// This allows lazy computation based on what the specific QueryCommand needs.
#[derive(Debug, Clone, Default)]
pub struct QueryContextConfig {
    /// Whether to compute expanded_deps (glob expansion via workspace.inspect_inputs).
    pub compute_expanded_deps: bool,
    /// Whether to compute serialized_yaml/serialized_json for tasks.
    pub compute_serialization: bool,
    /// Whether to compute the full dependency graph.
    pub compute_graph: bool,
}

impl QueryContextConfig {
    /// Create a config that computes everything (legacy behavior).
    pub fn all() -> Self {
        Self {
            compute_expanded_deps: true,
            compute_serialization: true,
            compute_graph: true,
        }
    }

    /// Create a minimal config that computes nothing expensive.
    pub fn minimal() -> Self {
        Self {
            compute_expanded_deps: false,
            compute_serialization: false,
            compute_graph: false,
        }
    }
}

/// Pre-computed display data for a single rule. Built by the evaluator from
/// `task::Task` so that `query.rs` has no dependency on the `spaces` crate.
#[derive(Debug)]
pub struct QueryRule {
    /// The full rule — gives access to name, help, deps (raw), targets, etc.
    pub rule: rule::Rule,
    /// Pre-computed source path (from `labels::get_source_from_label`).
    pub source: String,
    /// Rule-name deps plus workspace-expanded glob file paths.
    /// Only populated if `QueryContextConfig::compute_expanded_deps` is true.
    pub expanded_deps: Option<Vec<Arc<str>>>,
    /// Pre-computed executor markdown fragment, used by `Export`.
    pub executor_markdown: Option<String>,
    /// Full task serialised as YAML, used by `Rule --format=yaml`.
    /// Only populated if `QueryContextConfig::compute_serialization` is true.
    pub serialized_yaml: Option<String>,
    /// Full task serialised as JSON, used by `Rule --format=json`.
    /// Only populated if `QueryContextConfig::compute_serialization` is true.
    pub serialized_json: Option<String>,
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
    /// Environment variables set via `--env=KEY=VALUE` during the original
    /// checkout. Used by the `Checkout` subcommand to reproduce the workspace.
    pub assign_from_arg_env: Vec<(Arc<str>, Arc<str>)>,
    /// Store values set via `--store=KEY=VALUE` during the original checkout.
    /// Used by the `Checkout` subcommand to reproduce the workspace.
    pub command_line_store: Vec<(Arc<str>, Arc<str>)>,
    /// Workspace-relative path where `spaces` was invoked; used to compute a
    /// default filter when none is supplied.
    pub relative_invoked_path: Arc<str>,
    /// The dependency graph for all rules. Only populated when needed.
    pub graph: Option<Arc<graph::Graph>>,
}

impl QueryCommand {
    /// Returns the stardoc base-directory path if this command is a stardoc
    /// export, otherwise returns `None`. Used by the argument handler to wire
    /// the stardoc collection pipeline before module evaluation.
    pub fn export_stardoc_path(&self) -> Option<Arc<str>> {
        if let QueryCommand::Export {
            path,
            format: Some(ExportFormat::Stardoc),
            ..
        } = self
        {
            return Some(path.clone());
        }
        None
    }

    /// Returns the configuration specifying which expensive fields are needed
    /// for this particular command variant.
    pub fn required_config(&self) -> QueryContextConfig {
        match self {
            QueryCommand::Rules { deps, raw, .. } => QueryContextConfig {
                // Need expanded_deps if --deps flag is set
                compute_expanded_deps: *deps,
                // Need serialization if --raw flag is set
                compute_serialization: *raw,
                compute_graph: false,
            },
            QueryCommand::Rule { deps, .. } => QueryContextConfig {
                compute_expanded_deps: *deps,
                compute_serialization: true,
                compute_graph: false,
            },
            QueryCommand::Search { deps, .. } => QueryContextConfig {
                compute_expanded_deps: *deps,
                compute_serialization: false,
                compute_graph: false,
            },
            QueryCommand::Checkout { .. } => {
                // Checkout only needs git tasks, no expensive rule data
                QueryContextConfig::minimal()
            }
            QueryCommand::Export { .. } => {
                // Export needs executor_markdown (always computed) but not deps/serialization
                QueryContextConfig::minimal()
            }
            QueryCommand::Graph { .. } => QueryContextConfig {
                compute_expanded_deps: false,
                compute_serialization: false,
                // Graph command needs the dependency graph
                compute_graph: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DependencyNode, build_dependency_tree, dependency_node_to_tree};
    use crate::{graph, search};
    use std::collections::HashSet;
    use std::sync::Arc;

    fn arc_terms(terms: &[&str]) -> Vec<Arc<str>> {
        terms.iter().map(|term| Arc::<str>::from(*term)).collect()
    }

    #[test]
    fn only_highlights_whole_search_terms() {
        let mask = search::keyword_highlight_mask("some search thing", &arc_terms(&["something"]));
        let highlighted: Vec<usize> = mask
            .into_iter()
            .enumerate()
            .filter_map(|(idx, highlighted)| highlighted.then_some(idx))
            .collect();

        assert!(highlighted.is_empty());
    }

    #[test]
    fn highlights_term_substrings_and_multiple_occurrences() {
        let mask = search::keyword_highlight_mask("tested tests test", &arc_terms(&["test"]));
        let highlighted: Vec<usize> = mask
            .into_iter()
            .enumerate()
            .filter_map(|(idx, highlighted)| highlighted.then_some(idx))
            .collect();

        assert_eq!(highlighted, vec![0, 1, 2, 3, 7, 8, 9, 10, 13, 14, 15, 16]);
    }

    #[test]
    fn no_panic_on_length_changing_lowercase() {
        // 'ß'.to_lowercase() == "ss" (1 char expands to 2), which previously caused an
        // out-of-bounds panic because highlights was sized from the original char count
        // while value_lower used the expanded length.
        let mask = search::keyword_highlight_mask("Straße", &arc_terms(&["straße"]));
        assert_eq!(mask.len(), "Straße".chars().count());
        // "Straße".to_ascii_lowercase() == "straße", so the full string should match.
        assert!(mask.iter().all(|&h| h));
    }

    #[test]
    fn merges_highlights_from_multiple_terms() {
        let mask = search::keyword_highlight_mask("build and test", &arc_terms(&["build", "test"]));
        let highlighted: Vec<usize> = mask
            .into_iter()
            .enumerate()
            .filter_map(|(idx, highlighted)| highlighted.then_some(idx))
            .collect();

        assert_eq!(highlighted, vec![0, 1, 2, 3, 4, 10, 11, 12, 13]);
    }

    #[test]
    fn test_build_dependency_tree_no_dependencies() {
        let mut graph = graph::Graph::default();
        graph.add_task("//pkg:standalone".into());

        let mut visited = HashSet::new();
        let tree = build_dependency_tree(&graph, "//pkg:standalone", &mut visited).unwrap();

        assert_eq!(tree.name.as_ref(), "//pkg:standalone");
        assert_eq!(tree.dependencies.len(), 0);
    }

    #[test]
    fn test_dependency_node_to_tree_simple() {
        let node = DependencyNode {
            name: "//root:main".into(),
            dependencies: vec![DependencyNode {
                name: "//pkg:dep".into(),
                dependencies: vec![],
            }],
        };

        let tree = dependency_node_to_tree(&node);
        let output = tree.to_string();

        assert!(output.contains("//root:main"));
        assert!(output.contains("//pkg:dep"));
    }

    #[test]
    fn test_dependency_node_serialization() {
        let node = DependencyNode {
            name: "//test:rule".into(),
            dependencies: vec![],
        };

        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("//test:rule"));
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// A node in the dependency tree used for JSON/YAML serialization
#[derive(Debug, Serialize)]
struct DependencyNode {
    name: Arc<str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<DependencyNode>,
}

/// Builds a dependency tree starting from a specific rule
fn build_dependency_tree(
    graph: &graph::Graph,
    rule_name: &str,
    visited: &mut HashSet<Arc<str>>,
) -> anyhow::Result<DependencyNode> {
    let rule_arc: Arc<str> = rule_name.into();

    // Check for cycles
    if visited.contains(&rule_arc) {
        return Ok(DependencyNode {
            name: rule_arc.clone(),
            dependencies: vec![],
        });
    }

    visited.insert(rule_arc.clone());

    let mut dependencies = Vec::new();

    // Get dependencies from the graph
    if let Ok(deps) = graph.get_dependencies(rule_name) {
        for dep_name in deps {
            // Filter out //:setup from the dependency graph
            if dep_name.as_ref() == "//:setup" {
                continue;
            }
            if let Ok(dep_node) = build_dependency_tree(graph, dep_name.as_ref(), visited) {
                dependencies.push(dep_node);
            }
        }
    }

    visited.remove(&rule_arc);

    Ok(DependencyNode {
        name: rule_arc,
        dependencies,
    })
}

/// Converts a DependencyNode into a termtree Tree for pretty printing
fn dependency_node_to_tree(node: &DependencyNode) -> Tree<Arc<str>> {
    let mut tree = Tree::new(node.name.clone());

    for dep in &node.dependencies {
        tree.push(dependency_node_to_tree(dep));
    }

    tree
}

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

/// Returns the default (globs, strip_prefix) pair derived from the workspace
/// invocation path when no explicit `--filter` is provided.
fn default_globs(relative_invoked_path: &str) -> (glob::Globs, Option<Arc<str>>) {
    if relative_invoked_path.is_empty() {
        (glob::Globs::default(), None)
    } else {
        let mut globs = glob::Globs::default();
        globs
            .includes
            .insert(format!("{relative_invoked_path}**").into());
        let strip_prefix = Some(format!("//{relative_invoked_path}").into());
        (globs, strip_prefix)
    }
}

/// Collect and optionally filter a set of `QueryRule`s into `(name, RuleInfo)`
/// pairs, applying glob filtering, `has_help`, and name prefix-stripping.
fn collect_rule_infos(
    rules: &[QueryRule],
    globs: &glob::Globs,
    strip_prefix: Option<&Arc<str>>,
    has_help: bool,
    deps: bool,
) -> HashMap<Arc<str>, RuleInfo> {
    let mut map: HashMap<Arc<str>, RuleInfo> = HashMap::new();

    for qr in rules {
        let raw_name = qr.rule.name.as_ref();

        if !search::matches_filter_in_any_field(globs, [raw_name]) {
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
            (qr.expanded_deps.clone(), Some(extract_targets(&qr.rule)))
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

/// Serialise a `HashMap<Arc<str>, RuleInfo>` to JSON.
fn serialise_rule_map_json(map: &HashMap<Arc<str>, RuleInfo>) -> anyhow::Result<String> {
    let mut s = serde_json::to_string_pretty(map).context(format_context!(
        "Internal Error: failed to serialize rule map for JSON"
    ))?;
    s.push('\n');
    Ok(s)
}

/// Serialise a sorted `HashMap<Arc<str>, RuleInfo>` to YAML.
fn serialise_rule_map_yaml(map: &HashMap<Arc<str>, RuleInfo>) -> anyhow::Result<String> {
    // Use an IndexMap to preserve sorted order in the YAML output.
    let mut sorted: IndexMap<&Arc<str>, &RuleInfo> = map.iter().collect();
    sorted.sort_keys();
    serde_yaml::to_string(&sorted).context(format_context!(
        "Internal Error: failed to serialize rule map for YAML"
    ))
}

fn make_name_line(name: &str, highlight_terms: Option<&[Arc<str>]>) -> console::Line {
    let mut line = console::Line::default();
    for (chunk, highlighted) in search::highlight_chunks(name, highlight_terms) {
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

fn normalize_help_text(help: &str) -> String {
    let mut help_lines = help.lines();
    if let Some(first) = help_lines.next() {
        std::iter::once(first)
            .chain(help_lines.map(str::trim_start))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    }
}

fn make_help_lines(help: &str, highlight_terms: Option<&[Arc<str>]>) -> Vec<console::Line> {
    help.lines()
        .map(|line| {
            let mut styled_line = console::Line::default();
            for (segment_idx, segment) in line.split('`').enumerate() {
                if segment.is_empty() {
                    continue;
                }

                let is_code = segment_idx % 2 == 1;
                for (chunk, highlighted) in search::highlight_chunks(segment, highlight_terms) {
                    if chunk.is_empty() {
                        continue;
                    }

                    if highlighted {
                        styled_line.push(console::Span::new_styled_lossy(StyledContent::new(
                            console::warning_style(),
                            chunk,
                        )));
                    } else if is_code {
                        styled_line.push(console::components::code(chunk));
                    } else {
                        styled_line.push(console::Span::new_unstyled_lossy(chunk));
                    }
                }
            }
            styled_line
        })
        .collect()
}

fn yaml_key_to_string(key: &serde_yaml::Value) -> anyhow::Result<String> {
    let key = match key {
        serde_yaml::Value::Null => "null".to_string(),
        serde_yaml::Value::Bool(v) => v.to_string(),
        serde_yaml::Value::Number(v) => v.to_string(),
        serde_yaml::Value::String(v) => v.clone(),
        _ => {
            let rendered = serde_yaml::to_string(key)
                .context(format_context!("Failed to serialize YAML key"))?;
            rendered
                .strip_prefix("---\n")
                .unwrap_or(&rendered)
                .trim_end()
                .to_string()
        }
    };
    Ok(key)
}

fn yaml_value_to_pretty_string(value: &serde_yaml::Value) -> anyhow::Result<String> {
    match value {
        serde_yaml::Value::Null => Ok("null".to_string()),
        serde_yaml::Value::Bool(v) => Ok(v.to_string()),
        serde_yaml::Value::Number(v) => Ok(v.to_string()),
        serde_yaml::Value::String(v) => Ok(v.clone()),
        _ => {
            let rendered = serde_yaml::to_string(value)
                .context(format_context!("Failed to serialize YAML value"))?;
            Ok(rendered
                .strip_prefix("---\n")
                .unwrap_or(&rendered)
                .trim_end()
                .to_string())
        }
    }
}

fn deps_to_unordered_items(value: &serde_yaml::Value) -> anyhow::Result<Vec<String>> {
    match value {
        serde_yaml::Value::Null => Ok(vec!["<None>".to_string()]),
        serde_yaml::Value::Sequence(seq) => {
            let mut items = Vec::new();
            for entry in seq {
                let rendered = yaml_value_to_pretty_string(entry)
                    .context(format_context!("Failed to render deps sequence item"))?;
                let collapsed = rendered
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                items.push(if collapsed.is_empty() {
                    "<None>".to_string()
                } else {
                    collapsed
                });
            }
            if items.is_empty() {
                Ok(vec!["<None>".to_string()])
            } else {
                Ok(items)
            }
        }
        serde_yaml::Value::Mapping(map) => {
            if map.len() == 1
                && let Some((tag_key, tagged_value)) = map.iter().next()
                && let serde_yaml::Value::String(tag) = tag_key
                && (tag == "Rules" || tag == "Any")
            {
                return deps_to_unordered_items(tagged_value);
            }

            let rendered = yaml_value_to_pretty_string(value)
                .context(format_context!("Failed to render deps mapping"))?;
            let collapsed = rendered
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            Ok(vec![if collapsed.is_empty() {
                "<None>".to_string()
            } else {
                collapsed
            }])
        }
        _ => {
            let rendered = yaml_value_to_pretty_string(value)
                .context(format_context!("Failed to render deps value"))?;
            Ok(vec![rendered])
        }
    }
}

fn emit_pretty_rule_details(
    console: &console::Console,
    qr: &QueryRule,
    include_deps: bool,
) -> anyhow::Result<()> {
    let serialized_yaml = qr.serialized_yaml.as_ref().ok_or_else(|| {
        format_error!(
            "Internal error: serialized_yaml not computed for rule {}",
            qr.rule.name.as_ref()
        )
    })?;

    let value: serde_yaml::Value = serde_yaml::from_str(serialized_yaml)
        .context(format_context!("Failed to parse rule YAML"))?;

    let header = console::components::Header::h1(qr.rule.name.as_ref())
        .variant(console::components::Variant::Primary);
    console.emit_lines(header.render());

    let mut digest: Option<String> = None;
    let mut phase: Option<String> = None;
    let mut rule_value: Option<&serde_yaml::Value> = None;
    let mut executor_value: Option<&serde_yaml::Value> = None;

    if let Some(map) = value.as_mapping() {
        for (key, field_value) in map {
            let key = yaml_key_to_string(key).context(format_context!(
                "Failed to render rule field key for {}",
                qr.rule.name.as_ref()
            ))?;

            match key.as_str() {
                "digest" => {
                    digest = Some(yaml_value_to_pretty_string(field_value).context(
                        format_context!("Failed to render digest for {}", qr.rule.name.as_ref()),
                    )?);
                }
                "phase" => {
                    phase = Some(yaml_value_to_pretty_string(field_value).context(
                        format_context!("Failed to render phase for {}", qr.rule.name.as_ref()),
                    )?);
                }
                "rule" => rule_value = Some(field_value),
                "executor" => executor_value = Some(field_value),
                _ => {}
            }
        }
    }

    let top_level = console::components::DescriptionList::new()
        .variant(console::components::Variant::Info)
        .compact(true)
        .item("source", qr.source.as_str())
        .item(
            "digest",
            digest.unwrap_or_else(|| "<Not Provided>".to_string()),
        )
        .item(
            "phase",
            phase.unwrap_or_else(|| "<Not Provided>".to_string()),
        );
    console.emit_lines(top_level.render());

    let rule_header = console::components::Header::h2("Rule Details")
        .variant(console::components::Variant::Primary);
    console.emit_lines(rule_header.render());

    let mut rule_details = console::components::DescriptionList::new()
        .variant(console::components::Variant::Info)
        .compact(true);

    if let Some(rule_value) = rule_value {
        if let Some(rule_map) = rule_value.as_mapping() {
            let mut deps_value: Option<&serde_yaml::Value> = None;

            for (key, field_value) in rule_map {
                let key = yaml_key_to_string(key).context(format_context!(
                    "Failed to render rule member key for {}",
                    qr.rule.name.as_ref()
                ))?;

                if key == "deps" {
                    deps_value = Some(field_value);
                    continue;
                }

                if key == "help" {
                    let help_text = field_value.as_str().unwrap_or("<Not Provided>");
                    let normalized_help = normalize_help_text(help_text);
                    rule_details =
                        rule_details.item("help", make_help_lines(&normalized_help, None));
                    continue;
                }

                let rendered_value =
                    yaml_value_to_pretty_string(field_value).context(format_context!(
                        "Failed to render rule field '{key}' for {}",
                        qr.rule.name.as_ref()
                    ))?;
                rule_details = rule_details.item(key, rendered_value);
            }

            if include_deps {
                let mut deps_list = console::components::List::unordered()
                    .variant(console::components::Variant::Info);
                let dep_items = if let Some(deps) = deps_value {
                    deps_to_unordered_items(deps).context(format_context!(
                        "Failed to render deps list for {}",
                        qr.rule.name.as_ref()
                    ))?
                } else {
                    vec!["<None>".to_string()]
                };

                for dep in dep_items {
                    deps_list = deps_list.item(dep);
                }

                let deps_header = console::components::Header::h3("Deps")
                    .variant(console::components::Variant::Info);
                console.emit_lines(deps_header.render());
                console.emit_lines(deps_list.render());
            }
        } else {
            let rendered_value =
                yaml_value_to_pretty_string(rule_value).context(format_context!(
                    "Failed to render rule details for {}",
                    qr.rule.name.as_ref()
                ))?;
            rule_details = rule_details.item("rule", rendered_value);
        }
    } else {
        rule_details = rule_details.item("rule", "<Not Provided>");
    }

    console.emit_lines(rule_details.render());

    if include_deps {
        let expanded_deps = qr.expanded_deps.as_ref().ok_or_else(|| {
            format_error!(
                "Internal error: expanded_deps not computed for rule {}",
                qr.rule.name.as_ref()
            )
        })?;
        let expanded_deps_value = serde_yaml::to_value(expanded_deps)
            .context(format_context!("Failed to serialize expanded_deps"))?;
        let expanded_deps_items = deps_to_unordered_items(&expanded_deps_value)
            .context(format_context!("Failed to render expanded_deps"))?;

        let mut expanded_deps_list =
            console::components::List::unordered().variant(console::components::Variant::Info);
        for dep in expanded_deps_items {
            expanded_deps_list = expanded_deps_list.item(dep);
        }

        console.emit_lines(console::components::h3("Expanded Deps"));
        console.emit_lines(expanded_deps_list.render());
    }

    let executor_header = console::components::Header::h2("Rule Executor")
        .variant(console::components::Variant::Primary);
    console.emit_lines(executor_header.render());

    let mut executor_details = console::components::DescriptionList::new()
        .variant(console::components::Variant::Info)
        .compact(true);

    if let Some(executor_value) = executor_value {
        if let Some(executor_map) = executor_value.as_mapping() {
            if executor_map.len() == 1 {
                let (type_key, data_value) = executor_map.iter().next().ok_or_else(|| {
                    format_error!(
                        "Internal error: failed to read executor mapping for {}",
                        qr.rule.name.as_ref()
                    )
                })?;

                let executor_type = yaml_key_to_string(type_key).context(format_context!(
                    "Failed to render executor type for {}",
                    qr.rule.name.as_ref()
                ))?;

                executor_details = executor_details.item("type", executor_type.clone());

                if executor_type == "Exec" {
                    if let Some(exec_map) = data_value.as_mapping() {
                        let command = exec_map
                            .iter()
                            .find_map(|(key, val)| {
                                if matches!(key, serde_yaml::Value::String(s) if s == "command") {
                                    Some(val)
                                } else {
                                    None
                                }
                            })
                            .and_then(|v| v.as_str())
                            .unwrap_or("<Not Provided>");

                        let mut tokens = vec![command.to_string()];
                        if let Some(args_value) = exec_map.iter().find_map(|(key, val)| {
                            if matches!(key, serde_yaml::Value::String(s) if s == "args") {
                                Some(val)
                            } else {
                                None
                            }
                        }) && let Some(args) = args_value.as_sequence()
                        {
                            for arg in args {
                                let rendered_arg =
                                    yaml_value_to_pretty_string(arg).context(format_context!(
                                        "Failed to render executor args for {}",
                                        qr.rule.name.as_ref()
                                    ))?;
                                tokens.push(rendered_arg);
                            }
                        }

                        let pretty_command = tokens
                            .into_iter()
                            .map(|token| {
                                if token
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric() || "-_./:=+".contains(c))
                                {
                                    token
                                } else {
                                    format!("{token:?}")
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ");

                        let mut pretty_line = console::Line::default();
                        pretty_line.push(console::bootstrap::code(pretty_command));
                        console.emit_line(pretty_line);

                        let env_header = console::components::Header::h3("Env")
                            .variant(console::components::Variant::Info);
                        console.emit_lines(env_header.render());

                        let mut env_list = console::components::List::unordered()
                            .variant(console::components::Variant::Info);
                        let mut has_env = false;
                        if let Some(env_value) = exec_map.iter().find_map(|(key, val)| {
                            if matches!(key, serde_yaml::Value::String(s) if s == "env") {
                                Some(val)
                            } else {
                                None
                            }
                        }) && let Some(env_map) = env_value.as_mapping()
                        {
                            for (env_key, env_val) in env_map {
                                let env_key =
                                    yaml_key_to_string(env_key).context(format_context!(
                                        "Failed to render executor env key for {}",
                                        qr.rule.name.as_ref()
                                    ))?;
                                let env_val = yaml_value_to_pretty_string(env_val).context(
                                    format_context!(
                                        "Failed to render executor env value for {}",
                                        qr.rule.name.as_ref()
                                    ),
                                )?;
                                env_list = env_list.item(format!("{env_key}={env_val}"));
                                has_env = true;
                            }
                        }

                        if !has_env {
                            env_list = env_list.item("<None>");
                        }
                        console.emit_lines(env_list.render());

                        for (key, field_value) in exec_map {
                            let key = yaml_key_to_string(key).context(format_context!(
                                "Failed to render executor field key for {}",
                                qr.rule.name.as_ref()
                            ))?;
                            if key == "command" || key == "args" || key == "env" {
                                continue;
                            }

                            let rendered_value = yaml_value_to_pretty_string(field_value).context(
                                format_context!(
                                    "Failed to render executor field '{key}' for {}",
                                    qr.rule.name.as_ref()
                                ),
                            )?;
                            executor_details = executor_details.item(key, rendered_value);
                        }
                    } else {
                        let rendered_value =
                            yaml_value_to_pretty_string(data_value).context(format_context!(
                                "Failed to render executor details for {}",
                                qr.rule.name.as_ref()
                            ))?;
                        executor_details = executor_details.item("value", rendered_value);
                    }
                } else if let Some(other_map) = data_value.as_mapping() {
                    for (key, field_value) in other_map {
                        let key = yaml_key_to_string(key).context(format_context!(
                            "Failed to render executor field key for {}",
                            qr.rule.name.as_ref()
                        ))?;
                        let rendered_value =
                            yaml_value_to_pretty_string(field_value).context(format_context!(
                                "Failed to render executor field '{key}' for {}",
                                qr.rule.name.as_ref()
                            ))?;
                        executor_details = executor_details.item(key, rendered_value);
                    }
                } else {
                    let rendered_value =
                        yaml_value_to_pretty_string(data_value).context(format_context!(
                            "Failed to render executor details for {}",
                            qr.rule.name.as_ref()
                        ))?;
                    executor_details = executor_details.item("value", rendered_value);
                }
            } else {
                let rendered_value =
                    yaml_value_to_pretty_string(executor_value).context(format_context!(
                        "Failed to render executor details for {}",
                        qr.rule.name.as_ref()
                    ))?;
                executor_details = executor_details.item("executor", rendered_value);
            }
        } else {
            let rendered_value =
                yaml_value_to_pretty_string(executor_value).context(format_context!(
                    "Failed to render executor details for {}",
                    qr.rule.name.as_ref()
                ))?;
            executor_details = executor_details.item("executor", rendered_value);
        }
    } else {
        executor_details = executor_details.item("executor", "<Not Provided>");
    }

    let details_header =
        console::components::Header::h3("Details").variant(console::components::Variant::Default);
    console.emit_lines(details_header.render());
    console.emit_lines(executor_details.render());
    console.emit_lines(console::bootstrap::VerticalSpacer::new(1).render());
    Ok(())
}

fn emit_styled_rule(
    console: &console::Console,
    name: &str,
    source: &str,
    help: &str,
    deps: Option<&Vec<Arc<str>>>,
    targets: Option<&Vec<Arc<str>>>,
    highlight_terms: Option<&[Arc<str>]>,
) {
    console.emit_line(make_name_line(name, highlight_terms));

    let normalized_help = normalize_help_text(help);
    let highlighted_help = make_help_lines(&normalized_help, highlight_terms);

    let mut description_list = console::components::DescriptionList::new()
        .variant(console::components::Variant::Info)
        .compact(true)
        .item("source", source)
        .item("help", highlighted_help);

    if let Some(deps) = deps {
        for dep in deps {
            description_list = description_list.item("dep", dep.as_ref());
        }
    }

    if let Some(targets) = targets {
        for target in targets {
            description_list = description_list.item("target", target.as_ref());
        }
    }

    console.emit_lines(description_list.render());
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
                    Some(f) => (search::build_filter_globs(f.as_ref()), None),
                    None => default_globs(ctx.relative_invoked_path.as_ref()),
                };

                if *raw {
                    let emit = |qr: &QueryRule| -> anyhow::Result<()> {
                        let raw_name = qr.rule.name.as_ref();
                        if !search::matches_filter_in_any_field(&globs, [raw_name]) {
                            return Ok(());
                        }
                        if *has_help && qr.rule.help.is_none() {
                            return Ok(());
                        }
                        if let Some(yaml) = &qr.serialized_yaml {
                            console.write(&format!("# {raw_name}\n{yaml}"))?;
                        }
                        Ok(())
                    };
                    if *checkout {
                        for qr in &ctx.checkout_rules {
                            emit(qr)?;
                        }
                    }
                    for qr in &ctx.run_rules {
                        emit(qr)?;
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
                        console::Format::Pretty => {
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
                                    None,
                                );
                            }
                        }
                        console::Format::Yaml => {
                            console.write(&serialise_rule_map_yaml(&map).context(
                                format_context!(
                                    "Internal Error: while serialising rule map for YAML"
                                ),
                            )?)?;
                        }
                        console::Format::Json => {
                            console.write(&serialise_rule_map_json(&map).context(
                                format_context!(
                                    "Internal Error: while serialising rule map for JSON"
                                ),
                            )?)?;
                        }
                    }
                }
                Ok(())
            }

            // ------------------------------------------------------------------
            QueryCommand::Rule {
                name,
                deps,
                checkout,
                format,
            } => {
                let qr = if *checkout {
                    ctx.checkout_rules
                        .iter()
                        .chain(ctx.run_rules.iter())
                        .find(|qr| qr.rule.name.as_ref() == name.as_ref())
                } else {
                    ctx.run_rules
                        .iter()
                        .find(|qr| qr.rule.name.as_ref() == name.as_ref())
                }
                .ok_or_else(|| format_error!("Rule not found: {name}"))?;

                if matches!(format, console::Format::Pretty) {
                    emit_pretty_rule_details(&console, qr, *deps)?;
                    return Ok(());
                }

                let serialized_yaml = qr.serialized_yaml.as_ref().ok_or_else(|| {
                    format_error!("Internal error: serialized_yaml not computed for rule {name}")
                })?;
                let serialized_json = qr.serialized_json.as_ref().ok_or_else(|| {
                    format_error!("Internal error: serialized_json not computed for rule {name}")
                })?;

                let output = if *deps {
                    let expanded_deps = qr.expanded_deps.as_ref().ok_or_else(|| {
                        format_error!("Internal error: expanded_deps not computed for rule {name}")
                    })?;
                    match format {
                        console::Format::Pretty => unreachable!(),
                        console::Format::Yaml => {
                            let mut value: serde_yaml::Value =
                                serde_yaml::from_str(serialized_yaml)
                                    .context(format_context!("Failed to parse rule YAML"))?;
                            if let Some(map) = value.as_mapping_mut() {
                                map.insert(
                                    serde_yaml::Value::String("expanded_deps".into()),
                                    serde_yaml::to_value(expanded_deps).context(
                                        format_context!("Failed to serialize expanded_deps"),
                                    )?,
                                );
                            }
                            serde_yaml::to_string(&value)
                                .context(format_context!("Failed to serialize rule YAML"))?
                        }
                        console::Format::Json => {
                            let mut value: serde_json::Value =
                                serde_json::from_str(serialized_json)
                                    .context(format_context!("Failed to parse rule JSON"))?;
                            if let Some(map) = value.as_object_mut() {
                                map.insert(
                                    "expanded_deps".into(),
                                    serde_json::to_value(expanded_deps).context(
                                        format_context!("Failed to serialize expanded_deps"),
                                    )?,
                                );
                            }
                            let mut s = serde_json::to_string_pretty(&value)
                                .context(format_context!("Failed to serialize rule JSON"))?;
                            s.push('\n');
                            s
                        }
                    }
                } else {
                    match format {
                        console::Format::Pretty => unreachable!(),
                        console::Format::Yaml => serialized_yaml.clone(),
                        console::Format::Json => serialized_json.clone(),
                    }
                };
                console.raw(&output)?;
                Ok(())
            }

            // ------------------------------------------------------------------
            QueryCommand::Search {
                query,
                deps,
                checkout,
                limit,
            } => {
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

                // Separate query terms into filters and search terms
                // - Terms starting with "//" are prefix filters for rule names
                // - Terms containing "/" are substring filters for source paths
                // - Terms containing ":" are substring filters for rule names
                // - Other terms are fuzzy-searched against name and help text
                let mut prefix_filters: Vec<&str> = Vec::new();
                let mut path_filters: Vec<&str> = Vec::new();
                let mut search_terms: Vec<&str> = Vec::new();

                for q in query.iter() {
                    let term = q.as_ref();
                    if term.starts_with("//") {
                        prefix_filters.push(term);
                    } else if term.contains('/') || term.contains(':') {
                        path_filters.push(term);
                    } else {
                        search_terms.push(term);
                    }
                }

                let mut score_rule = |qr: &QueryRule| {
                    let raw_name = qr.rule.name.as_ref();
                    let help_text = qr.rule.help.as_deref().unwrap_or("");

                    // Apply hard filters first - rule must pass all of these

                    // Filter 1: Rule name must start with "//" prefix if specified
                    for prefix in &prefix_filters {
                        if !raw_name.starts_with(prefix) {
                            return;
                        }
                    }

                    // Filter 2: Source path must contain "/" pattern if specified
                    for path_pattern in &path_filters {
                        if !raw_name.contains(path_pattern) {
                            return;
                        }
                    }

                    let mut total_score: isize = 0;
                    let mut matched_terms = 0;

                    // Aggregate score across all search terms (non-filter terms)
                    for query_term in &search_terms {
                        // Score against name (higher weight)
                        let name_score = search::score_match(query_term, raw_name);

                        // Score against help text (lower weight: 1/3 of name)
                        let help_score = search::score_match(query_term, help_text) / 3;

                        // Take the better of the two
                        let term_score = name_score.max(help_score);

                        if term_score > 0 {
                            total_score += term_score;
                            matched_terms += 1;
                        }
                    }

                    // If there are search terms, require at least one to match
                    // If there are only filters (no search terms), pass through with a default score
                    if !search_terms.is_empty() && matched_terms == 0 {
                        return;
                    }

                    // If there are no search terms (only filters), use a neutral default score
                    if search_terms.is_empty() {
                        total_score = 1000;
                        matched_terms = 1;
                    }

                    // Apply bonus for matching more terms (rewards broader matches)
                    let final_score = total_score * matched_terms as isize;

                    let (rule_deps, rule_targets) = if *deps {
                        (qr.expanded_deps.clone(), Some(extract_targets(&qr.rule)))
                    } else {
                        (None, None)
                    };

                    scored.push(Scored {
                        score: final_score,
                        name: qr.rule.name.clone(),
                        info: ScoredInfo {
                            source: qr.source.clone(),
                            help: help_text.to_string(),
                            deps: rule_deps,
                            targets: rule_targets,
                        },
                    });
                };

                if *checkout {
                    for qr in &ctx.checkout_rules {
                        score_rule(qr);
                    }
                }
                for qr in &ctx.run_rules {
                    score_rule(qr);
                }

                // Sort by score ascending (lowest first), then by name length, then alphabetically
                // This puts best matches at the bottom of the terminal output
                scored.sort_by(|a, b| {
                    a.score
                        .cmp(&b.score)
                        .then_with(|| b.name.len().cmp(&a.name.len()))
                        .then_with(|| b.name.cmp(&a.name))
                });

                // Take the top N results from the end (highest scores)
                let top: IndexMap<Arc<str>, ScoredInfo> = scored
                    .into_iter()
                    .rev()
                    .take(*limit)
                    .rev()
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
                            Some(query.as_slice()),
                        );
                    }

                    if let Some((best_name, _)) = top.iter().next_back() {
                        console.emit_lines(console::components::h3("Run Best Match:"));
                        let mut command = console::Line::default();
                        command.push(console::components::code(format!("spaces run {best_name}")));
                        console.emit_line(command);
                        console.emit_line(console::Line::default());
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
                    .execute_inspect_checkout(
                        console,
                        ctx.checkout_git_tasks.as_slice(),
                        ctx.assign_from_arg_env.as_slice(),
                        ctx.command_line_store.as_slice(),
                    )
                    .context(format_context!("while printing checkout command"))
            }

            // ------------------------------------------------------------------
            QueryCommand::Export {
                path,
                checkout,
                format,
            } => {
                let effective_format = match format {
                    Some(f) => f.clone(),
                    None => ExportFormat::infer_from_path(path.as_ref()),
                };

                if *checkout && matches!(effective_format, ExportFormat::Stardoc) {
                    return Err(format_error!(
                        "--checkout is not applicable to stardoc export"
                    ));
                }

                match effective_format {
                    ExportFormat::Markdown => {
                        let run_pairs: Vec<(&rule::Rule, Option<String>)> = ctx
                            .run_rules
                            .iter()
                            .map(|qr| (&qr.rule, qr.executor_markdown.clone()))
                            .collect();

                        let file_console = console::Console::new_file(path.as_ref())
                            .context(format_context!("Failed to create file {path}"))?;
                        let mut md = markdown::Markdown::new(file_console);
                        if *checkout {
                            let checkout_pairs: Vec<(&rule::Rule, Option<String>)> = ctx
                                .checkout_rules
                                .iter()
                                .map(|qr| (&qr.rule, qr.executor_markdown.clone()))
                                .collect();
                            rule::Rule::print_markdown_section(
                                &mut md,
                                "Checkout Rules",
                                &checkout_pairs,
                                false,
                                false,
                            )?;
                        }
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
                        // Documentation was already written to disk during module
                        // evaluation via workspace.stardoc.generate() inside
                        // evaluate_starlark_modules (triggered by setting
                        // inspect_options.stardoc in the arguments handler).
                        Ok(())
                    }
                }
            }

            // ------------------------------------------------------------------
            QueryCommand::Graph { rule, format } => {
                // 1. Build rules map from context
                let rules: Vec<&QueryRule> = ctx
                    .checkout_rules
                    .iter()
                    .chain(ctx.run_rules.iter())
                    .collect();

                let rules_map: HashMap<Arc<str>, &QueryRule> =
                    rules.iter().map(|r| (r.rule.name.clone(), *r)).collect();

                // 2. Verify rule exists
                if !rules_map.contains_key(rule) {
                    let available_rules: Vec<Arc<str>> = rules_map.keys().cloned().collect();
                    let suggestions = suggest::get_suggestions(rule.clone(), &available_rules);
                    let suggestions_str = suggestions
                        .iter()
                        .take(5)
                        .map(|(_, s)| s.as_ref())
                        .collect::<Vec<&str>>()
                        .join(", ");

                    anyhow::bail!(
                        "Rule '{}' not found. Similar rules: {}",
                        rule,
                        suggestions_str
                    );
                }

                // 3. Build dependency tree
                let graph = ctx
                    .graph
                    .as_ref()
                    .ok_or_else(|| format_error!("Dependency graph not available"))?;
                let mut visited = HashSet::new();
                let tree = build_dependency_tree(graph, rule.as_ref(), &mut visited)
                    .context(format_context!("Failed to build dependency tree"))?;

                // 4. Output in requested format
                match format {
                    console::Format::Json => {
                        let json = serde_json::to_string_pretty(&tree)
                            .context(format_context!("Failed to serialize to JSON"))?;
                        console.write(&format!("{}\n", json))?;
                    }
                    console::Format::Yaml => {
                        let yaml = serde_yaml::to_string(&tree)
                            .context(format_context!("Failed to serialize to YAML"))?;
                        console.write(&format!("{}\n", yaml))?;
                    }
                    console::Format::Pretty => {
                        // Count total dependencies

                        // Write header
                        console.emit_lines(
                            console::components::Header::h2("Dependency Graph")
                                .variant(console::components::Variant::Primary)
                                .render(),
                        );

                        // Write tree
                        let term_tree = dependency_node_to_tree(&tree);
                        console.write(&format!("{}\n", term_tree))?;
                    }
                }

                Ok(())
            }
        }
    }
}
