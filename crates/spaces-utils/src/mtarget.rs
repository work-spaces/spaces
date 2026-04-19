//! Module evaluation result caching.
//!
//! This module provides data structures for capturing and persisting
//! the results of evaluating Starlark modules. This enables caching
//! of module evaluation results to avoid re-evaluation when the
//! module and its dependencies haven't changed.

use crate::{changes, rule};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Directory where module evaluation results are saved (relative to workspace root).
pub const MODULE_RESULTS_DIR: &str = "build/spaces-modules";

/// File extension for module result files.
pub const MODULE_RESULTS_SUFFIX: &str = ".spaces.json";

/// Represents the result of evaluating a single Starlark module.
/// This structure captures all information needed for caching and replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleEvaluationResult {
    /// The module path (e.g., "spaces/spaces.star" or "//repo:spaces.star")
    pub module_name: Arc<str>,

    /// List of load statements - modules that this module depends on
    pub loads: Vec<LoadStatement>,

    /// Tasks created during evaluation of this module.
    /// Keys are task names (rule labels like "//repo:task_name")
    pub tasks: HashMap<Arc<str>, TaskSummary>,
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
pub struct TaskSummary {
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

impl ModuleEvaluationResult {
    /// Creates a new ModuleEvaluationResult.
    pub fn new(module_name: Arc<str>) -> Self {
        Self {
            module_name,
            loads: Vec::new(),
            tasks: HashMap::new(),
        }
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

    /// Adds a task summary to the result.
    pub fn add_task(&mut self, task: TaskSummary) {
        self.tasks.insert(task.name.clone(), task);
    }

    /// Computes a unique digest for this module evaluation result.
    ///
    /// The digest incorporates:
    /// - The hash of the original module file
    /// - The hashes of all files loaded via load() statements
    ///
    /// This allows cache invalidation when any input file changes.
    ///
    /// # Arguments
    /// * `changes` - The Changes struct containing file hashes
    ///
    /// # Returns
    /// A blake3 digest string, or an error if required files aren't found in changes.
    pub fn compute_digest(&self, changes: &changes::Changes) -> anyhow::Result<Arc<str>> {
        use crate::changes::ChangeDetailType;

        let mut hasher = blake3::Hasher::new();

        // Hash the module file itself (module_name is already a relative workspace path)
        if let Some(detail) = changes.entries.get(self.module_name.as_ref()) {
            if let ChangeDetailType::File(hash) = &detail.detail_type {
                hasher.update(hash.as_bytes());
            }
        } else {
            return Err(format_error!(
                "Internal Error: Module file '{}' not found in changes",
                self.module_name
            ));
        }

        // Iterate over loads (assumed to be sorted via set_loads)
        for load in &self.loads {
            if let Some(detail) = changes.entries.get(&load.module_id) {
                if let ChangeDetailType::File(hash) = &detail.detail_type {
                    hasher.update(hash.as_bytes());
                } else {
                    return Err(format_error!(
                        "Load module '{}' is not of type file in changes",
                        load.module_id
                    ));
                }
            } else {
                return Err(format_error!(
                    "Internal Error: Load module '{}' not found in changes",
                    load.module_id
                ));
            }
        }

        Ok(hasher.finalize().to_string().into())
    }
}

impl LoadStatement {
    /// Creates a new LoadStatement.
    pub fn new(module_id: Arc<str>) -> Self {
        Self { module_id }
    }
}

impl TaskSummary {
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

/// Saves a module evaluation result to the build directory.
///
/// The file is saved to `<workspace_path>/build/spaces-modules/<sanitized_module_name>.spaces.json`
pub fn save_module_result(
    workspace_path: &str,
    result: &ModuleEvaluationResult,
) -> anyhow::Result<()> {
    let cache_dir = format!("{workspace_path}/{MODULE_RESULTS_DIR}");
    std::fs::create_dir_all(&cache_dir)
        .context(format_context!("Failed to create module results directory"))?;

    // Create filename from module name (sanitized)
    // Strip workspace_path prefix to get relative path for the filename
    let relative_name = result
        .module_name
        .strip_prefix(workspace_path)
        .map(|s| s.strip_prefix('/').unwrap_or(s))
        .unwrap_or(result.module_name.as_ref());
    let safe_name = relative_name.replace("/", "_").replace(":", "_");
    let file_path = format!("{cache_dir}/{safe_name}{MODULE_RESULTS_SUFFIX}");

    let content = serde_json::to_string_pretty(&result)
        .context(format_context!("Failed to serialize module result"))?;

    std::fs::write(&file_path, content).context(format_context!(
        "Failed to write module result to {file_path}"
    ))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_evaluation_result_new() {
        let result = ModuleEvaluationResult::new("test/module.star".into());
        assert_eq!(result.module_name.as_ref(), "test/module.star");
        assert!(result.loads.is_empty());
        assert!(result.tasks.is_empty());
    }

    #[test]
    fn test_add_load() {
        let mut result = ModuleEvaluationResult::new("test/module.star".into());
        result.add_load(LoadStatement::new("lib/common.star".into()));
        assert_eq!(result.loads.len(), 1);
        assert_eq!(result.loads[0].module_id.as_ref(), "lib/common.star");
    }

    #[test]
    fn test_add_task() {
        let mut result = ModuleEvaluationResult::new("test/module.star".into());
        result.add_task(TaskSummary::new(
            "//test:build".into(),
            "Run".into(),
            rule::Visibility::Public,
            serde_json::json!({"name": "//test:build"}),
        ));
        assert_eq!(result.tasks.len(), 1);
        assert!(result.tasks.contains_key(&Arc::from("//test:build")));
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut result = ModuleEvaluationResult::new("test/module.star".into());
        result.add_load(LoadStatement::new("lib/common.star".into()));
        result.add_task(TaskSummary::new(
            "//test:build".into(),
            "Run".into(),
            rule::Visibility::Private,
            serde_json::json!({"executor": "Target", "phase": "Run"}),
        ));

        let json = serde_json::to_string_pretty(&result).unwrap();
        let deserialized: ModuleEvaluationResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.module_name, result.module_name);
        assert_eq!(deserialized.loads.len(), result.loads.len());
        assert_eq!(deserialized.tasks.len(), result.tasks.len());
    }
}
