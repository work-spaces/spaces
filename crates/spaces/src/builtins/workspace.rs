use crate::{rules, singleton};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use starlark::values::{Heap, Value};
use starstd::{Arg, Function};
use std::collections::HashMap;
use utils::{environment, ws};

pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "get_path_to_log_file",
        description: "returns the relative workspace path to the log file for the target",
        return_type: "str",
        args: &[
            Arg {
                name: "name",
                description: "The name of the rule to get the log file",
                dict: &[],
            },
        ],
        example: None,
    },
    Function {
        name: "get_path_to_member",
        description: "returns a string to the workspace member matching the specified requirement (error if not found)",
        return_type: "str",
        args: &[
            Arg {
                name: "member",
                description: "The requirements for the member",
                dict: &[
                    ("url:str", "The url of the member"),
                    ("required:dict", "{'Revision': <git/sha256 hash>}|{'SemVer': <semver requirement>}"),
                ],
            },
        ],
        example: None,
    },
    Function {
        name: "is_path_to_member_available",
        description: "returns true if the workspace satisfies the requirments",
        return_type: "bool",
        args: &[
            Arg {
                name: "member",
                description: "The requirements for the member",
                dict: &[
                    ("url:str", "The url of the member"),
                    ("required:dict", "{'Revision': <git/sha256 hash>}|{'SemVer': <semver requirement>}"),
                ],
            },
        ],
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
        description: "returns the value of the workspace environment variable",
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
        name: "get_digest",
        description: "returns the digest of the workspace. This is only meaningful if the workspace is reproducible (which can't be known until after checkout)",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "get_path_to_shell_config",
        description: "returns the path to the shell config file",
        return_type: "str",
        args: &[],
        example: None,
    },
    Function {
        name: "is_env_var_set",
        description: "returns true if the workspace environment variable is set",
        return_type: "bool",
        args: &[
            Arg {
                name: "var_name",
                description: "The name of the environment variable to check",
                dict: &[],
            },
        ],
        example: None,
    },
];

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn is_reproducible() -> anyhow::Result<bool> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.is_reproducible())
    }

    fn get_path_to_shell_config() -> anyhow::Result<String> {
        Ok(crate::workspace::SHELL_TOML_NAME.to_string())
    }

    fn get_digest() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.digest.clone().to_string())
    }

    fn get_short_digest() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.get_short_digest().to_string())
    }

    fn is_env_var_set(var_name: &str) -> anyhow::Result<bool> {
        if var_name == "PATH" {
            return Ok(true);
        }
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        let env = workspace.get_env();
        Ok(env
            .vars
            .as_ref()
            .is_some_and(|vars| vars.contains_key(var_name)))
    }

    fn get_env_var(var_name: &str) -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();

        let env = workspace.get_env();
        if var_name == "PATH" {
            return Ok(env.get_path().to_string());
        }

        if let Some(value) = env.vars.as_ref().and_then(|e| e.get(var_name)) {
            return Ok(value.clone().to_string());
        }

        Err(format_error!(
            "{var_name} is not set in the workspace environment"
        ))
    }

    fn set_env(
        #[starlark(require = named)] env: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;

        let mut workspace = workspace_arc.write();
        let mut env: environment::Environment = serde_json::from_value(env.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        // extended with command line args
        let env_args = singleton::get_args_env();
        env.vars.get_or_insert_default().extend(env_args);

        // This checks for workspaces created with previous versions
        // It brings in PATH and inherited variables to match
        // the previous behavior
        if !env.vars.as_ref().is_some_and(|e| e.contains_key("PATH")) {
            let vars = env
                .get_checkout_vars()
                .context(format_context!("Failed to get environment variables"))?;
            env.vars.get_or_insert_default().extend(vars);
        }

        workspace.set_env(env);

        Ok(NoneType)
    }

    fn set_locks(
        #[starlark(require = named)] locks: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let locks = serde_json::from_value(locks.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        let workspace_arc = singleton::get_workspace()
            .context(format_error!("Internal Error: No active workspace found"))?;
        let mut workspace = workspace_arc.write();

        workspace.locks = locks;

        Ok(NoneType)
    }

    fn set_always_evaluate(always_evaluate: bool) -> anyhow::Result<NoneType> {
        let workspace_arc = singleton::get_workspace()
            .context(format_error!("Internal Error: No active workspace found"))?;
        let mut workspace = workspace_arc.write();
        workspace.settings.bin.is_always_evaluate = always_evaluate;
        Ok(NoneType)
    }

    fn get_path_to_member(
        #[starlark(require = named)] member: starlark::values::Value,
    ) -> anyhow::Result<String> {
        let workspace_arc = singleton::get_workspace()
            .context(format_error!("Internal Error: No active workspace found"))?;
        let member_requirement_json = member.to_json_value()?;
        let member_requirement: ws::MemberRequirement =
            serde_json::from_value(member_requirement_json.clone())
                .context(format_context!("bad options for workspace member"))?;

        let path = workspace_arc
            .read()
            .settings
            .json
            .get_path_to_member(&member_requirement);
        match path {
            Some(p) => Ok(p.to_string()),
            None => Err(format_error!(
                "`{}` not found in workspace matching {:?}",
                member_requirement.url,
                member_requirement.required
            )),
        }
    }

    fn is_path_to_member_available(
        #[starlark(require = named)] member: starlark::values::Value,
    ) -> anyhow::Result<bool> {
        let workspace_arc = singleton::get_workspace()
            .context(format_error!("Internal Error: No active workspace found"))?;
        let member_requirement_json = member.to_json_value()?;
        let member_requirement: ws::MemberRequirement =
            serde_json::from_value(member_requirement_json.clone())
                .context(format_context!("bad options for workspace member"))?;

        let path = workspace_arc
            .read()
            .settings
            .json
            .get_path_to_member(&member_requirement);
        match path {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    fn get_path_to_log_file(rule: &str) -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        {
            let mut workspace = workspace_arc.write();
            workspace.settings.bin.is_always_evaluate = true;
        }
        let workspace = workspace_arc.read();
        let rule_name = rules::get_sanitized_rule_name(rule.into());
        Ok(workspace.get_log_file(rule_name.as_ref()).to_string())
    }

    fn get_absolute_path() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.absolute_path.clone().to_string())
    }

    fn get_path_to_checkout() -> anyhow::Result<String> {
        rules::get_checkout_path().map(|path| path.to_string())
    }

    fn get_path_to_build_checkout(
        #[starlark(require = named)] rule_name: &str,
    ) -> anyhow::Result<String> {
        rules::get_path_to_build_checkout(rule_name.into()).map(|p| p.to_string())
    }

    fn get_path_to_build_archive(
        #[starlark(require = named)] rule_name: &str,
        #[starlark(require = named)] archive: starlark::values::Value,
    ) -> anyhow::Result<String> {
        let create_archive: archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let sanitized_rule_name = rules::get_sanitized_rule_name(rule_name.into());

        Ok(format!(
            "build/{sanitized_rule_name}/{}",
            create_archive.get_output_file()
        ))
    }

    fn get_build_archive_info<'v>(
        #[starlark(require = named)] rule_name: &str,
        #[starlark(require = named)] archive: starlark::values::Value,
        heap: &'v Heap,
    ) -> anyhow::Result<Value<'v>> {
        let create_archive: archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let create_archive_output = create_archive.get_output_file();
        let output_path = std::path::Path::new(create_archive_output.as_str());
        let output_sha_suffix = output_path.with_extension("").with_extension("sha256.txt");

        let sanitized_rule_name = rules::get_sanitized_rule_name(rule_name.into());

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

        Ok(alloc_value)
    }
}
