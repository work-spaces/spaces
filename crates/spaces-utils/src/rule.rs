use crate::{changes, platform};
use printer::markdown;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const SETUP_RULE_NAME: &str = "//:setup";
pub const TEST_RULE_NAME: &str = "//:test";
pub const PRE_COMMIT_RULE_NAME: &str = "//:pre-commit";
pub const CLEAN_RULE_NAME: &str = "//:clean";
pub const ALL_RULE_NAME: &str = "//:all";

// add pub enum Inputs with globs and envs
// add outputs as globs (includes/excludes)

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RuleType {
    Setup,
    Run,
    Test,
    PreCommit,
    Clean,
    Optional,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Visibility {
    /// Allows rules to be used anywhere.
    #[default]
    Public,
    /// Allows rules that start with the given prefixes.
    Rules(Vec<Arc<str>>),
    /// Allows the rules only to be used within the same file.
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Globs {
    Includes(HashSet<Arc<str>>),
    Excludes(HashSet<Arc<str>>),
}

impl Globs {
    pub fn to_changes_globs(items: &[Globs]) -> changes::glob::Globs {
        let mut globs = changes::glob::Globs::default();
        for item in items {
            match item {
                Globs::Includes(set) => globs.includes.extend(set.iter().cloned()),
                Globs::Excludes(set) => globs.excludes.extend(set.iter().cloned()),
            }
        }
        globs
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnyDep {
    Rule(Arc<str>),
    Glob(Globs),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Deps {
    Rules(Vec<Arc<str>>),
    Any(Vec<AnyDep>),
}

impl Default for Deps {
    fn default() -> Self {
        Self::Any(Vec::new())
    }
}

impl Deps {
    /// Returns a reference to the rules list if this is the `Rules` variant, or `None`.
    /// For the `Any` variant, returns `None` (use `all_rules` to collect rules from `Any`).
    pub fn rules(&self) -> Option<&Vec<Arc<str>>> {
        match self {
            Deps::Rules(rules) => Some(rules),
            Deps::Any(_) => None,
        }
    }

    /// Returns a mutable reference to the rules list if this is the `Rules` variant, or `None`.
    /// For the `Any` variant, returns `None` (use `all_rules_mut` to mutate rules within `Any`).
    pub fn rules_mut(&mut self) -> Option<&mut Vec<Arc<str>>> {
        match self {
            Deps::Rules(rules) => Some(rules),
            Deps::Any(_) => None,
        }
    }

    /// Returns all rule names from the `Any` variant's `AnyDep::Rule` entries.
    pub fn any_rules(&self) -> Vec<&Arc<str>> {
        match self {
            Deps::Rules(_) => Vec::new(),
            Deps::Any(list) => list
                .iter()
                .filter_map(|entry| match entry {
                    AnyDep::Rule(rule) => Some(rule),
                    _ => None,
                })
                .collect(),
        }
    }

    /// Returns mutable references to all rules from the `Any` variant's `AnyDep::Rule` entries.
    pub fn any_rules_mut(&mut self) -> Vec<&mut Arc<str>> {
        match self {
            Deps::Rules(_) => Vec::new(),
            Deps::Any(list) => list
                .iter_mut()
                .filter_map(|entry| match entry {
                    AnyDep::Rule(rule) => Some(rule),
                    _ => None,
                })
                .collect(),
        }
    }

    /// Returns mutable references to all globs from the `Any` variant's `AnyDep::Glob` entries.
    pub fn any_globs_mut(&mut self) -> Vec<&mut Globs> {
        match self {
            Deps::Rules(_) => Vec::new(),
            Deps::Any(list) => list
                .iter_mut()
                .filter_map(|entry| match entry {
                    AnyDep::Glob(glob) => Some(glob),
                    _ => None,
                })
                .collect(),
        }
    }

    /// Returns true if this is the `Rules` variant and the list is empty,
    /// or if this is the `Any` variant and the list is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Deps::Rules(rules) => rules.is_empty(),
            Deps::Any(any) => any.is_empty(),
        }
    }

    /// Ensures this is a `Rules` variant, converting from a default if needed,
    /// and pushes a rule name onto it.
    pub fn push_rule(&mut self, rule: Arc<str>) {
        match self {
            Deps::Rules(rules) => rules.push(rule),
            Deps::Any(_) => {
                // Any variant cannot directly hold rule names at the top level; this is a no-op
            }
        }
    }

    /// Returns true if this is the `Rules` variant and contains the given rule name,
    /// or if this is the `Any` variant and any `AnyDep::Rule` entry contains the rule.
    pub fn contains_rule(&self, rule: &Arc<str>) -> bool {
        match self {
            Deps::Rules(rules) => rules.contains(rule),
            Deps::Any(list) => list.iter().any(|entry| match entry {
                AnyDep::Rule(r) => r == rule,
                _ => false,
            }),
        }
    }

    /// Returns all rule names from both `Rules` and `Any(AnyDep::Rule)` variants.
    pub fn collect_all_rules(&self) -> Vec<Arc<str>> {
        match self {
            Deps::Rules(rules) => rules.clone(),
            Deps::Any(list) => list
                .iter()
                .filter_map(|entry| match entry {
                    AnyDep::Rule(rule) => Some(rule.clone()),
                    _ => None,
                })
                .collect(),
        }
    }

    /// Inserts an `AnyDep` entry into deps without clobbering existing entries.
    /// Converts `Deps::Rules` to `Deps::Any` if needed to accommodate the new entry.
    pub fn push_any_dep(deps: &mut Option<Deps>, dep: AnyDep) {
        match deps.take() {
            Some(Deps::Rules(rules)) => {
                let mut any: Vec<AnyDep> = rules.into_iter().map(AnyDep::Rule).collect();
                any.push(dep);
                *deps = Some(Deps::Any(any));
            }
            Some(Deps::Any(mut any)) => {
                any.push(dep);
                *deps = Some(Deps::Any(any));
            }
            None => {
                *deps = Some(Deps::Any(vec![dep]));
            }
        }
    }

    /// Inserts multiple `AnyDep` entries into deps without clobbering existing entries.
    /// Converts `Deps::Rules` to `Deps::Any` if needed to accommodate the new entries.
    pub fn push_any_deps(deps: &mut Option<Deps>, new_deps: Vec<AnyDep>) {
        match deps.take() {
            Some(Deps::Rules(rules)) => {
                let mut any: Vec<AnyDep> = rules.into_iter().map(AnyDep::Rule).collect();
                any.extend(new_deps);
                *deps = Some(Deps::Any(any));
            }
            Some(Deps::Any(mut any)) => {
                any.extend(new_deps);
                *deps = Some(Deps::Any(any));
            }
            None => {
                *deps = Some(Deps::Any(new_deps));
            }
        }
    }

    /// Returns true if the deps have globs (either `Any` variant containing `AnyDep::Glob`).
    pub fn has_globs(&self) -> bool {
        match self {
            Deps::Rules(_) => false,
            Deps::Any(list) => list.iter().any(|entry| matches!(entry, AnyDep::Glob(_))),
        }
    }

    /// Returns all `Globs` entries collected from `AnyDep::Glob` within the `Any` variant.
    pub fn collect_globs(&self) -> Vec<Globs> {
        match self {
            Deps::Rules(_) => Vec::new(),
            Deps::Any(any_list) => any_list
                .iter()
                .filter_map(|entry| match entry {
                    AnyDep::Glob(glob) => Some(glob.clone()),
                    _ => None,
                })
                .collect(),
        }
    }
}

/// A rule desribes what a task should do.
/// It specifies named depedencies that must be executed
/// before the task can run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// workspace unique name of the rule
    pub name: Arc<str>,
    /// list of rule dependencies by name, or file-based deps with includes/excludes
    pub deps: Option<Deps>,
    /// help text displayed to the user when running inspect - use markdown format
    pub help: Option<Arc<str>>,
    /// list of globs that must have a change to re-run the rule (deprecated: use deps with Globs)
    pub inputs: Option<HashSet<Arc<str>>>,
    /// Not used
    pub outputs: Option<HashSet<Arc<str>>>,
    /// list of platforms that the rule will run on. default is to run on all platforms
    pub platforms: Option<Vec<platform::Platform>>,
    /// The type for the rule in the run phase
    #[serde(rename = "type")]
    pub type_: Option<RuleType>,
    /// The visibility of the rule
    pub visibility: Option<Visibility>,
}

type RuleMap = HashMap<Arc<str>, (Rule, Option<String>)>;

struct Section {
    name: Arc<str>,
    rules: Vec<Arc<str>>,
}

impl Rule {
    pub fn sanitize(&mut self) {
        // Convert Deps::Rules to Deps::Any with individual AnyDep::Rule entries
        if let Some(Deps::Rules(rules)) = self.deps.take()
            && !rules.is_empty()
        {
            self.deps = Some(Deps::Any(rules.into_iter().map(AnyDep::Rule).collect()));
        }

        // Pull any glob values from inputs into deps as Deps::Any with AnyDep::Glob
        if let Some(hash_set) = self.inputs.take() {
            let mut includes = HashSet::new();
            let mut excludes = HashSet::new();

            // Annotated set: +prefix means include, -prefix means exclude
            for item in hash_set {
                if let Some(stripped) = item.strip_prefix('+') {
                    includes.insert(Arc::from(stripped));
                } else if let Some(stripped) = item.strip_prefix('-') {
                    excludes.insert(Arc::from(stripped));
                }
            }

            if !includes.is_empty() || !excludes.is_empty() {
                let mut globs = Vec::new();
                if !includes.is_empty() {
                    globs.push(AnyDep::Glob(Globs::Includes(includes)));
                }
                if !excludes.is_empty() {
                    globs.push(AnyDep::Glob(Globs::Excludes(excludes)));
                }

                Deps::push_any_deps(&mut self.deps, globs);
            }
        }
    }

    fn get_hash_map(rules: &[(&Rule, Option<String>)]) -> RuleMap {
        let mut map = HashMap::new();
        for (rule, details) in rules {
            map.insert(rule.name.clone(), ((*rule).clone(), details.clone()));
        }
        map
    }

    fn get_sections(rules: &[(&Rule, Option<String>)]) -> Vec<Section> {
        let mut sections = HashMap::new();
        for (rule, _) in rules {
            let mut parts = rule.name.split(':');
            let section_name: Arc<str> = parts.next().unwrap_or_default().into();
            let rule_name: Arc<str> = parts.next().unwrap_or_default().into();
            let section = sections
                .entry(section_name.clone())
                .or_insert_with(|| Section {
                    name: section_name,
                    rules: Vec::new(),
                });
            section.rules.push(rule_name);
        }
        let mut sections = sections.into_values().collect::<Vec<_>>();
        sections.sort_by(|a, b| a.name.cmp(&b.name));
        for section in sections.iter_mut() {
            section.rules.sort();
        }
        sections
    }

    pub fn print_markdown_section(
        md: &mut markdown::Markdown,
        section_name: &str,
        rules: &[(&Rule, Option<String>)],
        show_has_help: bool,
        is_run_rules: bool,
    ) -> anyhow::Result<()> {
        let rule_map = Self::get_hash_map(rules);
        md.heading(2, section_name)?;
        let sections = Self::get_sections(rules);
        for section in sections.iter() {
            md.heading(3, section.name.as_ref())?;
            for rule_name in section.rules.iter() {
                if let Some((rule, details)) =
                    rule_map.get(format!("{}:{rule_name}", section.name).as_str())
                    && (!show_has_help || rule.help.is_some())
                {
                    md.heading(4, rule_name)?;
                    rule.print_markdown(md, details.to_owned(), is_run_rules)?;
                }
            }
        }
        md.printer.newline()?;
        Ok(())
    }

    fn print_markdown(
        &self,
        md: &mut markdown::Markdown,
        details: Option<String>,
        is_run_rule: bool,
    ) -> anyhow::Result<()> {
        md.hline()?;
        if is_run_rule {
            let spaces_run_example = format!("spaces run {}", self.name);
            md.code_block("sh", spaces_run_example.as_str())?;
            md.printer.newline()?;
        }
        if let Some(help) = &self.help {
            md.bold("Description")?;
            md.printer.newline()?;
            md.printer.newline()?;
            md.paragraph(help)?;
            md.printer.newline()?;
        } else if is_run_rule {
            md.paragraph("No help text provided")?;
            md.printer.newline()?;
        }

        if let Some(details) = details {
            md.bold("Details")?;
            md.printer.newline()?;
            md.printer.newline()?;
            md.paragraph(details.as_str())?;
            md.printer.newline()?;
        }

        if let Some(deps) = self.deps.as_ref()
            && !deps.is_empty()
        {
            md.bold("Dependencies")?;
            md.printer.newline()?;
            md.printer.newline()?;
            match deps {
                Deps::Rules(rules) => {
                    for dep in rules {
                        // get the rule using the dep as the name
                        md.list_item(0, dep)?;
                    }
                }
                Deps::Any(any_deps) => {
                    for entry in any_deps {
                        match entry {
                            AnyDep::Rule(rule) => {
                                md.list_item(0, rule)?;
                            }
                            AnyDep::Glob(glob) => match glob {
                                Globs::Includes(set) => {
                                    for item in set {
                                        md.list_item(0, &format!("+{item}"))?;
                                    }
                                }
                                Globs::Excludes(set) => {
                                    for item in set {
                                        md.list_item(0, &format!("-{item}"))?;
                                    }
                                }
                            },
                        }
                    }
                }
            }
            md.printer.newline()?;
        }

        Ok(())
    }
}
