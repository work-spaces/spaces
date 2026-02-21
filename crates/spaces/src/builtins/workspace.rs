use crate::{rules, singleton};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::none::NoneType;
use starlark::values::Value;
use std::collections::HashMap;
use utils::{environment, ws};

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Returns true if the workspace is reproducible.
    ///
    /// ```python
    /// if workspace.is_reproducible():
    ///     print("Workspace is reproducible")
    /// ```
    ///
    /// # Returns
    /// * `bool`: True if the workspace is reproducible, False otherwise.
    fn is_reproducible() -> anyhow::Result<bool> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.is_reproducible())
    }

    /// Returns the path to the shell config file.
    ///
    /// ```python
    /// shell_config = workspace.get_path_to_shell_config()
    /// ```
    ///
    /// # Returns
    /// * `str`: The path to the shell configuration file used by the workspace.
    fn get_path_to_shell_config() -> anyhow::Result<String> {
        Ok(crate::workspace::SHELL_TOML_NAME.to_string())
    }

    /// Returns the digest of the workspace.
    ///
    /// This is only meaningful if the workspace is reproducible, which is typically
    /// determined after the checkout process is complete.
    ///
    /// ```python
    /// digest = workspace.get_digest()
    /// ```
    ///
    /// # Returns
    /// * `str`: The unique digest string of the workspace.
    fn get_digest() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.digest.clone().to_string())
    }

    /// Returns the short digest of the workspace.
    ///
    /// ```python
    /// short_digest = workspace.get_short_digest()
    /// ```
    ///
    /// # Returns
    /// * `str`: The short digest string of the workspace.
    fn get_short_digest() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.get_short_digest().to_string())
    }

    /// Returns true if the workspace environment variable is set.
    ///
    /// ```python
    /// if workspace.is_env_var_set("DEBUG_MODE"):
    ///     print("Debug mode is enabled")
    /// ```
    ///
    /// # Arguments
    /// * `var_name`: The name of the environment variable to check.
    ///
    /// # Returns
    /// * `bool`: True if the variable exists in the workspace environment, False otherwise.
    fn is_env_var_set(var_name: &str) -> anyhow::Result<bool> {
        if var_name == "PATH" {
            return Ok(true);
        }
        let workspace_arc = singleton::get_workspace()
            .context(format_error!("Internal error: no active workspace found"))?;
        let workspace = workspace_arc.read();
        let env = workspace.get_env();
        Ok(env.is_env_var_set(var_name))
    }

    /// Returns the value of a workspace environment variable.
    ///
    /// ```python
    /// home_dir = workspace.get_env_var("HOME")
    /// ```
    ///
    /// # Arguments
    /// * `var_name`: The name of the environment variable.
    ///
    /// # Returns
    /// * `str`: The value of the environment variable.
    fn get_env_var(var_name: &str) -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();

        let env_result = workspace
            .get_env()
            .get_run_environment()
            .context(format_context!("Failed to get ENV"));

        match env_result {
            Ok(env) => {
                if let Some(value) = env.vars.get(var_name) {
                    Ok(value.clone().to_string())
                } else if singleton::is_lsp_mode() {
                    Ok("<not available to LSP>".to_string())
                } else {
                    Err(format_error!(
                        "{var_name} is not set in the workspace environment"
                    ))
                }
            }
            Err(e) => {
                if singleton::is_lsp_mode() {
                    Ok("<not available to LSP>".to_string())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Sets the workspace environment.
    ///
    /// This is meant for internal use only from the `env.spaces.star` module.
    ///
    /// ```python
    /// workspace.set_env(
    ///     env = {
    ///         "vars": {"CC": "clang", "CXX": "clang++"},
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `env`: Environment definition containing `vars` (`dict`), `paths` (`list`), and `inherited` (`list`).
    fn set_env(
        #[starlark(require = named)] env: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;

        let any_env = environment::AnyEnvironment::try_from(env.to_json_value()?)
            .context(format_context!("Failed to parse set_env arguments"))?;

        let mut workspace = workspace_arc.write();
        workspace
            .update_env(any_env)
            .context(format_context!("Failed to update workspace env"))?;

        Ok(NoneType)
    }

    /// Sets the workspace locks.
    ///
    /// This is meant for internal use only from a lock module.
    ///
    /// ```python
    /// workspace.set_locks(
    ///     locks = {"my_lock": "lock_value"},
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `locks`: A dictionary of lock names to lock values.
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

    /// Sets whether the workspace should always evaluate scripts.
    ///
    /// ```python
    /// workspace.set_always_evaluate(True)
    /// ```
    ///
    /// # Arguments
    /// * `always_evaluate`: If True, scripts will always be evaluated regardless of caching.
    fn set_always_evaluate(always_evaluate: bool) -> anyhow::Result<NoneType> {
        let workspace_arc = singleton::get_workspace()
            .context(format_error!("Internal Error: No active workspace found"))?;
        let mut workspace = workspace_arc.write();
        workspace.settings.bin.is_always_evaluate = always_evaluate;
        Ok(NoneType)
    }

    /// Returns the path to the workspace member matching the specified requirement.
    ///
    /// If the member cannot be found, an error is raised.
    ///
    /// ```python
    /// member_req = {
    ///     "url": "https://github.com/example/repo.git",
    ///     "required": {"SemVer": "^1.2.0"}
    /// }
    /// path = workspace.get_path_to_member(member = member_req)
    /// ```
    ///
    /// # Arguments
    /// * `member`: The requirements for the member, containing `url` (`str`) and `required` (`dict`).
    ///
    /// # Returns
    /// * `str`: The workspace path to the matching member.
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

    /// Returns true if the workspace satisfies the specified member requirements.
    ///
    /// ```python
    /// member_req = {
    ///     "url": "https://github.com/example/repo.git",
    ///     "required": {"Revision": "a1b2c3d4e5f6"}
    /// }
    /// if workspace.is_path_to_member_available(member = member_req):
    ///     print("Member is available in the workspace.")
    /// ```
    ///
    /// # Arguments
    /// * `member`: The requirements for the member, containing `url` (`str`) and `required` (`dict`).
    ///
    /// # Returns
    /// * `bool`: True if the workspace contains a member matching the requirements, False otherwise.
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

    /// Returns the relative workspace path to the log file for the specified target.
    ///
    /// ```python
    /// log_file = workspace.get_path_to_log_file("build_service")
    /// ```
    ///
    /// # Arguments
    /// * `rule`: The name of the target rule.
    ///
    /// # Returns
    /// * `str`: The relative path to the log file within the workspace.
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

    /// Returns the absolute path to the workspace.
    ///
    /// ```python
    /// workspace_path = workspace.get_absolute_path()
    /// ```
    ///
    /// # Returns
    /// * `str`: The absolute path to the workspace.
    fn get_absolute_path() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.absolute_path.clone().to_string())
    }

    /// Returns the repository path in the workspace of the calling script.
    ///
    /// ```python
    /// script_location = workspace.get_path_to_checkout()
    /// ```
    ///
    /// # Returns
    /// * `str`: The path to the directory containing the current script.
    fn get_path_to_checkout() -> anyhow::Result<String> {
        rules::get_checkout_path().map(|path| path.to_string())
    }

    /// Returns the path to the workspace build folder for the current script.
    ///
    /// ```python
    /// build_path = workspace.get_path_to_build_checkout(rule_name = "my_rule")
    /// ```
    ///
    /// # Arguments
    /// * `rule_name`: The name of the rule to get the build checkout path for.
    ///
    /// # Returns
    /// * `str`: The path to the build directory associated with the current script evaluation.
    fn get_path_to_build_checkout(
        #[starlark(require = named)] rule_name: &str,
    ) -> anyhow::Result<String> {
        rules::get_path_to_build_checkout(rule_name.into()).map(|p| p.to_string())
    }

    /// Returns the path to where `run.add_archive()` creates the output archive.
    ///
    /// ```python
    /// archive_info = {
    ///     "input": "dist",
    ///     "name": "release_pkg",
    ///     "version": "2.1.0",
    ///     "driver": "zip",
    /// }
    /// path = workspace.get_path_to_build_archive(rule_name = "package_rule", archive = archive_info)
    /// ```
    ///
    /// # Arguments
    /// * `rule_name`: The name of the rule used to create the archive.
    /// * `archive`: The archive info used to create the archive.
    ///
    /// # Returns
    /// * `str`: The path to the generated output archive.
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

    /// Returns the archive and sha256 file paths for a build archive.
    ///
    /// ```python
    /// archive_info = {
    ///     "input": "build/install",
    ///     "name": "my_archive",
    ///     "version": "1.0",
    ///     "driver": "tar.gz",
    /// }
    /// info = workspace.get_build_archive_info(rule_name = "my_rule", archive = archive_info)
    /// ```
    ///
    /// # Arguments
    /// * `rule_name`: The name of the rule used to create the archive.
    /// * `archive`: The archive info used to create the archive.
    ///
    /// # Returns
    /// * `dict`: A dictionary containing `archive_path` (`str`) and `sha256_path` (`str`).
    fn get_build_archive_info<'v>(
        #[starlark(require = named)] rule_name: &str,
        #[starlark(require = named)] archive: starlark::values::Value,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();
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
