use crate::{changes::glob, graph, inspect, markdown, rule, suggest, targets};
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
  - `spaces query rules --filter='**/my-pkg:*'`: filter by glob pattern
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
    #[command(about = r"Search for rules using fuzzy matching.
  - `spaces query search build`: return top 10 matches for 'build'
  - `spaces query search build test`: return top 10 matches across all terms
  - `spaces query search build --deps`: include expanded deps and targets in results
  - `spaces query search build --limit=20`: return top 20 matches
  - `spaces query search build --checkout`: include checkout-phase rules in search")]
    Search {
        /// One or more search terms; a rule matches if any term matches
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
    #[command(about = r"Export workspace documentation to a file.
  - `spaces query export ./docs/rules.md`: export as markdown
  - `spaces query export ./api.star --format=stardoc`: export as stardoc
  - `spaces query export ./docs/rules.md --checkout`: include checkout-phase rules
  - Format is inferred from the file extension when not specified (.md → markdown, .star/.bzl → stardoc)")]
    Export {
        /// Output file path
        path: Arc<str>,
        /// Include checkout-phase rules in export
        #[arg(long)]
        checkout: bool,
        /// Export format (inferred from file extension when omitted)
        #[arg(long, value_enum)]
        format: Option<ExportFormat>,
    },
    #[command(about = r"Show dependency graph for a specific rule.
  - `spaces query graph //my-pkg:build`: show dependency tree for the rule
  - `spaces query graph //my-pkg:build --format=json`: output as JSON
  - `spaces query graph //my-pkg:build --format=yaml`: output as YAML
  - `spaces query graph //my-pkg:build --checkout`: include checkout-phase rules")]
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
}

impl QueryContextConfig {
    /// Create a config that computes everything (legacy behavior).
    pub fn all() -> Self {
        Self {
            compute_expanded_deps: true,
            compute_serialization: true,
        }
    }

