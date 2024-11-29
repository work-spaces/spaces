use crate::{executor, rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use starstd::Function;
use std::sync::RwLock;

struct State {
    #[allow(dead_code)]
    new_branch_name: Option<String>,
    env: executor::env::UpdateEnv,
    is_ci: bool
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
            paths: Vec::new(),
        },
        is_ci: false,
    }));
    STATE.get()
}

pub fn set_ci_true() {
    let mut state = get_state().write().unwrap();
    state.is_ci = true;
}

pub fn get_is_ci() -> bool {
    let state = get_state().read().unwrap();
    state.is_ci
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

pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "get_platform_name",
        description: "returns the name of the current platform: macos-aarch64|macos-x86_64|linux-x86_64|linux-aarch64|windows-x86_64|windows-aarch64",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "is_platform_windows",
        description: "returns true if platform is Windows",
        return_type: "bool",
        args: &[],
        example: None,
    },
    Function {
        name: "is_platform_macos",
        description: "returns true if platform is macos",
        return_type: "bool",
        args: &[],
        example: None,
    },
    Function {
        name: "is_platform_linux",
        description: "returns true if platform is linux",
        return_type: "bool",
        args: &[],
        example: None,
    },
    Function {
        name: "get_path_to_store",
        description: "returns the path to the spaces store (typically $HOME/.spaces/store)",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "get_absolute_path_to_workspace",
        description: "returns the absolute path to the workspace",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "get_path_to_checkout",
        description: "returns the path where the current script is located in the workspace",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "get_path_to_build_checkout",
        description: "returns the path to the workspace build folder for the current script",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "get_path_to_build_archive",
        description: "returns the path to where run.create_archive() creates the output archive",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "get_supported_platforms",
        description: "returns a list of the supported platforms",
        return_type: "list[str]",
        args: &[],
        example: None,
    },
    Function {
        name: "get_cpu_count",
        description: "returns the number of CPUs on the current machine",
        return_type: "int",
        args: &[],
        example: None,
    },
    Function {
        name: "is_ci",
        description: "returns true if `--ci` is passed on the command line",
        return_type: "int",
        args: &[],
        example: None,
    },
    Function {
        name: "set_minimum_version",
        description: "sets the minimum version of spaces required to run the script",
        return_type: "int",
        args: &[],
        example: None,
    },
];

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    // remove and replace with get_path_to_store()
    fn store_path() -> anyhow::Result<String> {
        Ok(workspace::get_store_path())
    }

    // remove and replace with get_absolute_path_to_workspace()
    fn absolute_workspace_path() -> anyhow::Result<String> {
        Ok(workspace::absolute_path())
    }

    // remove and replace with get_platform_name()
    fn platform_name() -> anyhow::Result<String> {
        platform::Platform::get_platform()
            .map(|p| p.to_string())
            .ok_or(anyhow::anyhow!("Failed to get platform name"))
    }

    // remove and replace with get_path_to_checkout()
    fn checkout_path() -> anyhow::Result<String> {
        rules::get_checkout_path()
    }

    // remove and replace with get_path_to_checkout()
    fn current_workspace_path() -> anyhow::Result<String> {
        rules::get_checkout_path()
    }

    fn get_platform_name() -> anyhow::Result<String> {
        platform::Platform::get_platform()
            .map(|p| p.to_string())
            .ok_or(anyhow::anyhow!("Failed to get platform name"))
    }

    fn get_supported_platforms() -> anyhow::Result<Vec<String>> {
        Ok(platform::Platform::get_supported_platforms().into_iter()
            .map(|p| p.to_string())
            .collect())
    }

    fn is_ci() -> anyhow::Result<bool> {
        Ok(get_state().read().unwrap().is_ci)
    }

    fn is_platform_windows() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_windows())
    }

    fn is_platform_macos() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_macos())
    }

    fn is_platform_linux() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_linux())
    }

    fn is_platform_x86_64() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_x86_64())
    }

    fn is_platform_aarch64() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_aarch64())
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

    fn get_cpu_count() -> anyhow::Result<i64> {
        Ok(num_cpus::get() as i64)
    }

    fn get_path_to_store() -> anyhow::Result<String> {
        Ok(workspace::get_store_path())
    }

    fn get_absolute_path_to_workspace() -> anyhow::Result<String> {
        Ok(workspace::absolute_path())
    }

    fn get_path_to_checkout() -> anyhow::Result<String> {
        rules::get_checkout_path()
    }

    fn get_path_to_build_checkout(
        #[starlark(require = named)] rule_name: &str,
    ) -> anyhow::Result<String> {
        rules::get_path_to_build_checkout(rule_name)
    }

    fn get_path_to_build_archive(
        #[starlark(require = named)] rule_name: &str,
        #[starlark(require = named)] archive: starlark::values::Value,
    ) -> anyhow::Result<String> {
        let create_archive: easy_archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let state = rules::get_state().read().unwrap();

        Ok(format!(
            "build/{}/{}",
            state.get_sanitized_rule_name(rule_name),
            create_archive.get_output_file()
        ))
    }

    fn set_minimum_version(version: &str) -> anyhow::Result<NoneType> {
        let current_version = env!("CARGO_PKG_VERSION");
        let version = version.parse::<semver::Version>().context(format_context!("bad version format"))?;
        if version > current_version.parse::<semver::Version>().unwrap() {
            return Err(anyhow::anyhow!(
                "Minimum required `spaces` version is {}. `spaces` version is {current_version}",
                version.to_string(),
            ));
        }
        Ok(NoneType)
    }

}
