//! Module Target creation (spaces modules to JSON compilation).
//!
//! This module provides data structures for capturing and persisting
//! the results of evaluating Starlark modules. This enables compiling
//! spaces modules to JSON to avoid re-evaluation when the
//! module and its dependencies haven't changed.

use crate::{bin_detail, platform, rule};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Directory where module evaluation results are saved (relative to workspace root).
pub const MODULE_TARGETS_DIR: &str = "build/spaces-module-targets";
pub const MODULE_DEPS_DIR: &str = "build/spaces-module-deps";

/// File extension for module result files.
pub const MODULE_RESULTS_SUFFIX: &str = ".json";

fn get_json_path(dir: &str, module_name: &str) -> Arc<std::path::Path> {
    let build_module_target_dir = std::path::Path::new(dir);
    let module_name_json = format!("{module_name}.json");
    let module_path = std::path::Path::new(module_name_json.as_str());

    let file_path = build_module_target_dir.join(module_path);
    file_path.into()
}

fn get_existing_json_path(dir: &str, module_name: &str) -> Option<Arc<std::path::Path>> {
    let path = get_json_path(dir, module_name);
    if !path.exists() {
        return None;
    }

    Some(path)
}

/// Represents a load statement in a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadStatement {
    /// The module path as a relative workspace path (e.g., "lib/common.star")
    pub module_id: Arc<str>,
}

/// Summary of a task created during module evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// The full task/rule name (label)
    pub name: Arc<str>,

    /// The task phase as a string (e.g., "Checkout", "Run", "Inspect")
    pub phase: Arc<str>,

    /// The default visibility that was applied during evaluation.
    /// Used when restoring from cache if the rule's visibility is None.
    pub default_visibility: rule::Visibility,

    /// Serialized task data for replay/inspection
    pub task_json: serde_json::Value,
}

/// Represents the dependencies of a Starlark module.
/// This structure captures all information needed for calculating cache keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleDeps {
    /// The module path (e.g., "spaces/spaces.star" or "//repo:spaces.star")
    pub module_name: Arc<str>,

    /// List of load statements - modules that this module depends on
    pub loads: Vec<LoadStatement>,

    pub checkout_state_digest: Arc<str>,
}

impl ModuleDeps {
    pub fn get_json_path(module_name: &str) -> Arc<std::path::Path> {
        get_json_path(MODULE_DEPS_DIR, module_name)
    }

    /// Loads a cached ModuleEvaluationResult from the build directory.
    ///
    /// Returns `Ok(None)` if the cache file doesn't exist (indicating rcache should be skipped).
    /// Returns `Ok(Some(result))` if the file exists and was successfully loaded.
    /// Returns `Err` if the file exists but couldn't be read or parsed.
    ///
    /// This function assumes it is called from the workspace root directory.
    ///
    /// # Arguments
    /// * `module_name` - The module path (e.g., "spaces/spaces.star")
    pub fn new_from_json(module_name: &str) -> anyhow::Result<Option<Self>> {
        let Some(file_path) = get_existing_json_path(MODULE_DEPS_DIR, module_name) else {
            return Ok(None);
        };

        let content = std::fs::read_to_string(file_path.as_ref()).context(format_context!(
            "Failed to read module deps from {}",
            file_path.display()
        ))?;

        let result: Self = serde_json::from_str(&content).context(format_context!(
            "Failed to parse module deps from {}",
            file_path.display()
        ))?;

        Ok(Some(result))
    }

    /// Computes a unique digest for this module evaluation result.
    /// Computes a digest for the module and its dependencies.
    ///
    /// The digest is computed from:
    /// - The hash of the module file itself
    /// - The hashes of all files loaded via load() statements
    /// - The evaluation inputs stored on `ModuleDeps` (`platform`, `is_ci`,
    ///   `store_values`, and `env_values`)
    ///
    /// This allows cache invalidation when any input file changes.
    ///
    /// # Arguments
    /// * `star_files` - The HashMap of star file paths to their BinDetail (containing hashes)
    ///
    /// # Returns
    /// A blake3 digest string, or an error if required files aren't found in star_files.
    pub fn compute_digest(
        &self,
        star_files: &HashMap<Arc<str>, bin_detail::BinDetail>,
    ) -> anyhow::Result<Arc<str>> {
        let mut hasher = blake3::Hasher::new();

        // Hash the module file itself (module_name is already a relative workspace path)
        if let Some(detail) = star_files.get(self.module_name.as_ref()) {
            hasher.update(&detail.hash);
        } else {
            return Err(format_error!(
                "Internal Error: Module file '{}' not found in star_files",
                self.module_name
            ));
        }

        // Iterate over loads (assumed to be sorted via set_loads)
        for load in &self.loads {
            if let Some(detail) = star_files.get(&load.module_id) {
                hasher.update(&detail.hash);
            } else {
                return Err(format_error!(
                    "Internal Error: Load module '{}' not found in star_files",
                    load.module_id
                ));
            }
        }

        hasher.update(self.checkout_state_digest.as_ref().as_bytes());

        Ok(hasher.finalize().to_string().into())
    }

