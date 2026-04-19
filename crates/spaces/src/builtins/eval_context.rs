use crate::{singleton, task, workspace};
use std::sync::{Arc, Mutex};
use utils::{mtarget, rule};

/// Per-evaluation context passed to builtin functions via `eval.extra_mut`.
///
/// This replaces the global singleton pattern for per-module state, enabling
/// parallel evaluation of starlark modules. Each module evaluation gets its
/// own `EvalContext` carrying the workspace reference and the current module
/// name, so multiple modules can be evaluated concurrently without races.
pub struct EvalContext {
    pub workspace: Option<workspace::WorkspaceArc>,
    pub module_name: Arc<str>,
    /// Default visibility applied to rules that don't specify one.
    /// Starts as `Public`; `workspace.set_default_module_visibility_private()`
    /// mutates this via `eval.extra_mut`.
    pub default_module_visibility: rule::Visibility,
    // Snapshot of global flags captured at context-creation time.
    pub is_checkout: bool,
    pub is_sync: bool,
    pub is_lsp: bool,
    pub is_ci: bool,
    pub execution_phase: task::Phase,

    /// Task names created during this module's evaluation.
    /// Used for module result caching to track which tasks originated from this module.
    created_tasks: Mutex<Vec<Arc<str>>>,

    /// Load statements captured during evaluation.
    /// Used for module result caching to track module dependencies.
    load_statements: Mutex<Vec<mtarget::LoadStatement>>,
}

// SAFETY: All fields are 'static (Arc, bool, enum, Mutex<Vec<...>>) so EvalContext is 'static.
unsafe impl<'a> starlark::any::ProvidesStaticType<'a> for EvalContext {
    type StaticType = Self;
}

impl EvalContext {
    pub fn new(workspace: Option<workspace::WorkspaceArc>, module_name: Arc<str>) -> Self {
        Self {
            workspace,
            module_name,
            default_module_visibility: rule::Visibility::Public,
            is_checkout: singleton::get_is_checkout(),
            is_sync: singleton::get_is_sync(),
            is_lsp: singleton::is_lsp_mode(),
            is_ci: singleton::get_is_ci(),
            execution_phase: singleton::get_execution_phase(),
            created_tasks: Mutex::new(Vec::new()),
            load_statements: Mutex::new(Vec::new()),
        }
    }

    /// Records that a task was created during this module's evaluation.
    pub fn record_task(&self, task_name: Arc<str>) {
        if let Ok(mut tasks) = self.created_tasks.lock() {
            tasks.push(task_name);
        }
    }

    /// Returns the list of task names created during this module's evaluation.
    pub fn get_created_tasks(&self) -> Vec<Arc<str>> {
        self.created_tasks
            .lock()
            .map(|tasks| tasks.clone())
            .unwrap_or_default()
    }

    /// Sets the load statements for this module.
    pub fn set_load_statements(&self, loads: Vec<mtarget::LoadStatement>) {
        if let Ok(mut statements) = self.load_statements.lock() {
            *statements = loads;
        }
    }

    /// Returns the load statements captured for this module.
    pub fn get_load_statements(&self) -> Vec<mtarget::LoadStatement> {
        self.load_statements
            .lock()
            .map(|loads| loads.clone())
            .unwrap_or_default()
    }
}

/// Convenience: extract `&EvalContext` from an evaluator's `extra_mut` slot.
pub fn get_eval_context<'a>(
    eval: &'a starlark::eval::Evaluator<'_, '_, '_>,
) -> anyhow::Result<&'a EvalContext> {
    eval.extra_mut
        .as_deref()
        .and_then(|e| e.downcast_ref::<EvalContext>())
        .ok_or_else(|| anyhow::anyhow!("Internal error: no EvalContext in evaluator"))
}

/// Convenience: extract `&mut EvalContext` from an evaluator's `extra_mut` slot.
pub fn get_eval_context_mut<'a>(
    eval: &'a mut starlark::eval::Evaluator<'_, '_, '_>,
) -> anyhow::Result<&'a mut EvalContext> {
    eval.extra_mut
        .as_deref_mut()
        .and_then(|e| e.downcast_mut::<EvalContext>())
        .ok_or_else(|| anyhow::anyhow!("Internal error: no EvalContext in evaluator"))
}
