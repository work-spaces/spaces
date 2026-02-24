use crate::{task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use utils::lock;

#[derive(Debug)]
struct State {
    active_workspace: Option<workspace::WorkspaceArc>,
    is_sync: bool,
    is_ci: bool,
    is_logging_disabled: bool,
    is_rescan: bool,
    is_lsp: bool,
    is_skip_deps: bool,
    max_queue_count: i64,
    error_chain: Vec<String>,
    args_env: HashMap<Arc<str>, Arc<str>>,
    new_branches: Vec<Arc<str>>,
    inspect_globs: HashSet<Arc<str>>,
    has_help: bool,
    inspect_markdown_path: Option<Arc<str>>,
    inspect_stardoc_path: Option<Arc<str>>,
    glob_warnings: Vec<Arc<str>>,
    execution_phase: task::Phase,
}

static STATE: state::InitCell<lock::StateLock<State>> = state::InitCell::new();

pub fn get_spaces_version() -> anyhow::Result<semver::Version> {
    let current_version = env!("CARGO_PKG_VERSION");
    let version = current_version
        .parse::<semver::Version>()
        .context(format_context!("Internal error: bad version for spaces"))?;
    Ok(version)
}

fn get_state() -> &'static lock::StateLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    STATE.set(lock::StateLock::new(State {
        is_ci: false,
        is_logging_disabled: false,
        is_sync: false,
        is_rescan: false,
        is_lsp: false,
        is_skip_deps: false,
        max_queue_count: 8,
        active_workspace: None,
        error_chain: Vec::new(),
        inspect_globs: HashSet::new(),
        has_help: false,
        new_branches: Vec::new(),
        inspect_markdown_path: None,
        inspect_stardoc_path: None,
        args_env: HashMap::new(),
        glob_warnings: Vec::new(),
        execution_phase: task::Phase::Complete,
    }));

    STATE.get()
}

pub fn process_anyhow_error(error: anyhow::Error) {
    let mut state = get_state().write();
    for cause in error.chain().rev() {
        state.error_chain.push(cause.to_string());
    }
}

pub fn process_error(error: String) {
    let mut state = get_state().write();
    state.error_chain.push(error);
}

pub fn show_error_chain() {
    let mut state = get_state().write();
    let args = std::env::args().collect::<Vec<String>>();
    eprintln!("While executing: {}", args.join(" "));
    state.error_chain.reverse();
    for (offset, error) in state.error_chain.iter().enumerate() {
        let show_error = error.to_string().replace('\n', "\n    ");
        eprintln!("  [{offset}]: {show_error}");
    }
}

pub fn push_glob_warning(warning: Arc<str>) {
    let mut state = get_state().write();
    state.glob_warnings.push(warning);
}

pub fn get_glob_warnings() -> Vec<Arc<str>> {
    let state = get_state().read();
    state.glob_warnings.clone()
}

pub fn get_has_help() -> bool {
    let state = get_state().read();
    state.has_help
}

pub fn enable_lsp_mode() {
    let mut state = get_state().write();
    state.is_lsp = true;
}

pub fn is_lsp_mode() -> bool {
    let state = get_state().read();
    state.is_lsp
}

pub fn enable_skip_deps_mode() {
    let mut state = get_state().write();
    state.is_skip_deps = true;
}

pub fn is_skip_deps_mode() -> bool {
    let state = get_state().read();
    state.is_skip_deps
}

pub fn set_execution_phase(phase: task::Phase) {
    let mut state = get_state().write();
    state.execution_phase = phase;
}

pub fn get_execution_phase() -> task::Phase {
    let state = get_state().read();
    state.execution_phase
}

pub fn get_args_env() -> HashMap<Arc<str>, Arc<str>> {
    let state = get_state().read();
    state.args_env.clone()
}

pub fn set_args_env(args: Vec<Arc<str>>) -> anyhow::Result<()> {
    let mut state = get_state().write();
    for env in args.iter() {
        let parts = env.split_once('=');
        if let Some((key, value)) = parts {
            state.args_env.insert(key.into(), value.into());
        } else {
            return Err(format_error!(
                "Bad env argument: `{env}` use `<key>=<value>`"
            ));
        }
    }
    Ok(())
}

pub fn get_new_branches() -> Vec<Arc<str>> {
    let state = get_state().read();
    state.new_branches.clone()
}

pub fn set_new_branches(new_branches: Vec<Arc<str>>) {
    let mut state = get_state().write();
    state.new_branches = new_branches;
}

pub fn is_sync() -> bool {
    let state = get_state().read();
    state.is_sync
}

pub fn set_is_sync() {
    let mut state = get_state().write();
    state.is_sync = true;
}

pub fn set_has_help(has_help: bool) {
    let mut state = get_state().write();
    state.has_help = has_help;
}

pub fn get_max_queue_count() -> i64 {
    let state = get_state().read();
    state.max_queue_count
}

pub fn get_inspect_stardoc_path() -> Option<Arc<str>> {
    let state = get_state().read();
    state.inspect_stardoc_path.clone()
}

pub fn set_inspect_stardoc_path(inspect_stardoc_path: Option<Arc<str>>) {
    let mut state = get_state().write();
    state.inspect_stardoc_path = inspect_stardoc_path;
}

pub fn set_inspect_markdown_path(inspect_markdown_path: Option<Arc<str>>) {
    let mut state = get_state().write();
    state.inspect_markdown_path = inspect_markdown_path;
}

pub fn get_inspect_markdown_path() -> Option<Arc<str>> {
    let state = get_state().read();
    state.inspect_markdown_path.clone()
}

pub fn set_inspect_globs(inspect_globs: HashSet<Arc<str>>) {
    let mut state = get_state().write();
    state.inspect_globs = inspect_globs;
}

pub fn get_inspect_globs() -> HashSet<Arc<str>> {
    let state = get_state().read();
    state.inspect_globs.clone()
}

pub fn get_is_rescan() -> bool {
    let state = get_state().read();
    state.is_rescan
}

pub fn set_logging_disabled(disable_logs: bool) {
    let mut state = get_state().write();
    state.is_logging_disabled = disable_logs;
}

pub fn get_is_logging_disabled() -> bool {
    let state = get_state().read();
    state.is_logging_disabled
}

pub fn set_rescan(is_rescan: bool) {
    let mut state = get_state().write();
    state.is_rescan = is_rescan;
}

pub fn set_max_queue_count(max_queue_count: i64) {
    let mut state = get_state().write();
    state.max_queue_count = max_queue_count;
}

pub fn get_is_ci() -> bool {
    let state = get_state().read();
    state.is_ci
}

pub fn set_ci(is_ci: bool) {
    let mut state = get_state().write();
    state.is_ci = is_ci;
}

pub fn set_active_workspace(workspace: workspace::WorkspaceArc) {
    let mut state = get_state().write();
    state.active_workspace = Some(workspace);
}

pub fn get_workspace() -> anyhow::Result<workspace::WorkspaceArc> {
    let state = get_state().read();
    state
        .active_workspace
        .clone()
        .ok_or(format_error!("Internal Error: No active workspace"))
}
