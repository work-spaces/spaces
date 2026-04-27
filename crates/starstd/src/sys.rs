use crate::is_lsp_mode;
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use std::path::PathBuf;

// This defines the functions that are visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Returns the current operating system name.
    ///
    /// ```python
    /// sys.os()  # "linux" | "macos" | "windows"
    /// ```
    fn os() -> anyhow::Result<&'static str> {
        Ok(std::env::consts::OS)
    }

    /// Returns the current CPU architecture.
    ///
    /// ```python
    /// sys.arch()  # "x86_64" | "aarch64" | ...
    /// ```
    fn arch() -> anyhow::Result<&'static str> {
        Ok(std::env::consts::ARCH)
    }

    /// Returns the hostname for the current machine, if available.
    ///
    /// ```python
    /// sys.hostname()
    /// ```
    fn hostname() -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let host = hostname::get()
            .context(format_context!("Failed to get hostname"))?
            .to_string_lossy()
            .into_owned();
        Ok(host)
    }

    /// Returns the current username, if available.
    ///
    /// ```python
    /// sys.username()
    /// ```
    fn username() -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let user = whoami::username();
        Ok(user)
    }

    /// Returns the current user's home directory path.
    ///
    /// ```python
    /// sys.user_home()
    /// ```
    fn user_home() -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let home: PathBuf =
            dirs::home_dir().context(format_context!("Failed to resolve user home directory"))?;
        Ok(home.to_string_lossy().into_owned())
    }

    /// Returns the logical CPU count.
    ///
    /// ```python
    /// sys.cpu_count()
    /// ```
    fn cpu_count() -> anyhow::Result<u64> {
        Ok(num_cpus::get() as u64)
    }

    /// Returns total system memory in bytes.
    ///
    /// ```python
    /// sys.total_memory_bytes()
    /// ```
    fn total_memory_bytes() -> anyhow::Result<u64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        Ok(system.total_memory())
    }

    /// Returns host byte order.
    ///
    /// ```python
    /// sys.endianness()  # "little" | "big"
    /// ```
    fn endianness() -> anyhow::Result<&'static str> {
        let bytes = 1u16.to_ne_bytes();
        if bytes[0] == 1 {
            Ok("little")
        } else {
            Ok("big")
        }
    }

    /// Returns the current executable path.
    ///
    /// ```python
    /// sys.executable()
    /// ```
    fn executable() -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let path = std::env::current_exe()
            .context(format_context!("Failed to determine executable path"))?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Returns true when running in common CI environments.
    ///
    /// ```python
    /// sys.is_ci()
    /// ```
    fn is_ci() -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }
        const CI_MARKERS: &[&str] = &[
            "CI",
            "GITHUB_ACTIONS",
            "GITLAB_CI",
            "BUILDKITE",
            "CIRCLECI",
            "TRAVIS",
            "JENKINS_URL",
            "TEAMCITY_VERSION",
            "TF_BUILD",
            "BITBUCKET_BUILD_NUMBER",
        ];

        Ok(CI_MARKERS
            .iter()
            .any(|name| std::env::var_os(name).is_some()))
    }

    /// Exits the program with the specified exit code.
    ///
    /// ```python
    /// sys.exit(0)  # Exit successfully
    /// sys.exit(1)  # Exit with error code
    /// ```
    fn exit(code: i32) -> anyhow::Result<i32> {
        if is_lsp_mode() {
            return Ok(code);
        }
        std::process::exit(code)
    }
}
