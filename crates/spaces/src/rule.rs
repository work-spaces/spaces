use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