    /// Computes a digest from just the evaluation inputs (without file hashes).
    ///
    /// This is useful for computing a digest from platform, is_ci, store_values,
    /// and env_values without needing the file hashes.
    ///
    /// # Arguments
    /// * `platform` - The target platform
    /// * `is_ci` - Whether running in CI
    /// * `store_values` - Store key-value pairs (should be sorted)
    /// * `env_values` - Environment key-value pairs (should be sorted)
    ///
    /// # Returns
    /// A blake3 digest string, or an error if serialization fails.
    pub fn digest_from_inputs(
        platform: platform::Platform,
        is_ci: bool,
        store_values: &[(Arc<str>, Arc<str>)],
        env_values: &[(Arc<str>, Arc<str>)],
    ) -> anyhow::Result<Arc<str>> {
        let mut hasher = blake3::Hasher::new();

        let dependency_inputs = serde_json::to_vec(&(platform, is_ci, store_values, env_values))
            .context(format_context!(
                "Failed to serialize module dependency inputs"
            ))?;
        hasher.update(&dependency_inputs);

        Ok(hasher.finalize().to_string().into())
    }

    /// Adds a load statement to the result.
    pub fn add_load(&mut self, load: LoadStatement) {
        self.loads.push(load);
    }

    /// Sets all load statements, sorting them by module_id for deterministic digest computation.
    pub fn set_loads(&mut self, mut loads: Vec<LoadStatement>) {
        loads.sort_by(|a, b| a.module_id.cmp(&b.module_id));
        self.loads = loads;
    }

    /// Saves this module evaluation result to the build directory.
    ///
    /// The file is saved to `<workspace_path>/build/spaces-modules-deps/<module_path>.json`
    /// mirroring the workspace directory structure (e.g., `spaces/spaces.star` becomes
    /// `build/spaces-modules-deps/spaces/spaces.star.json`).
    pub fn save_to_json(&self) -> anyhow::Result<()> {
        let file_path = get_json_path(MODULE_DEPS_DIR, self.module_name.as_ref());

        // Create parent directories to mirror workspace structure
        if let Some(parent) = std::path::Path::new(file_path.as_ref()).parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create module results directory at {}",
                parent.display()
            ))?;
        }

        let content = serde_json::to_string_pretty(&self)
            .context(format_context!("Failed to serialize module result"))?;

        // rcache links files in as read-only - remove to replace the file
        // this does not run exclusively under rcache so rcache
        // cannot manage the file removal
        let _ = std::fs::remove_file(&file_path);

        std::fs::write(&file_path, content).context(format_context!(
            "Failed to write module result to {}",
            file_path.display()
        ))?;

        Ok(())
    }
}

/// Represents the result of evaluating a single Starlark module.
/// This structure captures all information needed for caching and replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleTarget {
    /// The module path (e.g., "spaces/spaces.star" or "//repo:spaces.star")
    pub module_name: Arc<str>,

    /// List of load statements - modules that this module depends on
    pub loads: Vec<LoadStatement>,

    /// Tasks created during evaluation of this module.
    /// Keys are task names (rule labels like "//repo:task_name")
    pub rules: HashMap<Arc<str>, Rule>,
}

impl ModuleTarget {
    /// Creates a new ModuleEvaluationResult.
    pub fn new(module_name: Arc<str>) -> Self {
        Self {
            module_name,
            loads: Vec::new(),
            rules: HashMap::new(),
        }
    }

    /// Adds a load statement to the result.
    pub fn add_load(&mut self, load: LoadStatement) {
        self.loads.push(load);
    }

    pub fn get_json_path(module_name: &str) -> Arc<std::path::Path> {
        get_json_path(MODULE_TARGETS_DIR, module_name)
    }

