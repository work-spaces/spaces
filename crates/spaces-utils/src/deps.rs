use crate::{changes, labels};
use anyhow::Context;
use anyhow_source_location::format_context;
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

impl AnyDep {
    /// Sanitizes rule names and glob patterns within this dep entry.
    pub fn sanitize(
        &mut self,
        rule_label: Arc<str>,
        starlark_module: Option<Arc<str>>,
        spaces_module_suffix: &str,
    ) -> anyhow::Result<()> {
        match self {
            AnyDep::Rule(dep) => {
                if !labels::is_rule_sanitized(dep) {
                    *dep = labels::sanitize_rule(
                        dep.clone(),
                        starlark_module.clone(),
                        spaces_module_suffix,
                    );
                }
            }
            AnyDep::Globs(glob) => match glob {
                Globs::Includes(set) => {
                    Self::sanitize_glob_vec(
                        set,
                        labels::IsAnnotated::No,
                        rule_label.clone(),
                        starlark_module.clone(),
                    )?;
                }
                Globs::Excludes(set) => {
                    Self::sanitize_glob_vec(
                        set,
                        labels::IsAnnotated::No,
                        rule_label.clone(),
                        starlark_module.clone(),
                    )?;
                }
            },
            AnyDep::Target(target) => {
                if !labels::is_rule_sanitized(&target.rule) {
                    target.rule = labels::sanitize_rule(
                        target.rule.clone(),
                        starlark_module.clone(),
                        spaces_module_suffix,
                    );
                }
            }
        }
        Ok(())
    }

    fn sanitize_glob_vec(
        vec: &mut Vec<Arc<str>>,
        is_annotated: labels::IsAnnotated,
        rule_label: Arc<str>,
        starlark_module: Option<Arc<str>>,
    ) -> anyhow::Result<()> {
        *vec = vec
            .drain(..)
            .map(|item| {
                labels::sanitize_glob_value(
                    item.as_ref(),
                    is_annotated,
                    rule_label.as_ref(),
                    starlark_module.clone(),
                )
                .context(format_context!("Failed to sanitize deps glob: {item}"))
            })
            .collect::<anyhow::Result<Vec<Arc<str>>>>()?;

        Ok(())
    }
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

