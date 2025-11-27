use printer::markdown;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const SETUP_RULE_NAME: &str = "//:setup";
pub const TEST_RULE_NAME: &str = "//:test";
pub const CLEAN_RULE_NAME: &str = "//:clean";
pub const ALL_RULE_NAME: &str = "//:all";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RuleType {
    Setup,
    Run,
    Test,
    Clean,
    Optional,
}

/// A rule desribes what a task should do.
/// It specifies named depedencies that must be executed
/// before the task can run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// workspace unique name of the rule
    pub name: Arc<str>,
    /// list of rule dependencies by name
    pub deps: Option<Vec<Arc<str>>>,
    /// help text displayed to the user when running inspect - use markdown format
    pub help: Option<Arc<str>>,
    /// list of globs that must have a change to re-run the rule
    pub inputs: Option<HashSet<Arc<str>>>,
    /// No used
    pub outputs: Option<HashSet<Arc<str>>>,
    /// list of platforms that the rule will run on. default is to run on all platforms
    pub platforms: Option<Vec<platform::Platform>>,
    /// The type for the rule in the run phase
    #[serde(rename = "type")]
    pub type_: Option<RuleType>,
}

type RuleMap = HashMap<Arc<str>, (Rule, Option<String>)>;

struct Section {
    name: Arc<str>,
    rules: Vec<Arc<str>>,
}

impl Rule {
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
                {
                    if !show_has_help || rule.help.is_some() {
                        md.heading(4, rule_name)?;
                        rule.print_markdown(md, details.to_owned(), is_run_rules)?;
                    }
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

        if let Some(deps) = self.deps.as_ref() {
            if !deps.is_empty() {
                md.bold("Dependencies")?;
                md.printer.newline()?;
                md.printer.newline()?;
                for dep in deps {
                    // get the rule using the dep as the name
                    md.list_item(0, dep)?;
                }
                md.printer.newline()?;
            }
        }

        Ok(())
    }
}