    /// Loads a cached ModuleEvaluationResult from the build directory.
    ///
    /// Returns `Ok(None)` if the cache file doesn't exist (indicating rcache should be skipped).
    /// Returns `Ok(Some(result))` if the file exists and was successfully loaded.
    /// Returns `Err` if the file exists but couldn't be read or parsed.
    ///
    /// This function assumes it is called from the workspace root directory.
    ///
    /// # Arguments
    /// * `module_name` - The module path (e.g., "spaces/spaces.star")
    pub fn new_from_json(module_name: &str) -> anyhow::Result<Option<Self>> {
        let Some(file_path) = get_existing_json_path(MODULE_TARGETS_DIR, module_name) else {
            return Ok(None);
        };

        let content = std::fs::read_to_string(file_path.as_ref()).context(format_context!(
            "Failed to read module result from {}",
            file_path.display()
        ))?;

        let result: Self = serde_json::from_str(&content).context(format_context!(
            "Failed to parse module result from {}",
            file_path.display()
        ))?;

        Ok(Some(result))
    }

    /// Adds a task summary to the result.
    pub fn insert_rule(&mut self, rule: Rule) {
        self.rules.insert(rule.name.clone(), rule);
    }

    /// Saves this module evaluation result to the build directory.
    ///
    /// The file is saved to `<workspace_path>/build/spaces-modules/<module_path>.json`
    /// mirroring the workspace directory structure (e.g., `spaces/spaces.star` becomes
    /// `build/spaces-modules/spaces/spaces.star.json`).
    pub fn save_to_json(&self) -> anyhow::Result<()> {
        let file_path = get_json_path(MODULE_TARGETS_DIR, self.module_name.as_ref());

        // Create parent directories to mirror workspace structure
        if let Some(parent) = std::path::Path::new(file_path.as_ref()).parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create module results directory at {}",
                parent.display()
            ))?;
        }

        let content = serde_json::to_string_pretty(&self)
            .context(format_context!("Failed to serialize module result"))?;

        // rcache links files in as read-only - remove to replace the file
        // this does not run exclusively under rcache so rcache
        // cannot manage the file removal
        let _ = std::fs::remove_file(&file_path);

        std::fs::write(&file_path, content).context(format_context!(
            "Failed to write module result to {}",
            file_path.display()
        ))?;

        Ok(())
    }
}

impl LoadStatement {
    /// Creates a new LoadStatement.
    pub fn new(module_id: Arc<str>) -> Self {
        Self { module_id }
    }
}

impl Rule {
    /// Creates a new TaskSummary.
    pub fn new(
        name: Arc<str>,
        phase: Arc<str>,
        default_visibility: rule::Visibility,
        task_json: serde_json::Value,
    ) -> Self {
        Self {
            name,
            phase,
            default_visibility,
            task_json,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== LoadStatement Tests =====

    #[test]
    fn test_load_statement_new() {
        let load = LoadStatement::new("lib/common.star".into());
        assert_eq!(load.module_id.as_ref(), "lib/common.star");
    }

    #[test]
    fn test_load_statement_serde() {
        let load = LoadStatement::new("lib/common.star".into());
        let json = serde_json::to_string(&load).unwrap();
        let deserialized: LoadStatement = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.module_id, load.module_id);
    }

    // ===== Rule Tests =====

    #[test]
    fn test_rule_new() {
        let rule = Rule::new(
            "//test:build".into(),
            "Run".into(),
            rule::Visibility::Public,
            serde_json::json!({"name": "//test:build"}),
        );
        assert_eq!(rule.name.as_ref(), "//test:build");
        assert_eq!(rule.phase.as_ref(), "Run");
        assert_eq!(rule.default_visibility, rule::Visibility::Public);
    }

    #[test]
    fn test_rule_with_private_visibility() {
        let rule = Rule::new(
            "//test:internal".into(),
            "Test".into(),
            rule::Visibility::Private,
            serde_json::json!({"type": "test"}),
        );
        assert_eq!(rule.default_visibility, rule::Visibility::Private);
    }

    #[test]
    fn test_rule_serde() {
        let rule = Rule::new(
            "//test:build".into(),
            "Run".into(),
            rule::Visibility::Public,
            serde_json::json!({"executor": "Target"}),
        );
        let json = serde_json::to_string(&rule).unwrap();
        let deserialized: Rule = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, rule.name);
        assert_eq!(deserialized.phase, rule.phase);
        assert_eq!(deserialized.default_visibility, rule.default_visibility);
    }

