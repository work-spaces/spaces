use crate::singleton;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::none::NoneType;
use starlark::values::Value;
use std::sync::Arc;
use utils::platform;

fn check_required_semver(required: &str) -> anyhow::Result<bool> {
    let current_version = env!("CARGO_PKG_VERSION");
    let required = required
        .parse::<semver::VersionReq>()
        .context(format_context!("Bad semver required"))?;
    let version = current_version
        .parse::<semver::Version>()
        .context(format_context!(
            "Internal Error: Failed to parse current version {current_version}"
        ))?;

    Ok(required.matches(&version))
}

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Returns the name of the current operating system and architecture platform.
    ///
    /// ```python
    /// platform = info.get_platform_name()
    /// if "linux" in platform:
    ///     print("Running on a Linux-based system")
    /// ```
    ///
    /// # Returns
    /// * `str`: The platform identifier, such as `macos-aarch64`, `macos-x86_64`, `linux-x86_64`, `linux-aarch64`, `windows-x86_64`, or `windows-aarch64`.
    fn get_platform_name() -> anyhow::Result<String> {
        platform::Platform::get_platform()
            .map(|p| p.to_string())
            .ok_or(anyhow::anyhow!("Failed to get platform name"))
    }

    /// Returns a list of all platforms supported by the system.
    ///
    /// ```python
    /// supported = info.get_supported_platforms()
    /// if "macos-aarch64" in supported:
    ///     print("This system supports Apple Silicon.")
    /// ```
    ///
    /// # Returns
    /// * `list[str]`: A list of supported platform identifiers, such as `macos-aarch64`, `linux-x86_64`, etc.
    fn get_supported_platforms() -> anyhow::Result<Vec<String>> {
        Ok(platform::Platform::get_supported_platforms()
            .into_iter()
            .map(|p| p.to_string())
            .collect())
    }

    /// Returns true if the `--ci` flag was passed on the command line.
    ///
    /// ```python
    /// if info.is_ci():
    ///     print("Running in CI mode.")
    /// ```
    ///
    /// # Returns
    /// * `bool`: True if the `--ci` flag is present, False otherwise.
    fn is_ci() -> anyhow::Result<bool> {
        Ok(singleton::get_is_ci())
    }

    /// Returns the current execution phase of the system.
    ///
    /// ```python
    /// phase = info.get_execution_phase()
    /// if phase == "Run":
    ///     print("System is in the execution phase.")
    /// ```
    ///
    /// # Returns
    /// * `str`: The current phase, which will be "Run", "Checkout", or "Inspect".
    fn get_execution_phase() -> anyhow::Result<String> {
        let phase = singleton::get_execution_phase();
        Ok(format!("{phase}"))
    }

    /// Returns true if the current platform is Windows.
    ///
    /// ```python
    /// if info.is_platform_windows():
    ///     print("Applying Windows-specific configurations...")
    /// ```
    ///
    /// # Returns
    /// * `bool`: True if the platform is `windows-x86_64` or `windows-aarch64`, False otherwise.
    fn is_platform_windows() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_windows())
    }

    /// Returns true if the current platform is macOS.
    ///
    /// ```python
    /// if info.is_platform_macos():
    ///     print("Setting up macOS-specific environment...")
    /// ```
    ///
    /// # Returns
    /// * `bool`: True if the platform is `macos-aarch64` or `macos-x86_64`, False otherwise.
    fn is_platform_macos() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_macos())
    }

    /// Returns true if the current platform is Linux.
    ///
    /// ```python
    /// if info.is_platform_linux():
    ///     print("Applying Linux-specific configurations...")
    /// ```
    ///
    /// # Returns
    /// * `bool`: True if the platform is `linux-x86_64` or `linux-aarch64`, False otherwise.
    fn is_platform_linux() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_linux())
    }

    /// Returns true if the current platform architecture is x86_64.
    ///
    /// ```python
    /// if info.is_platform_x86_64():
    ///     print("Running on x86_64 architecture")
    /// ```
    ///
    /// # Returns
    /// * `bool`: True if the platform architecture is `x86_64`, False otherwise.
    fn is_platform_x86_64() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_x86_64())
    }

    /// Returns true if the current platform architecture is aarch64.
    ///
    /// ```python
    /// if info.is_platform_aarch64():
    ///     print("Running on aarch64 architecture")
    /// ```
    ///
    /// # Returns
    /// * `bool`: True if the platform architecture is `aarch64`, False otherwise.
    fn is_platform_aarch64() -> anyhow::Result<bool> {
        Ok(platform::Platform::is_aarch64())
    }

    /// Abort script evaluation with a message.
    ///
    /// ```python
    /// info.abort("Failed to do something")
    /// ```
    ///
    /// # Arguments
    /// * `message`: Abort message to show the user.
    fn abort(message: &str) -> anyhow::Result<NoneType> {
        Err(format_error!("Info Aborting: {}", message))
    }

    /// Returns the number of CPUs on the current machine.
    ///
    /// ```python
    /// num_cpus = info.get_cpu_count()
    /// print(f"Available CPUs: {num_cpus}")
    /// ```
    ///
    /// # Returns
    /// * `int`: The total number of logical CPU cores available.
    fn get_cpu_count() -> anyhow::Result<i64> {
        Ok(num_cpus::get() as i64)
    }

    /// Parses a log file into its YAML header and message lines.
    ///
    /// ```python
    /// log_data = info.parse_log_file("outputs/build.log")
    /// print(f"Target: {log_data['header']['target']}")
    /// for line in log_data['lines']:
    ///     print(line)
    /// ```
    ///
    /// # Arguments
    /// * `path`: The absolute or relative path to the spaces log file.
    ///
    /// # Returns
    /// * `dict`: A dictionary containing `header` (`dict`) with the parsed YAML metadata and `lines` (`list[str]`) with the body of the log.
    fn parse_log_file<'v>(
        path: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let heap = eval.heap();

        #[derive(serde::Serialize, serde::Deserialize)]
        struct Log {
            header: printer::LogHeader,
            lines: Vec<Arc<str>>,
        }

        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let mut header = String::new();
        let mut lines = Vec::new();
        let log_divider = printer::Printer::get_log_divider();
        let mut collect_header = true;
        for line in content.lines() {
            if line == log_divider.as_ref() {
                collect_header = false;
                continue;
            }
            if collect_header {
                header.push_str(line);
                header.push('\n');
            } else {
                lines.push(line.to_string().into());
            }
        }

        let log_header: printer::LogHeader = serde_yaml::from_str(&header).context(
            format_context!("Failed to parse (yaml) Log Header file {}", path),
        )?;

        let json_value = serde_json::to_value(&Log {
            header: log_header,
            lines,
        })
        .context(format_context!(
            "Internal Error: Failed to convert Log to JSON {}",
            path
        ))?;

        // Convert the JSON value to a Starlark value
        let alloc_value = heap.alloc(json_value);
        Ok(alloc_value)
    }

    /// Returns the path to the spaces store.
    ///
    /// ```python
    /// store_path = info.get_path_to_store()
    /// print(f"Store location: {store_path}")
    /// ```
    ///
    /// # Returns
    /// * `str`: The absolute path to the local spaces store directory.
    fn get_path_to_store() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.get_store_path().to_string())
    }

    /// Returns the path to the spaces tools directory.
    ///
    /// ```python
    /// tools_path = info.get_path_to_spaces_tools()
    /// print(f"Tools location: {tools_path}")
    /// ```
    ///
    /// # Returns
    /// * `str`: The absolute path to the spaces tools directory.
    fn get_path_to_spaces_tools() -> anyhow::Result<String> {
        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();
        Ok(workspace.get_spaces_tools_path().to_string())
    }

    /// Returns a string representing the end of the log header.
    ///
    /// ```python
    /// divider = info.get_log_divider_string()
    /// print(f"Header Finished\n{divider}")
    /// ```
    ///
    /// # Returns
    /// * `str`: The standard divider string used to separate log headers from the body.
    fn get_log_divider_string() -> anyhow::Result<String> {
        Ok(printer::Printer::get_log_divider().to_string())
    }

    /// Sets the minimum version of spaces required to run the script.
    ///
    /// ```python
    /// info.set_minimum_version("1.2.0")
    /// ```
    ///
    /// # Arguments
    /// * `version`: The minimum version string (e.g., "1.5.0") required.
    fn set_minimum_version(version: &str) -> anyhow::Result<NoneType> {
        let current_version = env!("CARGO_PKG_VERSION");
        let version = version
            .parse::<semver::Version>()
            .context(format_context!("bad version format"))?;
        if version
            > current_version
                .parse::<semver::Version>()
                .context(format_context!(
                    "Internal Error: Failed to parse current version {current_version}"
                ))?
        {
            return Err(anyhow::anyhow!(
                "Minimum required `spaces` version is `{version}`. `spaces` version is `{current_version}`"
            ));
        }
        Ok(NoneType)
    }

    /// Sets the semantic version of `spaces` required to run the workspace.
    ///
    /// ```python
    /// info.set_required_semver("^2.1.0")
    /// ```
    ///
    /// # Arguments
    /// * `required`: The semantic version requirement string (e.g., "^2.1.0").
    fn set_required_semver(required: &str) -> anyhow::Result<NoneType> {
        let is_required_version = check_required_semver(required)?;
        if !is_required_version {
            let current_version = env!("CARGO_PKG_VERSION");
            return Err(anyhow::anyhow!(
                "Workflow/workspaces requires `spaces` semver `{required}`. `spaces` version is `{current_version}`",
            ));
        }
        Ok(NoneType)
    }

    /// Checks if the current version of `spaces` satisfies the given semver requirement.
    ///
    /// ```python
    /// is_compatible = info.check_required_semver("^2.1.0")
    /// if not is_compatible:
    ///     print("Incompatible version")
    /// ```
    ///
    /// # Arguments
    /// * `required`: The semantic version requirement string to check against.
    ///
    /// # Returns
    /// * `bool`: True if the current version satisfies the requirement, False otherwise.
    fn check_required_semver(required: &str) -> anyhow::Result<bool> {
        check_required_semver(required)
    }

    /// Sets the maximum number of items that can be queued at one time.
    ///
    /// ```python
    /// info.set_max_queue_count(10)
    /// ```
    ///
    /// # Arguments
    /// * `count`: The maximum number of items to allow in the queue.
    fn set_max_queue_count(count: i64) -> anyhow::Result<NoneType> {
        if count < 1 {
            return Err(anyhow::anyhow!("max_queue_count must be greater than 0"));
        }
        if count > 64 {
            return Err(anyhow::anyhow!("max_queue_count must be less than 65"));
        }
        singleton::set_max_queue_count(count);
        Ok(NoneType)
    }
}
