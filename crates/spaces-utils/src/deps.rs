use crate::changes;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Globs {
    Includes(Vec<Arc<str>>),
    Excludes(Vec<Arc<str>>),
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
pub struct RuleTarget {
    pub rule: Arc<str>,
    pub target: Arc<str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnyDep {
    Rule(Arc<str>),
    Globs(Globs),
    Target(RuleTarget),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Deps {
    // This is deprecated. These are auto-converted to Any entries
    Rules(Vec<Arc<str>>),
    Any(Vec<AnyDep>),
}

impl Default for Deps {
    fn default() -> Self {
        Self::Any(Vec::new())
    }
}

impl Deps {
    /// Returns true if this is the `Rules` variant and the list is empty,
    /// or if this is the `Any` variant and the list is empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Deps::Rules(rules) => rules.is_empty(),
            Deps::Any(any) => any.is_empty(),
        }
    }

    /// Returns all rule names from `Rules`, `Any(AnyDep::Rule)`, and `Any(AnyDep::Target)` variants.
    pub fn collect_all_rules(&self) -> Vec<Arc<str>> {
        match self {
            Deps::Rules(rules) => rules.clone(),
            Deps::Any(list) => list
                .iter()
                .filter_map(|entry| match entry {
                    AnyDep::Rule(rule) => Some(rule.clone()),
                    AnyDep::Target(target) => Some(target.rule.clone()),
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
            Deps::Any(list) => list.iter().any(|entry| matches!(entry, AnyDep::Globs(_))),
        }
    }

    /// Returns all `Globs` entries collected from `AnyDep::Glob` within the `Any` variant.
    pub fn collect_globs(&self) -> Vec<Globs> {
        match self {
            Deps::Rules(_) => Vec::new(),
            Deps::Any(any_list) => any_list
                .iter()
                .filter_map(|entry| match entry {
                    AnyDep::Globs(glob) => Some(glob.clone()),
                    _ => None,
                })
                .collect(),
        }
    }
}