    /// Sanitizes rule names and glob vectors within the deps.
    /// For `Rules` variant, sanitizes each rule name.
    /// For `Any` variant, sanitizes rule names in `AnyDep::Rule` and `AnyDep::Target`,
    /// and sanitizes glob patterns in `AnyDep::Globs`.
    pub fn sanitize(
        &mut self,
        rule_label: Arc<str>,
        starlark_module: Option<Arc<str>>,
        spaces_module_suffix: &str,
    ) -> anyhow::Result<()> {
        match self {
            Deps::Rules(rules) => {
                for dep in rules.iter_mut() {
                    if labels::is_rule_sanitized(dep) {
                        continue;
                    }
                    *dep = labels::sanitize_rule(
                        dep.clone(),
                        starlark_module.clone(),
                        spaces_module_suffix,
                    );
                }
            }
            Deps::Any(any_list) => {
                for any_entry in any_list.iter_mut() {
                    any_entry.sanitize(
                        rule_label.clone(),
                        starlark_module.clone(),
                        spaces_module_suffix,
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------
    // Globs
    // -------------------------------------------------------

    #[test]
    fn test_to_changes_globs_empty() {
        let items: Vec<Globs> = vec![];
        let result = Globs::to_changes_globs(&items);
        assert!(result.includes.is_empty());
        assert!(result.excludes.is_empty());
    }

    #[test]
    fn test_to_changes_globs_includes_only() {
        let items = vec![Globs::Includes(vec!["a/**".into(), "b/**".into()])];
        let result = Globs::to_changes_globs(&items);
        assert_eq!(result.includes.len(), 2);
        assert!(result.includes.contains::<Arc<str>>(&"a/**".into()));
        assert!(result.includes.contains::<Arc<str>>(&"b/**".into()));
        assert!(result.excludes.is_empty());
    }

    #[test]
    fn test_to_changes_globs_excludes_only() {
        let items = vec![Globs::Excludes(vec!["*.log".into()])];
        let result = Globs::to_changes_globs(&items);
        assert!(result.includes.is_empty());
        assert_eq!(result.excludes.len(), 1);
        assert!(result.excludes.contains::<Arc<str>>(&"*.log".into()));
    }

    #[test]
    fn test_to_changes_globs_mixed() {
        let items = vec![
            Globs::Includes(vec!["src/**".into()]),
            Globs::Excludes(vec!["*.tmp".into()]),
            Globs::Includes(vec!["lib/**".into()]),
            Globs::Excludes(vec!["*.bak".into()]),
        ];
        let result = Globs::to_changes_globs(&items);
        assert_eq!(result.includes.len(), 2);
        assert_eq!(result.excludes.len(), 2);
        assert!(result.includes.contains::<Arc<str>>(&"src/**".into()));
        assert!(result.includes.contains::<Arc<str>>(&"lib/**".into()));
        assert!(result.excludes.contains::<Arc<str>>(&"*.tmp".into()));
        assert!(result.excludes.contains::<Arc<str>>(&"*.bak".into()));
    }

    // -------------------------------------------------------
    // Deps::default
    // -------------------------------------------------------

    #[test]
    fn test_deps_default_is_empty_any() {
        let deps = Deps::default();
        assert!(deps.is_empty());
        match &deps {
            Deps::Any(v) => assert!(v.is_empty()),
            _ => panic!("default should be Any"),
        }
    }

    // -------------------------------------------------------
    // Deps::is_empty
    // -------------------------------------------------------

    #[test]
    fn test_is_empty_rules_empty() {
        let deps = Deps::Rules(vec![]);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_is_empty_rules_non_empty() {
        let deps = Deps::Rules(vec!["//a:rule".into()]);
        assert!(!deps.is_empty());
    }

    #[test]
    fn test_is_empty_any_empty() {
        let deps = Deps::Any(vec![]);
        assert!(deps.is_empty());
    }

    #[test]
    fn test_is_empty_any_non_empty() {
        let deps = Deps::Any(vec![AnyDep::Rule("//a:rule".into())]);
        assert!(!deps.is_empty());
    }

    // -------------------------------------------------------
    // Deps::collect_all_rules
    // -------------------------------------------------------

    #[test]
    fn test_collect_all_rules_from_rules_variant() {
        let deps = Deps::Rules(vec!["//a:one".into(), "//b:two".into()]);
        let rules = deps.collect_all_rules();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].as_ref(), "//a:one");
        assert_eq!(rules[1].as_ref(), "//b:two");
    }

    #[test]
    fn test_collect_all_rules_from_any_variant() {
        let deps = Deps::Any(vec![
            AnyDep::Rule("//a:rule".into()),
            AnyDep::Globs(Globs::Includes(vec!["src/**".into()])),
            AnyDep::Target(RuleTarget {
                rule: "//b:build".into(),
                target: "output.tar".into(),
            }),
            AnyDep::Rule("//c:test".into()),
        ]);
        let rules = deps.collect_all_rules();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].as_ref(), "//a:rule");
        assert_eq!(rules[1].as_ref(), "//b:build");
        assert_eq!(rules[2].as_ref(), "//c:test");
    }

    #[test]
    fn test_collect_all_rules_empty() {
        let deps = Deps::Any(vec![]);
        assert!(deps.collect_all_rules().is_empty());

        let deps = Deps::Rules(vec![]);
        assert!(deps.collect_all_rules().is_empty());
    }

    #[test]
    fn test_collect_all_rules_globs_only() {
        let deps = Deps::Any(vec![AnyDep::Globs(Globs::Includes(vec!["src/**".into()]))]);
        assert!(deps.collect_all_rules().is_empty());
    }

    // -------------------------------------------------------
    // Deps::push_any_dep
    // -------------------------------------------------------

    #[test]
    fn test_push_any_dep_into_none() {
        let mut deps: Option<Deps> = None;
        Deps::push_any_dep(&mut deps, AnyDep::Rule("//a:rule".into()));
        let deps = deps.unwrap();
        match &deps {
            Deps::Any(list) => {
                assert_eq!(list.len(), 1);
                match &list[0] {
                    AnyDep::Rule(r) => assert_eq!(r.as_ref(), "//a:rule"),
                    _ => panic!("expected Rule"),
                }
            }
            _ => panic!("expected Any variant"),
        }
    }

    #[test]
    fn test_push_any_dep_into_rules_converts_to_any() {
        let mut deps: Option<Deps> = Some(Deps::Rules(vec![
            "//existing:one".into(),
            "//existing:two".into(),
        ]));
        Deps::push_any_dep(
            &mut deps,
            AnyDep::Globs(Globs::Includes(vec!["src/**".into()])),
        );
        let deps = deps.unwrap();
        match &deps {
            Deps::Any(list) => {
                assert_eq!(list.len(), 3);
                // first two are converted from Rules
                match &list[0] {
                    AnyDep::Rule(r) => assert_eq!(r.as_ref(), "//existing:one"),
                    _ => panic!("expected Rule"),
                }
                match &list[1] {
                    AnyDep::Rule(r) => assert_eq!(r.as_ref(), "//existing:two"),
                    _ => panic!("expected Rule"),
                }
                // third is the new Globs
                assert!(matches!(&list[2], AnyDep::Globs(Globs::Includes(_))));
            }
            _ => panic!("expected Any variant"),
        }
    }

    #[test]
    fn test_push_any_dep_into_existing_any() {
        let mut deps: Option<Deps> = Some(Deps::Any(vec![AnyDep::Rule("//a:first".into())]));
        Deps::push_any_dep(&mut deps, AnyDep::Rule("//b:second".into()));
        let deps = deps.unwrap();
        match &deps {
            Deps::Any(list) => {
                assert_eq!(list.len(), 2);
                match &list[1] {
                    AnyDep::Rule(r) => assert_eq!(r.as_ref(), "//b:second"),
                    _ => panic!("expected Rule"),
                }
            }
            _ => panic!("expected Any variant"),
        }
    }

    // -------------------------------------------------------
    // Deps::push_any_deps
    // -------------------------------------------------------

    #[test]
    fn test_push_any_deps_into_none() {
        let mut deps: Option<Deps> = None;
        Deps::push_any_deps(
            &mut deps,
            vec![
                AnyDep::Rule("//a:one".into()),
                AnyDep::Rule("//a:two".into()),
            ],
        );
        let deps = deps.unwrap();
        match &deps {
            Deps::Any(list) => assert_eq!(list.len(), 2),
            _ => panic!("expected Any variant"),
        }
    }

    #[test]
    fn test_push_any_deps_into_rules_converts_to_any() {
        let mut deps: Option<Deps> = Some(Deps::Rules(vec!["//x:existing".into()]));
        Deps::push_any_deps(
            &mut deps,
            vec![
                AnyDep::Rule("//y:new1".into()),
                AnyDep::Rule("//y:new2".into()),
            ],
        );
        let deps = deps.unwrap();
        match &deps {
            Deps::Any(list) => {
                assert_eq!(list.len(), 3);
            }
            _ => panic!("expected Any variant"),
        }
    }

    #[test]
    fn test_push_any_deps_into_existing_any() {
        let mut deps: Option<Deps> = Some(Deps::Any(vec![AnyDep::Rule("//a:first".into())]));
        Deps::push_any_deps(
            &mut deps,
            vec![
                AnyDep::Rule("//b:second".into()),
                AnyDep::Rule("//c:third".into()),
            ],
        );
        let deps = deps.unwrap();
        match &deps {
            Deps::Any(list) => assert_eq!(list.len(), 3),
            _ => panic!("expected Any variant"),
        }
    }

    // -------------------------------------------------------
    // Deps::has_globs
    // -------------------------------------------------------

    #[test]
    fn test_has_globs_rules_variant() {
        let deps = Deps::Rules(vec!["//a:rule".into()]);
        assert!(!deps.has_globs());
    }

    #[test]
    fn test_has_globs_any_without_globs() {
        let deps = Deps::Any(vec![
            AnyDep::Rule("//a:rule".into()),
            AnyDep::Target(RuleTarget {
                rule: "//b:build".into(),
                target: "out".into(),
            }),
        ]);
        assert!(!deps.has_globs());
    }

    #[test]
    fn test_has_globs_any_with_globs() {
        let deps = Deps::Any(vec![
            AnyDep::Rule("//a:rule".into()),
            AnyDep::Globs(Globs::Includes(vec!["src/**".into()])),
        ]);
        assert!(deps.has_globs());
    }

    #[test]
    fn test_has_globs_empty_any() {
        let deps = Deps::Any(vec![]);
        assert!(!deps.has_globs());
    }

    // -------------------------------------------------------
    // Deps::collect_globs
    // -------------------------------------------------------

    #[test]
    fn test_collect_globs_rules_variant() {
        let deps = Deps::Rules(vec!["//a:rule".into()]);
        assert!(deps.collect_globs().is_empty());
    }

    #[test]
    fn test_collect_globs_any_no_globs() {
        let deps = Deps::Any(vec![AnyDep::Rule("//a:rule".into())]);
        assert!(deps.collect_globs().is_empty());
    }

    #[test]
    fn test_collect_globs_any_with_globs() {
        let deps = Deps::Any(vec![
            AnyDep::Rule("//a:rule".into()),
            AnyDep::Globs(Globs::Includes(vec!["src/**".into()])),
            AnyDep::Globs(Globs::Excludes(vec!["*.tmp".into()])),
        ]);
        let globs = deps.collect_globs();
        assert_eq!(globs.len(), 2);
        match &globs[0] {
            Globs::Includes(v) => assert_eq!(v[0].as_ref(), "src/**"),
            _ => panic!("expected Includes"),
        }
        match &globs[1] {
            Globs::Excludes(v) => assert_eq!(v[0].as_ref(), "*.tmp"),
            _ => panic!("expected Excludes"),
        }
    }

    // -------------------------------------------------------
    // AnyDep::sanitize
    // -------------------------------------------------------

    #[test]
    fn test_sanitize_rule_unsanitized() {
        let mut dep = AnyDep::Rule("my_rule".into());
        dep.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        match &dep {
            AnyDep::Rule(r) => assert_eq!(r.as_ref(), "//pkg:my_rule"),
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn test_sanitize_rule_already_sanitized() {
        let mut dep = AnyDep::Rule("//already:sanitized".into());
        dep.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        match &dep {
            AnyDep::Rule(r) => assert_eq!(r.as_ref(), "//already:sanitized"),
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn test_sanitize_target_unsanitized() {
        let mut dep = AnyDep::Target(RuleTarget {
            rule: "build_rule".into(),
            target: "output.tar".into(),
        });
        dep.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        match &dep {
            AnyDep::Target(t) => {
                assert_eq!(t.rule.as_ref(), "//pkg:build_rule");
                assert_eq!(t.target.as_ref(), "output.tar");
            }
            _ => panic!("expected Target"),
        }
    }

    #[test]
    fn test_sanitize_target_already_sanitized() {
        let mut dep = AnyDep::Target(RuleTarget {
            rule: "//already:done".into(),
            target: "out".into(),
        });
        dep.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        match &dep {
            AnyDep::Target(t) => assert_eq!(t.rule.as_ref(), "//already:done"),
            _ => panic!("expected Target"),
        }
    }

    #[test]
    fn test_sanitize_globs_includes() {
        let mut dep = AnyDep::Globs(Globs::Includes(vec!["//src/**".into()]));
        dep.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        match &dep {
            AnyDep::Globs(Globs::Includes(v)) => {
                assert_eq!(v.len(), 1);
                // IsAnnotated::No + starts with "//" → stripped to "src/**"
                assert_eq!(v[0].as_ref(), "src/**");
            }
            _ => panic!("expected Globs::Includes"),
        }
    }

    #[test]
    fn test_sanitize_globs_excludes() {
        let mut dep = AnyDep::Globs(Globs::Excludes(vec!["//build/**".into()]));
        dep.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        match &dep {
            AnyDep::Globs(Globs::Excludes(v)) => {
                assert_eq!(v.len(), 1);
                assert_eq!(v[0].as_ref(), "build/**");
            }
            _ => panic!("expected Globs::Excludes"),
        }
    }

    // -------------------------------------------------------
    // Deps::sanitize
    // -------------------------------------------------------

    #[test]
    fn test_deps_sanitize_rules_variant() {
        let mut deps = Deps::Rules(vec!["unsanitized".into(), "//already:ok".into()]);
        deps.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        match &deps {
            Deps::Rules(rules) => {
                assert_eq!(rules[0].as_ref(), "//pkg:unsanitized");
                assert_eq!(rules[1].as_ref(), "//already:ok");
            }
            _ => panic!("expected Rules variant"),
        }
    }

    #[test]
    fn test_deps_sanitize_any_variant_mixed() {
        let mut deps = Deps::Any(vec![
            AnyDep::Rule("my_rule".into()),
            AnyDep::Globs(Globs::Includes(vec!["//src/**".into()])),
            AnyDep::Target(RuleTarget {
                rule: "build".into(),
                target: "out".into(),
            }),
        ]);
        deps.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        match &deps {
            Deps::Any(list) => {
                match &list[0] {
                    AnyDep::Rule(r) => assert_eq!(r.as_ref(), "//pkg:my_rule"),
                    _ => panic!("expected Rule"),
                }
                match &list[1] {
                    AnyDep::Globs(Globs::Includes(v)) => assert_eq!(v[0].as_ref(), "src/**"),
                    _ => panic!("expected Globs::Includes"),
                }
                match &list[2] {
                    AnyDep::Target(t) => assert_eq!(t.rule.as_ref(), "//pkg:build"),
                    _ => panic!("expected Target"),
                }
            }
            _ => panic!("expected Any variant"),
        }
    }

    #[test]
    fn test_deps_sanitize_empty_any() {
        let mut deps = Deps::Any(vec![]);
        deps.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn test_deps_sanitize_empty_rules() {
        let mut deps = Deps::Rules(vec![]);
        deps.sanitize(
            "//pkg:label".into(),
            Some("pkg/spaces.star".into()),
            "spaces.star",
        )
        .unwrap();
        assert!(deps.is_empty());
    }

    // -------------------------------------------------------
    // Serde round-trip
    // -------------------------------------------------------

    #[test]
    fn test_globs_serde_roundtrip() {
        let globs = Globs::Includes(vec!["a/**".into(), "b/**".into()]);
        let json = serde_json::to_string(&globs).unwrap();
        let deserialized: Globs = serde_json::from_str(&json).unwrap();
        match deserialized {
            Globs::Includes(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].as_ref(), "a/**");
                assert_eq!(v[1].as_ref(), "b/**");
            }
            _ => panic!("expected Includes"),
        }
    }

    #[test]
    fn test_any_dep_serde_roundtrip() {
        let dep = AnyDep::Rule("//pkg:rule".into());
        let json = serde_json::to_string(&dep).unwrap();
        let deserialized: AnyDep = serde_json::from_str(&json).unwrap();
        match deserialized {
            AnyDep::Rule(r) => assert_eq!(r.as_ref(), "//pkg:rule"),
            _ => panic!("expected Rule"),
        }
    }

    #[test]
    fn test_rule_target_serde_roundtrip() {
        let target = RuleTarget {
            rule: "//pkg:build".into(),
            target: "output.tar".into(),
        };
        let json = serde_json::to_string(&target).unwrap();
        let deserialized: RuleTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.rule.as_ref(), "//pkg:build");
        assert_eq!(deserialized.target.as_ref(), "output.tar");
    }
}
