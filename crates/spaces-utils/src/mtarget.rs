//! Module Target creation (spaces modules to JSON compilation).
//!
//! This module provides data structures for capturing and persisting
//! the results of evaluating Starlark modules. This enables compiling
//! spaces modules to JSON to avoid re-evaluation when the
//! module and its dependencies haven't changed.

use crate::{platform, rule, ws};
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

    Some(path.into())
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

    pub platform: platform::Platform,

    /// needed because info.is_ci() can affect evaluation
    pub is_ci: bool,

    /// store values
    pub store_values: Vec<(Arc<str>, Arc<str>)>,

    /// env values
    pub env_values: Vec<(Arc<str>, Arc<str>)>,
}

impl ModuleDeps {
    fn sort_values(values: HashMap<Arc<str>, Arc<str>>) -> Vec<(Arc<str>, Arc<str>)> {
        let mut values: Vec<_> = values.into_iter().collect();
        values.sort_by(|(left_key, left_value), (right_key, right_value)| {
            left_key
                .cmp(right_key)
                .then_with(|| left_value.cmp(right_value))
        });
        values
    }

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
        star_files: &HashMap<Arc<str>, ws::BinDetail>,
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

        let dependency_inputs = serde_json::to_vec(&(
            self.platform,
            self.is_ci,
            &self.store_values,
            &self.env_values,
        ))
        .context(format_context!(
            "Failed to serialize module dependency inputs for {}",
            self.module_name
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

    pub fn set_env_values(&mut self, env_values: HashMap<Arc<str>, Arc<str>>) {
        self.env_values = Self::sort_values(env_values);
    }

    pub fn set_store_values(&mut self, store_values: HashMap<Arc<str>, Arc<str>>) {
        self.store_values = Self::sort_values(store_values);
    }

    /// Saves this module evaluation result to the build directory.
    ///
    /// The file is saved to `<workspace_path>/build/spaces-modules/<module_path>.json`
    /// mirroring the workspace directory structure (e.g., `spaces/spaces.star` becomes
    /// `build/spaces-modules/spaces/spaces.star.json`).
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
    fn test_module_deps_sorts_env_and_store_values() {
        let mut deps = ModuleDeps {
            module_name: "test/module.star".into(),
            loads: Vec::new(),
            platform: platform::Platform::MacosAarch64,
            is_ci: false,
            store_values: Vec::new(),
            env_values: Vec::new(),
        };

        deps.set_env_values(HashMap::from([
            (Arc::from("Z_KEY"), Arc::from("z")),
            (Arc::from("A_KEY"), Arc::from("a")),
        ]));
        deps.set_store_values(HashMap::from([
            (Arc::from("store-z"), Arc::from("z")),
            (Arc::from("store-a"), Arc::from("a")),
        ]));

        assert_eq!(
            deps.env_values,
            vec![
                (Arc::from("A_KEY"), Arc::from("a")),
                (Arc::from("Z_KEY"), Arc::from("z")),
            ]
        );
        assert_eq!(
            deps.store_values,
            vec![
                (Arc::from("store-a"), Arc::from("a")),
                (Arc::from("store-z"), Arc::from("z")),
            ]
        );
    }
}
