use crate::{deps, platform, targets};
use printer::markdown;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub use deps::{AnyDep, Deps, Globs};

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
    /// Not used - use targets
    pub outputs: Option<HashSet<Arc<str>>>,
    /// The targets that the rule creates
    pub targets: Option<Vec<targets::Target>>,
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
        if let Some(Deps::Rules(rules)) = self.deps.take() {
            if !rules.is_empty() {
                self.deps = Some(Deps::Any(rules.into_iter().map(AnyDep::Rule).collect()));
            }
        }

        // Pull any glob values from inputs into deps as Deps::Any with AnyDep::Glob
        if let Some(hash_set) = self.inputs.take() {
            let mut includes = Vec::new();
            let mut excludes = Vec::new();

            // Annotated set: +prefix means include, -prefix means exclude
            for item in hash_set {
                if let Some(stripped) = item.strip_prefix('+') {
                    includes.push(Arc::from(stripped));
                } else if let Some(stripped) = item.strip_prefix('-') {
                    excludes.push(Arc::from(stripped));
                }
            }

            if !includes.is_empty() || !excludes.is_empty() {
                let mut globs = Vec::new();
                if !includes.is_empty() {
                    globs.push(AnyDep::Globs(Globs::Includes(includes)));
                }
                if !excludes.is_empty() {
                    globs.push(AnyDep::Globs(Globs::Excludes(excludes)));
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

    pub fn push_target(&mut self, target: targets::Target) {
        self.targets.get_or_insert_with(Vec::new).push(target);
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
                            AnyDep::Globs(glob) => match glob {
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
                            AnyDep::Target(target) => {
                                md.list_item(0, &format!("{}:{}", target.rule, target.target))?;
                            }
                        }
                    }
                }
            }
            md.printer.newline()?;
        }

        Ok(())
    }
}