    // ===== ModuleTarget Tests =====

    #[test]
    fn test_module_evaluation_result_new() {
        let result = ModuleTarget::new("test/module.star".into());
        assert_eq!(result.module_name.as_ref(), "test/module.star");
        assert!(result.loads.is_empty());
        assert!(result.rules.is_empty());
    }

    #[test]
    fn test_add_load() {
        let mut result = ModuleTarget::new("test/module.star".into());
        result.add_load(LoadStatement::new("lib/common.star".into()));
        assert_eq!(result.loads.len(), 1);
        assert_eq!(result.loads[0].module_id.as_ref(), "lib/common.star");
    }

    #[test]
    fn test_add_multiple_loads() {
        let mut result = ModuleTarget::new("test/module.star".into());
        result.add_load(LoadStatement::new("lib/common.star".into()));
        result.add_load(LoadStatement::new("lib/utils.star".into()));
        result.add_load(LoadStatement::new("lib/helpers.star".into()));
        assert_eq!(result.loads.len(), 3);
        assert_eq!(result.loads[0].module_id.as_ref(), "lib/common.star");
        assert_eq!(result.loads[1].module_id.as_ref(), "lib/utils.star");
        assert_eq!(result.loads[2].module_id.as_ref(), "lib/helpers.star");
    }

    #[test]
    fn test_add_task() {
        let mut result = ModuleTarget::new("test/module.star".into());
        result.insert_rule(Rule::new(
            "//test:build".into(),
            "Run".into(),
            rule::Visibility::Public,
            serde_json::json!({"name": "//test:build"}),
        ));
        assert_eq!(result.rules.len(), 1);
        assert!(result.rules.contains_key(&Arc::from("//test:build")));
    }

    #[test]
    fn test_insert_multiple_rules() {
        let mut result = ModuleTarget::new("test/module.star".into());
        result.insert_rule(Rule::new(
            "//test:build".into(),
            "Run".into(),
            rule::Visibility::Public,
            serde_json::json!({"name": "//test:build"}),
        ));
        result.insert_rule(Rule::new(
            "//test:test".into(),
            "Test".into(),
            rule::Visibility::Public,
            serde_json::json!({"name": "//test:test"}),
        ));
        assert_eq!(result.rules.len(), 2);
        assert!(result.rules.contains_key(&Arc::from("//test:build")));
        assert!(result.rules.contains_key(&Arc::from("//test:test")));
    }