    /// Create a minimal config that computes nothing expensive.
    pub fn minimal() -> Self {
        Self {
            compute_expanded_deps: false,
            compute_serialization: false,
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
    /// Workspace-relative path where `spaces` was invoked; used to compute a
    /// default filter when none is supplied.
    pub relative_invoked_path: Arc<str>,
    /// The dependency graph for all rules.
    pub graph: Arc<graph::Graph>,
}

impl QueryCommand {
    /// Returns the configuration specifying which expensive fields are needed
    /// for this particular command variant.
    pub fn required_config(&self) -> QueryContextConfig {
        match self {
            QueryCommand::Rules { deps, raw, .. } => QueryContextConfig {
                // Need expanded_deps if --deps flag is set
                compute_expanded_deps: *deps,
                // Need serialization if --raw flag is set
                compute_serialization: *raw,
            },
            QueryCommand::Rule { deps, .. } => QueryContextConfig {
                compute_expanded_deps: *deps,
                compute_serialization: true,
            },
            QueryCommand::Search { deps, .. } => QueryContextConfig {
                compute_expanded_deps: *deps,
                compute_serialization: false,
            },
            QueryCommand::Checkout { .. } => {
                // Checkout only needs git tasks, no expensive rule data
                QueryContextConfig::minimal()
            }
            QueryCommand::Export { .. } => {
                // Export needs executor_markdown (always computed) but not deps/serialization
                QueryContextConfig::minimal()
            }
            QueryCommand::Graph { .. } => {
                // Graph only needs the graph structure, no expensive rule data
                QueryContextConfig::minimal()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DependencyNode, QueryRule, build_dependency_tree, dependency_node_to_tree,
        query_highlight_mask,
    };
    use crate::{graph, rule};
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    fn arc_terms(terms: &[&str]) -> Vec<Arc<str>> {
        terms.iter().map(|term| Arc::<str>::from(*term)).collect()
    }

    #[test]
    fn only_highlights_whole_search_terms() {
        let mask = query_highlight_mask("some search thing", &arc_terms(&["something"]));
        let highlighted: Vec<usize> = mask
            .into_iter()
            .enumerate()
            .filter_map(|(idx, highlighted)| highlighted.then_some(idx))
            .collect();

        assert!(highlighted.is_empty());
    }

    #[test]
    fn highlights_term_substrings_and_multiple_occurrences() {
        let mask = query_highlight_mask("tested tests test", &arc_terms(&["test"]));
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
        let mask = query_highlight_mask("Straße", &arc_terms(&["straße"]));
        assert_eq!(mask.len(), "Straße".chars().count());
        // "Straße".to_ascii_lowercase() == "straße", so the full string should match.
        assert!(mask.iter().all(|&h| h));
    }

    #[test]
    fn merges_highlights_from_multiple_terms() {
        let mask = query_highlight_mask("build and test", &arc_terms(&["build", "test"]));
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

        // Create a mock QueryRule
        let rule = QueryRule {
            rule: rule::Rule {
                name: "//pkg:standalone".into(),
                deps: None,
                help: None,
                inputs: None,
                outputs: None,
                targets: None,
                platforms: None,
                type_: Some(rule::RuleType::Run),
                visibility: None,
            },
            source: "pkg/standalone.star".to_string(),
            expanded_deps: None,
            executor_markdown: None,
            serialized_yaml: None,
            serialized_json: None,
        };

        let mut rules_map = HashMap::new();
        rules_map.insert("//pkg:standalone".into(), &rule);

        let mut visited = HashSet::new();
        let tree =
            build_dependency_tree(&graph, "//pkg:standalone", &rules_map, &mut visited).unwrap();

        assert_eq!(tree.name.as_ref(), "//pkg:standalone");
        assert_eq!(tree.dependencies.len(), 0);
        assert_eq!(tree.source, Some("pkg/standalone.star".to_string()));
    }

    #[test]
    fn test_dependency_node_to_tree_simple() {
        let node = DependencyNode {
            name: "//root:main".into(),
            source: Some("root/main.star".to_string()),
            dependencies: vec![DependencyNode {
                name: "//pkg:dep".into(),
                source: Some("pkg/dep.star".to_string()),
                dependencies: vec![],
            }],
        };

        let tree = dependency_node_to_tree(&node);
        let output = tree.to_string();

        assert!(output.contains("//root:main"));
        assert!(output.contains("//pkg:dep"));
        assert!(output.contains("root/main.star"));
        assert!(output.contains("pkg/dep.star"));
    }

    #[test]
    fn test_dependency_node_serialization() {
        let node = DependencyNode {
            name: "//test:rule".into(),
            source: Some("test/rule.star".to_string()),
            dependencies: vec![],
        };

        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("//test:rule"));
        assert!(json.contains("test/rule.star"));
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// A node in the dependency tree used for JSON/YAML serialization
#[derive(Debug, Serialize)]
struct DependencyNode {
    name: Arc<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<DependencyNode>,
}

/// Builds a dependency tree starting from a specific rule
fn build_dependency_tree(
    graph: &graph::Graph,
    rule_name: &str,
    rules_map: &HashMap<Arc<str>, &QueryRule>,
    visited: &mut HashSet<Arc<str>>,
) -> anyhow::Result<DependencyNode> {
    let rule = rules_map
        .get(rule_name)
        .ok_or_else(|| format_error!("Rule '{}' not found", rule_name))?;

    let rule_arc: Arc<str> = rule_name.into();

    // Check for cycles
    if visited.contains(&rule_arc) {
        return Ok(DependencyNode {
            name: rule_arc.clone(),
            source: Some(format!("{} (circular reference)", rule.source)),
            dependencies: vec![],
        });
    }

    visited.insert(rule_arc.clone());

    let mut dependencies = Vec::new();

    // Get dependencies from the graph
    if let Ok(deps) = graph.get_dependencies(rule_name) {
        for dep_name in deps {
            if let Ok(dep_node) =
                build_dependency_tree(graph, dep_name.as_ref(), rules_map, visited)
            {
                dependencies.push(dep_node);
            }
        }
    }

    visited.remove(&rule_arc);

    Ok(DependencyNode {
        name: rule_arc,
        source: Some(rule.source.clone()),
        dependencies,
    })
}

/// Converts a DependencyNode into a termtree Tree for pretty printing
fn dependency_node_to_tree(node: &DependencyNode) -> Tree<String> {
    let node_label = if let Some(source) = &node.source {
        format!("{} ({})", node.name, source)
    } else {
        node.name.to_string()
    };

    let mut tree = Tree::new(node_label);

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

fn query_highlight_mask(value: &str, query: &[Arc<str>]) -> Vec<bool> {
    // ASCII-only folding preserves char count, so value_lower.len() == value.chars().count().
    let value_lower: Vec<char> = value.to_ascii_lowercase().chars().collect();
    let mut highlights = vec![false; value_lower.len()];

    for term in query {
        let term_lower: Vec<char> = term.as_ref().to_ascii_lowercase().chars().collect();
        if term_lower.is_empty() || term_lower.len() > value_lower.len() {
            continue;
        }

        for start in 0..=value_lower.len() - term_lower.len() {
            if value_lower[start..start + term_lower.len()] == term_lower[..] {
                for h in &mut highlights[start..start + term_lower.len()] {
                    *h = true;
                }
            }
        }
    }
    highlights
}

fn push_highlighted_value(
    line: &mut console::Line,
    value: &str,
    highlight_terms: Option<&[Arc<str>]>,
) {
    let Some(highlight_terms) = highlight_terms.filter(|terms| !terms.is_empty()) else {
        line.push(console::Span::new_unstyled_lossy(value));
        return;
    };

    let chars: Vec<char> = value.chars().collect();
    let highlights = query_highlight_mask(value, highlight_terms);
    if !highlights.iter().any(|highlighted| *highlighted) {
        line.push(console::Span::new_unstyled_lossy(value));
        return;
    }

    let mut current_highlighted = highlights[0];
    let mut chunk = String::new();
    for (ch, highlighted) in chars.into_iter().zip(highlights) {
        if highlighted != current_highlighted {
            if current_highlighted {
                line.push(console::Span::new_styled_lossy(StyledContent::new(
                    console::keyword_style(),
                    std::mem::take(&mut chunk),
                )));
            } else {
                line.push(console::Span::new_unstyled_lossy(std::mem::take(
                    &mut chunk,
                )));
            }
            current_highlighted = highlighted;
        }
        chunk.push(ch);
    }

    if current_highlighted {
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::keyword_style(),
            chunk,
        )));
    } else {
        line.push(console::Span::new_unstyled_lossy(chunk));
    }
}

fn make_name_line(name: &str, highlight_terms: Option<&[Arc<str>]>) -> console::Line {
    let mut line = console::Line::default();
    let Some(highlight_terms) = highlight_terms.filter(|terms| !terms.is_empty()) else {
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::name_style(),
            name.to_owned(),
        )));
        return line;
    };

