//! Module evaluation result caching.
//!
//! This module provides data structures for capturing and persisting
//! the results of evaluating Starlark modules. This enables caching
//! of module evaluation results to avoid re-evaluation when the
//! module and its dependencies haven't changed.

use anyhow::Context;
use anyhow_source_location::format_context;
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

    /// Blake3 hash of the module's source content
    pub content_hash: Arc<str>,

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
    /// The module path as written in the load() call
    pub module_id: Arc<str>,

    /// The resolved absolute path to the loaded module
    pub resolved_path: Arc<str>,

    /// Blake3 hash of the loaded module's content
    pub content_hash: Arc<str>,
}

/// Summary of a task created during module evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSummary {
    /// The full task/rule name (label)
    pub name: Arc<str>,

    /// The task phase as a string (e.g., "Checkout", "Run", "Inspect")
    pub phase: Arc<str>,

    /// Serialized task data for replay/inspection
    pub task_json: serde_json::Value,
}

impl ModuleEvaluationResult {
    /// Creates a new ModuleEvaluationResult.
    pub fn new(module_name: Arc<str>, content_hash: Arc<str>) -> Self {
        Self {
            module_name,
            content_hash,
            loads: Vec::new(),
            tasks: HashMap::new(),
        }
    }

    /// Adds a load statement to the result.
    pub fn add_load(&mut self, load: LoadStatement) {
        self.loads.push(load);
    }

    /// Adds a task summary to the result.
    pub fn add_task(&mut self, task: TaskSummary) {
        self.tasks.insert(task.name.clone(), task);
    }

    /// Computes a combined hash of this module and all its loads.
    /// This can be used to determine if the module needs re-evaluation.
    pub fn compute_combined_hash(&self) -> Arc<str> {
        let mut hasher = blake3::Hasher::new();

        // Include the module's own content hash
        hasher.update(self.content_hash.as_bytes());

        // Include all load hashes in a deterministic order
        let mut load_hashes: Vec<&str> =
            self.loads.iter().map(|l| l.content_hash.as_ref()).collect();
        load_hashes.sort();
        for hash in load_hashes {
            hasher.update(hash.as_bytes());
        }

        hasher.finalize().to_string().into()
    }
}

impl LoadStatement {
    /// Creates a new LoadStatement.
    pub fn new(module_id: Arc<str>, resolved_path: Arc<str>, content_hash: Arc<str>) -> Self {
        Self {
            module_id,
            resolved_path,
            content_hash,
        }
    }
}

impl TaskSummary {
    /// Creates a new TaskSummary.
    pub fn new(name: Arc<str>, phase: Arc<str>, task_json: serde_json::Value) -> Self {
        Self {
            name,
            phase,
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
    let safe_name = result.module_name.replace("/", "_").replace(":", "_");
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
        let result = ModuleEvaluationResult::new("test/module.star".into(), "abc123".into());
        assert_eq!(result.module_name.as_ref(), "test/module.star");
        assert_eq!(result.content_hash.as_ref(), "abc123");
        assert!(result.loads.is_empty());
        assert!(result.tasks.is_empty());
    }

    #[test]
    fn test_add_load() {
        let mut result = ModuleEvaluationResult::new("test/module.star".into(), "abc123".into());
        result.add_load(LoadStatement::new(
            "//lib:common.star".into(),
            "/workspace/lib/common.star".into(),
            "def456".into(),
        ));
        assert_eq!(result.loads.len(), 1);
        assert_eq!(result.loads[0].module_id.as_ref(), "//lib:common.star");
    }

    #[test]
    fn test_add_task() {
        let mut result = ModuleEvaluationResult::new("test/module.star".into(), "abc123".into());
        result.add_task(TaskSummary::new(
            "//test:build".into(),
            "Run".into(),
            serde_json::json!({"name": "//test:build"}),
        ));
        assert_eq!(result.tasks.len(), 1);
        assert!(result.tasks.contains_key(&Arc::from("//test:build")));
    }

    #[test]
    fn test_compute_combined_hash() {
        let mut result = ModuleEvaluationResult::new("test/module.star".into(), "abc123".into());
        result.add_load(LoadStatement::new(
            "//lib:a.star".into(),
            "/workspace/lib/a.star".into(),
            "hash_a".into(),
        ));
        result.add_load(LoadStatement::new(
            "//lib:b.star".into(),
            "/workspace/lib/b.star".into(),
            "hash_b".into(),
        ));

        let hash1 = result.compute_combined_hash();

        // Same loads in different order should produce same hash
        let mut result2 = ModuleEvaluationResult::new("test/module.star".into(), "abc123".into());
        result2.add_load(LoadStatement::new(
            "//lib:b.star".into(),
            "/workspace/lib/b.star".into(),
            "hash_b".into(),
        ));
        result2.add_load(LoadStatement::new(
            "//lib:a.star".into(),
            "/workspace/lib/a.star".into(),
            "hash_a".into(),
        ));

        let hash2 = result2.compute_combined_hash();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut result = ModuleEvaluationResult::new("test/module.star".into(), "abc123".into());
        result.add_load(LoadStatement::new(
            "//lib:common.star".into(),
            "/workspace/lib/common.star".into(),
            "def456".into(),
        ));
        result.add_task(TaskSummary::new(
            "//test:build".into(),
            "Run".into(),
            serde_json::json!({"executor": "Target", "phase": "Run"}),
        ));

        let json = serde_json::to_string_pretty(&result).unwrap();
        let deserialized: ModuleEvaluationResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.module_name, result.module_name);
        assert_eq!(deserialized.content_hash, result.content_hash);
        assert_eq!(deserialized.loads.len(), result.loads.len());
        assert_eq!(deserialized.tasks.len(), result.tasks.len());
    }
}
