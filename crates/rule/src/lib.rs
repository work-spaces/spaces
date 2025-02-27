use printer::markdown;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const SETUP_RULE_NAME: &str = "//:setup";
pub const ALL_RULE_NAME: &str = "//:all";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RuleType {
    Setup,
    Run,
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

type RuleMap = HashMap<Arc<str>, Rule>;

impl Rule {
    fn get_hash_map(rules: &[(&Rule, Option<String>)]) -> RuleMap {
        let mut map = HashMap::new();
        for (rule, _) in rules {
            map.insert(rule.name.clone(), (*rule).clone());
        }
        map
    }

    pub fn print_markdown_header(md: &mut markdown::Markdown) -> anyhow::Result<()> {
        md.heading(1, "Rules")?;
        Ok(())
    }

    pub fn print_markdown_section_heading(
        md: &mut markdown::Markdown,
        section_name: &str,
        rules: &[(&Rule, Option<String>)],
    ) -> anyhow::Result<()> {
        md.heading(2, format!("Overview: {section_name}").as_str())?;
        let mut sorted_rules = rules.to_vec();
        sorted_rules.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        for (rule, _) in sorted_rules {
            if rule.help.is_some() {
                md.list_item(0, &markdown::hyperlink(&rule.name, &rule.to_tag_anchor()))?;
            }
        }
        md.printer.newline()?;

        Ok(())
    }

    pub fn print_markdown_section_body(
        md: &mut markdown::Markdown,
        section_name: &str,
        rules: &[(&Rule, Option<String>)],
    ) -> anyhow::Result<()> {
        let rule_map = Self::get_hash_map(rules);
        md.heading(2, format!("Details: {section_name}").as_str())?;
        let mut sorted_rules = rules.to_vec();
        sorted_rules.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        for (rule, details) in sorted_rules {
            if rule.help.is_some() {
                rule.print_markdown(md, &rule_map, details)?;
            }
        }
        md.printer.newline()?;
        Ok(())
    }

    fn name_to_anchor(name: &str) -> String {
        let anchor = name.to_string().replace(':', "_");
        let anchor = anchor.to_string().replace('.', "-");
        let anchor = anchor.to_string().replace("//", "");
        let anchor = anchor.to_string().replace('/', "-");
        anchor.to_lowercase()
    }

    fn name_to_tag_anchor(name: &str) -> String {
        format!("#{}", Self::name_to_anchor(name))
    }

    fn to_anchor(&self) -> String {
        Self::name_to_anchor(&self.name)
    }

    fn to_tag_anchor(&self) -> String {
        Self::name_to_tag_anchor(&self.name)
    }

    fn print_markdown(
        &self,
        md: &mut markdown::Markdown,
        rule_map: &RuleMap,
        details: Option<String>,
    ) -> anyhow::Result<()> {
        md.hline()?;

        let heading = format!("{}", self.name);
        md.heading(3, heading.as_str())?;
        md.heading(5, &self.to_anchor())?;

        md.printer.newline()?;

        let spaces_run_example = format!("spaces run {}", self.name);
        md.code_block("sh", spaces_run_example.as_str())?;

        if let Some(help) = &self.help {
            md.heading(3, "Description")?;
            md.paragraph(help)?;
            md.printer.newline()?;
            if let Some(details) = details {
                md.paragraph(details.as_str())?;
                md.printer.newline()?;
            }
        } else {
            md.paragraph("No help text provided")?;
            md.printer.newline()?;
        }

        if let Some(deps) = self.deps.as_ref() {
            md.heading(3, "Dependencies")?;
            for dep in deps {
                // get the rule using the dep as the name
                if let Some(dep_rule) = rule_map.get(dep) {
                    if dep_rule.help.is_some() {
                        md.list_item(0, &markdown::hyperlink(dep, &Self::name_to_tag_anchor(dep)))?;
                    } else {
                        md.list_item(0, dep)?;
                    }
                }
            }
            md.printer.newline()?;
        }

        Ok(())
    }
}
