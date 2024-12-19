use crate::workspace;
use anyhow_source_location::format_error;



#[derive(Debug)]
struct State {
    active_workspace: Option<workspace::WorkspaceArc>,
    is_ci: bool,
    max_queue_count: i64,
}

static STATE: state::InitCell<state_lock::StateLock<State>> = state::InitCell::new();

fn get_state() -> &'static state_lock::StateLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(state_lock::StateLock::new(State {
        is_ci: false,
        max_queue_count: 8,
        active_workspace: None,
    }));

    STATE.get()
}

pub fn get_max_queue_count() -> i64 {
    let state = get_state().read();
    state.max_queue_count
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
    state.active_workspace.clone().ok_or(format_error!("Internal Error: No active workspace"))
}
