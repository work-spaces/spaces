use crate::{executor, rules};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::GlobalsBuilder;
use std::sync::RwLock;

struct State {
    workspace_path: Option<String>,
    new_branch_name: Option<String>,
    env: executor::env::UpdateEnv,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(RwLock::new(State {
        workspace_path: None,
        new_branch_name: None,
        env: executor::env::UpdateEnv {
            vars: std::collections::HashMap::new(),
            paths: Vec::new(),
        },
    }));
    STATE.get()
}

fn get_unique() -> anyhow::Result<String> {
    let duration_since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context(format_context!("No system time"))?;
    let duration_since_epoch_string = format!("{}", duration_since_epoch.as_nanos());
    let unique_sha256 = sha256::digest(duration_since_epoch_string.as_bytes());
    Ok(unique_sha256.as_str()[0..4].to_string())
}

pub fn set_workspace_path(path: String) -> anyhow::Result<()> {
    let mut state = get_state().write().unwrap();
    let unique = get_unique().context(format_context!("failed to get unique marker"))?;
    state.new_branch_name = Some(format!("{path}-{}", unique));
    state.workspace_path = Some(path);
    Ok(())
}

pub fn update_env(env: executor::env::UpdateEnv) -> anyhow::Result<()> {
    let mut state = get_state().write().unwrap();
    state.env.vars.extend(env.vars);
    state.env.paths.extend(env.paths);
    Ok(())
}

pub fn get_env() -> executor::env::UpdateEnv {
    let state = get_state().read().unwrap();
    state.env.clone()
}

pub fn get_store_path() -> String {
    let home = std::env::var("HOME")
        .context(format_context!("Failed to get HOME environment variable"))
        .unwrap();
    format!("{home}/.spaces/store")
}

pub fn get_workspace_path() -> Option<String> {
    let state = get_state().read().unwrap();
    state.workspace_path.clone()
}

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn platform_name() -> anyhow::Result<String> {
        platform::Platform::get_platform()
            .map(|p| p.to_string())
            .ok_or(anyhow::anyhow!("Failed to get platform name"))
    }

    fn store_path() -> anyhow::Result<String> {
        Ok(get_store_path())
    }

    fn absolute_workspace_path() -> anyhow::Result<String> {
        get_workspace_path().ok_or(format_error!("Workspace path not set"))
    }

    fn current_workspace_path() -> anyhow::Result<String> {
        let state = rules::get_state().read().unwrap();
        if let Some(latest) = state.latest_starlark_module.as_ref() {
            let path = std::path::Path::new(latest.as_str());
            let parent = path
                .parent()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or(String::new());
            Ok(parent)
        } else {
            Err(format_error!("No starlark module set"))
        }
    }

    fn get_archive_output(
        #[starlark(require = named)] archive: starlark::values::Value,
    ) -> anyhow::Result<String> {
        let create_archive: easy_archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let workspace_directory =
            get_workspace_path().ok_or(format_error!("Workspace not available"))?;

        Ok(format!(
            "{workspace_directory}/build/{}",
            create_archive.get_output_file()
        ))
    }
}
