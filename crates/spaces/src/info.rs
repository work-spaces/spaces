use crate::{executor, rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use std::sync::RwLock;

struct State {
    new_branch_name: Option<String>,
    env: executor::env::UpdateEnv,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(RwLock::new(State {
        new_branch_name: None,
        env: executor::env::UpdateEnv {
            vars: std::collections::HashMap::new(),
            paths: vec![format!("{}/sysroot/bin", workspace::absolute_path())],
        },
    }));
    STATE.get()
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

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn platform_name() -> anyhow::Result<String> {
        platform::Platform::get_platform()
            .map(|p| p.to_string())
            .ok_or(anyhow::anyhow!("Failed to get platform name"))
    }

    fn store_path() -> anyhow::Result<String> {
        Ok(workspace::get_store_path())
    }

    fn absolute_workspace_path() -> anyhow::Result<String> {
        Ok(workspace::absolute_path())
    }

    fn set_env(
        #[starlark(require = named)] env: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let mut state = get_state().write().unwrap();

        // support JSON, yaml, and toml
        state.env = serde_json::from_value(env.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        Ok(NoneType)
    }

    fn checkout_path() -> anyhow::Result<String> {
        rules::get_checkout_path()
    }

    fn current_workspace_path() -> anyhow::Result<String> {
        rules::get_checkout_path()
    }

    fn get_archive_output(
        #[starlark(require = named)] archive: starlark::values::Value,
    ) -> anyhow::Result<String> {
        let create_archive: easy_archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let workspace_directory = workspace::absolute_path();

        Ok(format!(
            "{workspace_directory}/build/{}",
            create_archive.get_output_file()
        ))
    }
}
