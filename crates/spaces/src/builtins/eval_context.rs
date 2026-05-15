use crate::{singleton, task, workspace};
use std::cell::RefCell;
use std::sync::Arc;
use utils::{mtarget, rule};

/// Per-evaluation context passed to builtin functions via `eval.extra_mut`.
///
/// This replaces the global singleton pattern for per-module state, enabling
/// parallel evaluation of starlark modules. Each module evaluation gets its
/// own `EvalContext` carrying the workspace reference and the current module
/// name, so multiple modules can be evaluated concurrently without races.
///
/// PERFORMANCE: Frequently-accessed workspace state is cached here at context
/// creation time to avoid repeated RwLock acquisitions during evaluation.
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
    /// Uses RefCell for interior mutability since callers borrow other ctx fields
    /// while recording tasks.
    created_rules: RefCell<Vec<Arc<str>>>,

    /// Load statements captured during evaluation.
    /// Used for module result caching to track module dependencies.
    load_statements: Vec<mtarget::LoadStatement>,

    // === Cached workspace state for performance ===
    // These fields cache immutable or rarely-changed workspace state
    // to avoid repeated lock acquisitions during evaluation.
    /// Cached absolute path to workspace root
    pub workspace_absolute_path: Arc<str>,

    /// Cached path to store directory
    pub workspace_store_path: Arc<str>,

    /// Cached path to spaces tools directory
    pub workspace_spaces_tools_path: Arc<str>,

    /// Cached path to cargo binstall root directory
    pub workspace_cargo_binstall_root: Arc<str>,

    /// Cached workspace digest (empty if not available)
    pub workspace_digest: Arc<str>,

    /// Cached workspace short digest
    pub workspace_short_digest: Arc<str>,

    /// Whether workspace is reproducible
    pub workspace_is_reproducible: bool,
}

// SAFETY: All fields are 'static (Arc, bool, enum, RefCell<Vec<...>>, Vec<...>) so EvalContext is 'static.
unsafe impl<'a> starlark::any::ProvidesStaticType<'a> for EvalContext {
    type StaticType = Self;
}

impl EvalContext {
    pub fn new(workspace: Option<workspace::WorkspaceArc>, module_name: Arc<str>) -> Self {
        // Cache frequently-accessed workspace state to avoid lock contention
        let (
            absolute_path,
            store_path,
            spaces_tools_path,
            cargo_binstall_root,
            digest,
            short_digest,
            is_reproducible,
        ) = if let Some(ref ws) = workspace {
            let ws_read = ws.read();
            (
                ws_read.get_absolute_path(),
                ws_read.get_store_path(),
                ws_read.get_spaces_tools_path(),
                ws_read.get_cargo_binstall_root(),
                ws_read
                    .settings
                    .json
                    .digest
                    .clone()
                    .unwrap_or_else(|| Arc::from("")),
                ws_read.get_short_digest(),
                ws_read.is_reproducible(),
            )
        } else {
            (
                Arc::from("."),
                Arc::from("."),
                Arc::from("."),
                Arc::from("."),
                Arc::from(""),
                Arc::from(""),
                false,
            )
        };

        Self {
            workspace,
            module_name,
            default_module_visibility: rule::Visibility::Public,
            is_checkout: singleton::get_is_checkout(),
            is_sync: singleton::get_is_sync(),
            is_lsp: singleton::is_lsp_mode(),
            is_ci: singleton::get_is_ci(),
            execution_phase: singleton::get_execution_phase(),
            created_rules: RefCell::new(Vec::new()),
            load_statements: Vec::new(),
            workspace_absolute_path: absolute_path,
            workspace_store_path: store_path,
            workspace_spaces_tools_path: spaces_tools_path,
            workspace_cargo_binstall_root: cargo_binstall_root,
            workspace_digest: digest,
            workspace_short_digest: short_digest,
            workspace_is_reproducible: is_reproducible,
        }
    }

    /// Records that a task was created during this module's evaluation.
    pub fn record_rule(&self, task_name: Arc<str>) {
        self.created_rules.borrow_mut().push(task_name);
    }

    /// Returns the list of rule names created during this module's evaluation.
    pub fn get_created_rules(&self) -> Vec<Arc<str>> {
        self.created_rules.borrow().clone()
    }

    /// Sets the load statements for this module.
    pub fn set_load_statements(&mut self, loads: Vec<mtarget::LoadStatement>) {
        self.load_statements = loads;
    }

    /// Returns the load statements captured for this module.
    pub fn get_load_statements(&self) -> &[mtarget::LoadStatement] {
        &self.load_statements
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
