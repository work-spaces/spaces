use crate::workspace;
use anyhow_source_location::format_error;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug)]
struct State {
    active_workspace: Option<workspace::WorkspaceArc>,
    is_ci: bool,
    is_rescan: bool,
    max_queue_count: i64,
    error_chain: Vec<String>,
    inspect_globs: HashSet<Arc<str>>,
    has_help: bool,
    inspect_markdown_path: Option<Arc<str>>,
    inspect_stardoc_path: Option<Arc<str>>,
    glob_warnings: Vec<Arc<str>>,
}

static STATE: state::InitCell<lock::StateLock<State>> = state::InitCell::new();

fn get_state() -> &'static lock::StateLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(lock::StateLock::new(State {
        is_ci: false,
        is_rescan: false,
        max_queue_count: 8,
        active_workspace: None,
        error_chain: Vec::new(),
        inspect_globs: HashSet::new(),
        has_help: false,
        inspect_markdown_path: None,
        inspect_stardoc_path: None,
        glob_warnings: Vec::new(),
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