    let chars: Vec<char> = name.chars().collect();
    let highlights = query_highlight_mask(name, highlight_terms);
    if !highlights.iter().any(|highlighted| *highlighted) {
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::name_style(),
            name.to_owned(),
        )));
        return line;
    }

    let mut current_highlighted = highlights[0];
    let mut chunk = String::new();
    for (ch, highlighted) in chars.into_iter().zip(highlights) {
        if highlighted != current_highlighted {
            if current_highlighted {
                line.push(console::Span::new_styled_lossy(StyledContent::new(
                    console::keyword_style(),
                    std::mem::take(&mut chunk),
                )));
            } else {
                line.push(console::Span::new_styled_lossy(StyledContent::new(
                    console::name_style(),
                    std::mem::take(&mut chunk),
                )));
            }
            current_highlighted = highlighted;
        }
        chunk.push(ch);
    }

    if current_highlighted {
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::keyword_style(),
            chunk,
        )));
    } else {
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::name_style(),
            chunk,
        )));
    }
    line
}

fn make_kv_line(key: &str, value: &str, highlight_terms: Option<&[Arc<str>]>) -> console::Line {
    let mut line = console::Line::default();
    line.push(console::Span::new_styled_lossy(StyledContent::new(
        console::key_style(),
        format!("  {key:<8}"),
    )));
    push_highlighted_value(&mut line, value, highlight_terms);
    line
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
    console.emit_line(make_kv_line("source", source, None));
    let help_lines: Vec<&str> = help.lines().collect();
    if let Some((first, rest)) = help_lines.split_first() {
        console.emit_line(make_kv_line("help", first, highlight_terms));
        for continuation in rest {
            let mut line = console::Line::default();
            let continuation = continuation.trim_start();
            line.push(console::Span::new_unstyled_lossy("          "));
            push_highlighted_value(&mut line, continuation, highlight_terms);
            console.emit_line(line);
        }
    } else {
        console.emit_line(make_kv_line("help", "", highlight_terms));
    }
    if let Some(deps) = deps {
        for dep in deps {
            console.emit_line(make_kv_line("dep", dep.as_ref(), None));
        }
    }
    if let Some(targets) = targets {
        for target in targets {
            console.emit_line(make_kv_line("target", target.as_ref(), None));
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
                    let emit = |qr: &QueryRule| -> anyhow::Result<()> {
                        let raw_name = qr.rule.name.as_ref();
                        if !globs.is_empty()
                            && !glob_filter
                                .is_match(raw_name.strip_prefix("//").unwrap_or(raw_name))
                        {
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
                    let rule_deps = if *deps {
                        qr.expanded_deps.as_ref()
                    } else {
                        None
                    };
                    let targets = extract_targets(&qr.rule);
                    let rule_targets = if *deps { Some(&targets) } else { None };
                    emit_styled_rule(
                        &console,
                        qr.rule.name.as_ref(),
                        &qr.source,
                        qr.rule.help.as_deref().unwrap_or("<Not Provided>"),
                        rule_deps,
                        rule_targets,
                        None,
                    );
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

                let mut score_rule = |qr: &QueryRule| {
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
                            (qr.expanded_deps.clone(), Some(extract_targets(&qr.rule)))
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
                };

                if *checkout {
                    for qr in &ctx.checkout_rules {
                        score_rule(qr);
                    }
                }
                for qr in &ctx.run_rules {
                    score_rule(qr);
                }

                scored.sort_by_key(|b| std::cmp::Reverse(b.score));
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
                            Some(query.as_slice()),
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
            QueryCommand::Export {
                path,
                checkout,
                format,
            } => {
                let effective_format = match format {
                    Some(f) => f.clone(),
                    None => ExportFormat::infer_from_path(path.as_ref()),
                };

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
                        Err(format_error!("Stardoc export is not yet implemented"))
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
                let mut visited = HashSet::new();
                let tree =
                    build_dependency_tree(&ctx.graph, rule.as_ref(), &rules_map, &mut visited)
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
                        let term_tree = dependency_node_to_tree(&tree);
                        console.write(&format!("{}\n", term_tree))?;
                    }
                }

                Ok(())
            }
        }
    }
}