    #[test]
    fn test_insert_rule_replaces_existing() {
        let mut result = ModuleTarget::new("test/module.star".into());
        result.insert_rule(Rule::new(
            "//test:build".into(),
            "Run".into(),
            rule::Visibility::Public,
            serde_json::json!({"version": 1}),
        ));
        result.insert_rule(Rule::new(
            "//test:build".into(),
            "Run".into(),
            rule::Visibility::Private,
            serde_json::json!({"version": 2}),
        ));
        assert_eq!(result.rules.len(), 1);
        let rule = result.rules.get(&Arc::from("//test:build")).unwrap();
        assert_eq!(rule.default_visibility, rule::Visibility::Private);
        assert_eq!(rule.task_json["version"], 2);
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut result = ModuleTarget::new("test/module.star".into());
        result.add_load(LoadStatement::new("lib/common.star".into()));
        result.insert_rule(Rule::new(
            "//test:build".into(),
            "Run".into(),
            rule::Visibility::Private,
            serde_json::json!({"executor": "Target", "phase": "Run"}),
        ));

        let json = serde_json::to_string_pretty(&result).unwrap();
        let deserialized: ModuleTarget = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.module_name, result.module_name);
        assert_eq!(deserialized.loads.len(), result.loads.len());
        assert_eq!(deserialized.rules.len(), result.rules.len());
    }

    #[test]
    fn test_module_target_get_json_path() {
        let path = ModuleTarget::get_json_path("test/module.star");
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("build/spaces-module-targets"));
        assert!(path_str.ends_with("test/module.star.json"));
    }

    #[test]
    fn test_module_target_new_from_json_nonexistent() {
        let result = ModuleTarget::new_from_json("nonexistent/module.star").unwrap();
        assert!(result.is_none());
    }

    // ===== ModuleDeps Tests =====

    #[test]
    fn test_module_deps_add_load() {
        let mut deps = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: Vec::new(),
            checkout_state_digest: "abc123".into(),
        };
        deps.add_load(LoadStatement::new("lib/common.star".into()));
        assert_eq!(deps.loads.len(), 1);
        assert_eq!(deps.loads[0].module_id.as_ref(), "lib/common.star");
    }

    #[test]
    fn test_module_deps_set_loads_sorts() {
        let mut deps = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: Vec::new(),
            checkout_state_digest: "abc123".into(),
        };

        let loads = vec![
            LoadStatement::new("lib/zzz.star".into()),
            LoadStatement::new("lib/aaa.star".into()),
            LoadStatement::new("lib/mmm.star".into()),
        ];

        deps.set_loads(loads);
        assert_eq!(deps.loads.len(), 3);
        assert_eq!(deps.loads[0].module_id.as_ref(), "lib/aaa.star");
        assert_eq!(deps.loads[1].module_id.as_ref(), "lib/mmm.star");
        assert_eq!(deps.loads[2].module_id.as_ref(), "lib/zzz.star");
    }

    #[test]
    fn test_module_deps_get_json_path() {
        let path = ModuleDeps::get_json_path("test/module.star");
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("build/spaces-module-deps"));
        assert!(path_str.ends_with("test/module.star.json"));
    }

    #[test]
    fn test_module_deps_new_from_json_nonexistent() {
        let result = ModuleDeps::new_from_json("nonexistent/module.star").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_module_deps_compute_digest_success() {
        let mut deps = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: vec![LoadStatement::new("lib/common.star".into())],
            checkout_state_digest: "checkout_abc123".into(),
        };
        deps.set_loads(deps.loads.clone());

        let mut star_files = HashMap::new();
        star_files.insert(
            Arc::from("test/module.star"),
            bin_detail::BinDetail {
                hash: [1u8; 32],
                modified: None,
            },
        );
        star_files.insert(
            Arc::from("lib/common.star"),
            bin_detail::BinDetail {
                hash: [2u8; 32],
                modified: None,
            },
        );

        let digest = deps.compute_digest(&star_files).unwrap();
        assert!(!digest.is_empty());

        // Verify digest is deterministic
        let digest2 = deps.compute_digest(&star_files).unwrap();
        assert_eq!(digest, digest2);
    }

    #[test]
    fn test_module_deps_compute_digest_missing_module() {
        let deps = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: vec![],
            checkout_state_digest: "abc123".into(),
        };

        let star_files = HashMap::new();
        let result = deps.compute_digest(&star_files);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_module_deps_compute_digest_missing_load() {
        let deps = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: vec![LoadStatement::new("lib/missing.star".into())],
            checkout_state_digest: "abc123".into(),
        };

        let mut star_files = HashMap::new();
        star_files.insert(
            Arc::from("test/module.star"),
            bin_detail::BinDetail {
                hash: [1u8; 32],
                modified: None,
            },
        );

        let result = deps.compute_digest(&star_files);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_module_deps_compute_digest_different_hashes() {
        let deps = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: vec![],
            checkout_state_digest: "abc123".into(),
        };

        let mut star_files1 = HashMap::new();
        star_files1.insert(
            Arc::from("test/module.star"),
            bin_detail::BinDetail {
                hash: [1u8; 32],
                modified: None,
            },
        );

        let mut star_files2 = HashMap::new();
        star_files2.insert(
            Arc::from("test/module.star"),
            bin_detail::BinDetail {
                hash: [2u8; 32],
                modified: None,
            },
        );

        let digest1 = deps.compute_digest(&star_files1).unwrap();
        let digest2 = deps.compute_digest(&star_files2).unwrap();
        assert_ne!(digest1, digest2);
    }

    #[test]
    fn test_module_deps_compute_digest_different_checkout_state() {
        let deps1 = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: vec![],
            checkout_state_digest: "abc123".into(),
        };

        let deps2 = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: vec![],
            checkout_state_digest: "xyz789".into(),
        };

        let mut star_files = HashMap::new();
        star_files.insert(
            Arc::from("test/module.star"),
            bin_detail::BinDetail {
                hash: [1u8; 32],
                modified: None,
            },
        );

        let digest1 = deps1.compute_digest(&star_files).unwrap();
        let digest2 = deps2.compute_digest(&star_files).unwrap();
        assert_ne!(digest1, digest2);
    }

    #[test]
    fn test_module_deps_digest_from_inputs() {
        let platform = platform::Platform::LinuxX86_64;
        let is_ci = true;
        let store_values = vec![
            (Arc::from("key1"), Arc::from("value1")),
            (Arc::from("key2"), Arc::from("value2")),
        ];
        let env_values = vec![
            (Arc::from("ENV1"), Arc::from("val1")),
            (Arc::from("ENV2"), Arc::from("val2")),
        ];

        let digest =
            ModuleDeps::digest_from_inputs(platform, is_ci, &store_values, &env_values).unwrap();

        assert!(!digest.is_empty());

        // Verify digest is deterministic
        let digest2 =
            ModuleDeps::digest_from_inputs(platform, is_ci, &store_values, &env_values).unwrap();
        assert_eq!(digest, digest2);
    }

    #[test]
    fn test_module_deps_digest_from_inputs_different_platform() {
        let store_values = vec![];
        let env_values = vec![];

        let digest1 = ModuleDeps::digest_from_inputs(
            platform::Platform::LinuxX86_64,
            false,
            &store_values,
            &env_values,
        )
        .unwrap();

        let digest2 = ModuleDeps::digest_from_inputs(
            platform::Platform::MacosAarch64,
            false,
            &store_values,
            &env_values,
        )
        .unwrap();

        assert_ne!(digest1, digest2);
    }

    #[test]
    fn test_module_deps_digest_from_inputs_different_is_ci() {
        let platform = platform::Platform::LinuxX86_64;
        let store_values = vec![];
        let env_values = vec![];

        let digest1 =
            ModuleDeps::digest_from_inputs(platform, false, &store_values, &env_values).unwrap();

        let digest2 =
            ModuleDeps::digest_from_inputs(platform, true, &store_values, &env_values).unwrap();

        assert_ne!(digest1, digest2);
    }

    #[test]
    fn test_module_deps_digest_from_inputs_different_store_values() {
        let platform = platform::Platform::LinuxX86_64;
        let env_values = vec![];

        let store_values1 = vec![(Arc::from("key1"), Arc::from("value1"))];
        let store_values2 = vec![(Arc::from("key1"), Arc::from("value2"))];

        let digest1 =
            ModuleDeps::digest_from_inputs(platform, false, &store_values1, &env_values).unwrap();

        let digest2 =
            ModuleDeps::digest_from_inputs(platform, false, &store_values2, &env_values).unwrap();

        assert_ne!(digest1, digest2);
    }

    #[test]
    fn test_module_deps_digest_from_inputs_different_env_values() {
        let platform = platform::Platform::LinuxX86_64;
        let store_values = vec![];

        let env_values1 = vec![(Arc::from("ENV1"), Arc::from("val1"))];
        let env_values2 = vec![(Arc::from("ENV1"), Arc::from("val2"))];

        let digest1 =
            ModuleDeps::digest_from_inputs(platform, false, &store_values, &env_values1).unwrap();

        let digest2 =
            ModuleDeps::digest_from_inputs(platform, false, &store_values, &env_values2).unwrap();

        assert_ne!(digest1, digest2);
    }

    #[test]
    fn test_module_deps_serde() {
        let deps = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: vec![
                LoadStatement::new("lib/common.star".into()),
                LoadStatement::new("lib/utils.star".into()),
            ],
            checkout_state_digest: "abc123".into(),
        };

        let json = serde_json::to_string_pretty(&deps).unwrap();
        let deserialized: ModuleDeps = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.module_name, deps.module_name);
        assert_eq!(deserialized.loads.len(), deps.loads.len());
        assert_eq!(
            deserialized.checkout_state_digest,
            deps.checkout_state_digest
        );
    }

    // ===== Helper Function Tests =====

    #[test]
    fn test_get_json_path() {
        let path = get_json_path("some/dir", "module/path.star");
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("some/dir"));
        assert!(path_str.ends_with("module/path.star.json"));
    }

    #[test]
    fn test_get_json_path_different_dirs() {
        let path1 = get_json_path("dir1", "module.star");
        let path2 = get_json_path("dir2", "module.star");
        assert_ne!(path1, path2);
    }

    #[test]
    fn test_get_existing_json_path_nonexistent() {
        let path = get_existing_json_path("nonexistent/dir", "nonexistent.star");
        assert!(path.is_none());
    }

    // ===== Constants Tests =====

    #[test]
    fn test_constants() {
        assert_eq!(MODULE_TARGETS_DIR, "build/spaces-module-targets");
        assert_eq!(MODULE_DEPS_DIR, "build/spaces-module-deps");
        assert_eq!(MODULE_RESULTS_SUFFIX, ".json");
    }
}
