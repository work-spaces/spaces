use crate::{executor, rules, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use starlark::values::{Heap, Value};
use starstd::{Arg, Function};
use std::collections::HashMap;
use std::sync::RwLock;

struct State {
    #[allow(dead_code)]
    new_branch_name: Option<String>,
    env: executor::env::UpdateEnv,
    is_ci: bool,
    max_queue_count: i64,
    phase: rules::Phase,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    let mut env = executor::env::UpdateEnv {
        vars: HashMap::new(),
        paths: Vec::new(),
        system_paths: None,
    };

    env.vars.insert(
        workspace::SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE.to_owned(),
        "true".to_string(),
    );

    STATE.set(RwLock::new(State {
        new_branch_name: None,
        env,
        is_ci: false,
        max_queue_count: 8,
        phase: rules::Phase::Cancelled,
    }));
    STATE.get()
}

pub fn set_phase(phase: rules::Phase) {
    let mut state = get_state().write().unwrap();
    state.phase = phase;
}

fn get_phase() -> rules::Phase {
    let state = get_state().read().unwrap();
    state.phase
}

pub fn set_is_reproducible(value: bool) {
    let mut state = get_state().write().unwrap();
    state.env.vars.insert(
        workspace::SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE.to_owned(),
        value.to_string(),
    );
}

pub fn is_reproducible() -> bool {
    let state = get_state().read().unwrap();
    if let Some(value) = state
        .env
        .vars
        .get(workspace::SPACES_ENV_IS_WORKSPACE_REPRODUCIBLE)
    {
        return value == "true";
    }
    false
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
    if let Some(system_paths) = env.system_paths {
        if let Some(existing_system_paths) = state.env.system_paths.as_mut() {
            existing_system_paths.extend(system_paths.clone());
        } else {
            state.env.system_paths = Some(system_paths);
        }
    }
    Ok(())
}

pub fn get_env() -> executor::env::UpdateEnv {
    let state = get_state().read().unwrap();
    state.env.clone()
}

pub fn get_max_queue_count() -> i64 {
    let state = get_state().read().unwrap();
    state.max_queue_count
}

fn set_max_queue_count(count: i64) {
    let mut state = get_state().write().unwrap();
    state.max_queue_count = count;
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
        name: "get_env_var",
        description: "returns the path where the current script is located in the workspace",
        return_type: "str",
        args: &[
            Arg {
                name: "var",
                description: "The name of the environment variable",
                dict: &[],
            },
        ],
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
        args: &[
            Arg {
                name: "rule_name",
                description: "The name of the rule used to create the archive",
                dict: &[],
            },
            Arg {
                name: "archive",
                description: "The archive info used to create the archive",
                dict: &[],
            },
        ],
        example: None,
    },
    Function {
        name: "get_build_archive_info",
        description: "returns the path to where run.create_archive() creates the sha256 txt file",
        return_type: "dict['archive_path': str, 'sha256_path': str]",
        args: &[
            Arg {
                name: "rule_name",
                description: "The name of the rule used to create the archive",
                dict: &[],
            },
            Arg {
                name: "archive",
                description: "The archive info used to create the archive",
                dict: &[],
            },
        ],
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
        name: "get_workspace_digest",
        description: "returns the digest of the workspace. This is only meaningful if the workspace is reproducible (which can't be known until after checkout)",
        return_type: "str",
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
        args: &[
            Arg {
                name: "version",
                description: "the minimum version of spaces required to run the script",
                dict: &[],
            },
        ],
        example: None,
    },
    Function {
        name: "set_max_queue_count",
        description: "sets the maxiumum number of items to queue at one time",
        return_type: "int",
        args: &[
            Arg {
                name: "count",
                description: "the maximum number of items to queue at one time",
                dict: &[],
            },
        ],
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
        Ok(platform::Platform::get_supported_platforms()
            .into_iter()
            .map(|p| p.to_string())
            .collect())
    }

    fn is_workspace_reproducible() -> anyhow::Result<bool> {
        Ok(is_reproducible())
    }

    fn get_workspace_digest() -> anyhow::Result<String> {
        Ok(workspace::get_digest())
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

    fn is_env_var_set(var_name: &str) -> anyhow::Result<bool> {
        let state = get_state().read().unwrap();
        if var_name == "PATH" {
            return Ok(true);
        }

        Ok(state.env.vars.contains_key(var_name))
    }

    fn get_env_var(var_name: &str) -> anyhow::Result<String> {
        let state = get_state().read().unwrap();
        if var_name == "PATH" {
            return Ok(state.env.get_path());
        }

        if let Some(value) = state.env.vars.get(var_name) {
            return Ok(value.clone());
        }

        Err(format_error!(
            "{var_name} is not set in the workspace environment"
        ))
    }

    fn get_capsule_info<'v>(
        dependency: starlark::values::Value,
        heap: &'v Heap,
    ) -> anyhow::Result<Option<Value<'v>>> {
        let phase = get_phase();
        if phase == rules::Phase::Run {
            let capsule_depedency: executor::capsule::Dependency =
                serde_json::from_value(dependency.to_json_value()?)
                    .context(format_context!("Failed to parse dependency arguments"))?;

            let resolved_dependency = capsule_depedency
                .resolve()
                .context(format_context!("Failed to resolve dependency"))?;

            let json_value = serde_json::to_value(resolved_dependency)
                .context(format_context!("Failed to convert Result to JSON"))?;

            // Convert the JSON value to a Starlark value
            let alloc_value = heap.alloc(json_value);

            Ok(Some(alloc_value))
        } else {
            Ok(None)
        }
    }

    fn set_env(
        #[starlark(require = named)] env: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let mut state = get_state().write().unwrap();

        state.env = serde_json::from_value(env.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        Ok(NoneType)
    }

    fn set_locks(
        #[starlark(require = named)] locks: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let locks = serde_json::from_value(locks.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        workspace::set_locks(locks);

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

        let sanitized_rule_name = rules::get_sanitized_rule_name(rule_name);

        Ok(format!(
            "build/{sanitized_rule_name}/{}",
            create_archive.get_output_file()
        ))
    }

    fn get_path_to_spaces_tools() -> anyhow::Result<String> {
        Ok(workspace::get_spaces_tools_path())
    }

    fn get_build_archive_info<'v>(
        #[starlark(require = named)] rule_name: &str,
        #[starlark(require = named)] archive: starlark::values::Value,
        heap: &'v Heap,
    ) -> anyhow::Result<Option<Value<'v>>> {
        let create_archive: easy_archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let create_archive_output = create_archive.get_output_file();
        let output_path = std::path::Path::new(create_archive_output.as_str());
        let output_sha_suffix = output_path.with_extension("").with_extension("sha256.txt");

        let sanitized_rule_name = rules::get_sanitized_rule_name(rule_name);

        let mut output = HashMap::new();
        let rule_output_path = format!("build/{sanitized_rule_name}");

        output.insert(
            "archive_path".to_string(),
            format!("{rule_output_path}/{create_archive_output}",),
        );
        output.insert(
            "sha256_path".to_string(),
            format!("{rule_output_path}/{}", output_sha_suffix.to_string_lossy()),
        );

        let json_value = serde_json::to_value(&output)
            .context(format_context!("Failed to convert Result to JSON"))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);

        Ok(Some(alloc_value))
    }

    fn set_minimum_version(version: &str) -> anyhow::Result<NoneType> {
        let current_version = env!("CARGO_PKG_VERSION");
        let version = version
            .parse::<semver::Version>()
            .context(format_context!("bad version format"))?;
        if version > current_version.parse::<semver::Version>().unwrap() {
            return Err(anyhow::anyhow!(
                "Minimum required `spaces` version is {}. `spaces` version is {current_version}",
                version.to_string(),
            ));
        }
        Ok(NoneType)
    }

    fn set_max_queue_count(count: i64) -> anyhow::Result<NoneType> {
        if count < 1 {
            return Err(anyhow::anyhow!("max_queue_count must be greater than 0"));
        }
        if count > 64 {
            return Err(anyhow::anyhow!("max_queue_count must be less than 65"));
        }
        set_max_queue_count(count);
        Ok(NoneType)
    }
}
