use crate::is_lsp_mode;
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use std::process::Command;

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Runs a command string using the platform shell.
    ///
    /// Unix: `/bin/sh -c <command>`
    /// Windows: `cmd.exe /C <command>`
    ///
    /// Returns a dict with:
    /// - `status` (int)
    /// - `stdout` (str)
    /// - `stderr` (str)
    ///
    /// If `check=True` and status is non-zero, returns an error.
    ///
    /// ```python
    /// result = sh.run("cat *.log | grep ERROR | wc -l", check=True)
    /// print(result["stdout"])
    /// ```
    fn run(
        command: &str,
        #[starlark(require = named, default = false)] check: bool,
        #[starlark(require = named)] cwd: Option<String>,
    ) -> anyhow::Result<serde_json::Value> {
        if is_lsp_mode() {
            return Ok(serde_json::json!({"status": 0, "stdout": "", "stderr": ""}));
        }
        let output = run_shell(command, cwd.as_deref())
            .context(format_context!("Failed to execute shell command"))?;

        let status = output.status.code().unwrap_or(1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if check && status != 0 {
            return Err(anyhow::anyhow!(
                "Shell command failed with status {status}: {command}\nstderr:\n{stderr}"
            ))
            .context(format_context!(
                "Shell command returned non-zero exit status"
            ));
        }

        Ok(serde_json::json!({
            "status": status,
            "stdout": stdout,
            "stderr": stderr,
        }))
    }

    /// Runs a command and returns stdout as a single string.
    ///
    /// Trailing newlines are trimmed.
    ///
    /// If `check=True` and status is non-zero, returns an error.
    ///
    /// ```python
    /// head = sh.capture("git rev-parse HEAD")
    /// ```
    fn capture(
        command: &str,
        #[starlark(require = named, default = true)] check: bool,
        #[starlark(require = named)] cwd: Option<String>,
    ) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let output = run_shell(command, cwd.as_deref())
            .context(format_context!("Failed to execute shell command"))?;

        let status = output.status.code().unwrap_or(1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if check && status != 0 {
            return Err(anyhow::anyhow!(
                "Shell command failed with status {status}: {command}\nstderr:\n{stderr}"
            ))
            .context(format_context!(
                "Shell command returned non-zero exit status"
            ));
        }

        Ok(stdout.trim_end_matches(['\n', '\r']).to_string())
    }

    /// Runs a command and returns stdout split into lines.
    ///
    /// Empty trailing line is not returned.
    ///
    /// If `check=True` and status is non-zero, returns an error.
    ///
    /// ```python
    /// files = sh.lines("ls -1")
    /// ```
    fn lines(
        command: &str,
        #[starlark(require = named, default = true)] check: bool,
        #[starlark(require = named)] cwd: Option<String>,
    ) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }
        let output = run_shell(command, cwd.as_deref())
            .context(format_context!("Failed to execute shell command"))?;

        let status = output.status.code().unwrap_or(1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if check && status != 0 {
            return Err(anyhow::anyhow!(
                "Shell command failed with status {status}: {command}\nstderr:\n{stderr}"
            ))
            .context(format_context!(
                "Shell command returned non-zero exit status"
            ));
        }

        let text = stdout.trim_end_matches(['\n', '\r']);
        if text.is_empty() {
            return Ok(Vec::new());
        }
        Ok(text.lines().map(ToOwned::to_owned).collect())
    }

    /// Runs a command and returns only its numeric exit code.
    ///
    /// Never errors for non-zero command status; only errors if process spawn/wait fails.
    ///
    /// ```python
    /// code = sh.exit_code("test -f foo")
    /// ```
    fn exit_code(
        command: &str,
        #[starlark(require = named)] cwd: Option<String>,
    ) -> anyhow::Result<i32> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let output = run_shell(command, cwd.as_deref())
            .context(format_context!("Failed to execute shell command"))?;
        Ok(output.status.code().unwrap_or(1))
    }
}

fn run_shell(command: &str, cwd: Option<&str>) -> anyhow::Result<std::process::Output> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd.exe");
        c.arg("/C").arg(command);
        c
    };

    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(command);
        c
    };

    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    cmd.output()
        .context(format_context!("Failed to run shell command: {command}"))
}
