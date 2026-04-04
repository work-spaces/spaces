use crate::task;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::collections::HashMap;
use std::sync::Arc;
use utils::inspect;
use utils::lock;

#[derive(Debug)]
struct State {
    is_sync: bool,
    is_ci: bool,
    is_checkout: bool,
    is_logging_disabled: bool,
    is_rescan: bool,
    is_lsp: bool,
    is_use_locks: bool,
    is_skip_deps: bool,
    logs_for_failed_rules: Vec<Arc<str>>,
    max_queue_count: i64,
    error_chain: Vec<String>,
    args_env: HashMap<Arc<str>, Arc<str>>,
    args_store: HashMap<Arc<str>, serde_json::Value>,
    args_locks: HashMap<Arc<str>, Arc<str>>,
    new_branches: Vec<Arc<str>>,
    inspect: inspect::Options,
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
        is_checkout: false,
        is_logging_disabled: false,
        is_sync: false,
        is_rescan: false,
        is_lsp: false,
        is_skip_deps: false,
        is_use_locks: false,
        logs_for_failed_rules: Vec::new(),
        max_queue_count: 8,
        error_chain: Vec::new(),
        new_branches: Vec::new(),
        inspect: inspect::Options::default(),
        args_env: HashMap::new(),
        args_store: HashMap::new(),
        args_locks: HashMap::new(),
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

pub fn set_rule_failure(log_files: Vec<Arc<str>>) {
    let mut state = get_state().write();
    state.logs_for_failed_rules = log_files;
}

pub fn has_rule_failure() -> bool {
    let state = get_state().read();
    !state.logs_for_failed_rules.is_empty()
}

pub fn get_logs_for_failed_rules() -> Vec<Arc<str>> {
    let state = get_state().read();
    state.logs_for_failed_rules.clone()
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

pub fn get_args_store() -> HashMap<Arc<str>, serde_json::Value> {
    let state = get_state().read();
    state.args_store.clone()
}

pub fn set_args_store(args: Vec<Arc<str>>) -> anyhow::Result<()> {
    let mut state = get_state().write();
    for arg in args.iter() {
        let parts = arg.split_once('=');
        if let Some((key, value)) = parts {
            state
                .args_store
                .insert(key.into(), serde_json::Value::String(value.to_string()));
        } else {
            return Err(format_error!(
                "Bad store argument: `{arg}` use `<key>=<value>`"
            ));
        }
    }
    Ok(())
}

pub fn set_args_store_from_toml(store: HashMap<Arc<str>, toml::Value>) -> anyhow::Result<()> {
    let mut state = get_state().write();
    for (key, toml_value) in store {
        let json_value = toml_value_to_json(toml_value);
        state.args_store.insert(key, json_value);
    }
    Ok(())
}

fn toml_value_to_json(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(table) => {
            let map = table
                .into_iter()
                .map(|(k, v)| (k, toml_value_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

pub fn get_new_branches() -> Vec<Arc<str>> {
    let state = get_state().read();
    state.new_branches.clone()
}

pub fn set_new_branches(new_branches: Vec<Arc<str>>) {
    let mut state = get_state().write();
    state.new_branches = new_branches;
}

pub fn get_is_sync() -> bool {
    let state = get_state().read();
    state.is_sync
}

pub fn set_is_sync() {
    let mut state = get_state().write();
    state.is_sync = true;
}

pub fn get_max_queue_count() -> i64 {
    let state = get_state().read();
    state.max_queue_count
}

pub fn set_inspect_options(options: inspect::Options) {
    let mut state = get_state().write();
    state.inspect = options;
}

pub fn get_inspect_options() -> inspect::Options {
    let state = get_state().read();
    state.inspect.clone()
}

pub fn get_is_use_locks() -> bool {
    let state = get_state().read();
    state.is_use_locks
}

pub fn set_use_locks() {
    let mut state = get_state().write();
    state.is_use_locks = true;
}

pub fn get_args_locks() -> HashMap<Arc<str>, Arc<str>> {
    let state = get_state().read();
    state.args_locks.clone()
}

pub fn set_args_locks(args: Vec<Arc<str>>) -> anyhow::Result<()> {
    let mut state = get_state().write();
    for lock in args.iter() {
        let parts = lock.split_once('=');
        if let Some((key, value)) = parts {
            state.args_locks.insert(key.into(), value.into());
        } else {
            return Err(format_error!(
                "Bad lock argument: `{lock}` use `--lock=<REPO>=<REV>`"
            ));
        }
    }
    Ok(())
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

pub fn get_is_checkout() -> bool {
    let state = get_state().read();
    state.is_checkout
}

pub fn set_is_checkout() {
    let mut state = get_state().write();
    state.is_checkout = true;
}
