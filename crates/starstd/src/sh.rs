//! Shell command execution module.
//!
//! Every function in this module passes the command string verbatim to the
//! **platform shell**:
//!
//! - Unix  : `/bin/sh -c <command>`
//! - Windows: `cmd.exe /C <command>`
//!
//! # Choosing between `sh` and `process`
//!
//! | Concern                          | Use `sh`            | Use `process`       |
//! |----------------------------------|---------------------|---------------------|
//! | Need pipes, globs, redirects     | ✓                   |                     |
//! | Need a timeout                   |                     | ✓ (`run`)           |
//! | Need to set environment vars     |                     | ✓ (`exec` / `run`)  |
//! | Need to provide stdin            |                     | ✓ (`exec` / `run`)  |
//! | Need async / background process  |                     | ✓ (`spawn`)         |
//! | Avoid shell-injection risk       |                     | ✓ (argv list)       |
//! | Quick one-liner / script snippet | ✓                   |                     |
//!
//! `process` functions take an explicit argv list and never invoke a shell, so
//! they are immune to shell injection. `sh` trades that safety for the
//! convenience of full shell syntax.
//!
//! # ⚠  Shell-Injection Hazard
//!
//! The `command` string is forwarded **as-is** to the shell.
//! **Never interpolate untrusted or externally-supplied data into the command
//! string.** Doing so allows arbitrary command execution:
//!
//! ```python
//! # UNSAFE – user_input could be "foo; rm -rf /"
//! sh.run("process " + user_input)
//!
//! # SAFE – use process.exec / process.run with an argv list instead
//! process.exec({"command": "process", "args": [user_input]})
//! ```
//!
//! If you must embed a dynamic value in a shell command, shell-quote it first
//! (e.g. wrap in single quotes and escape any embedded single quotes as `'\''`).
//!
//! # Quoting rules
//!
//! Because the command goes through the shell, normal shell quoting applies:
//!
//! - **Single quotes** (`'…'`) pass every character literally — no variable
//!   expansion or backslash interpretation inside.
//! - **Double quotes** (`"…"`) allow `$VAR` and `\` escapes but protect spaces.
//! - **Unquoted** tokens are subject to word-splitting and glob expansion.
//!
//! Example — count files whose names may contain spaces:
//! ```python
//! result = sh.run("find . -name '*.log' | wc -l", check=True)
//! ```
//!
//! # Windows notes
//!
//! On Windows the underlying shell is `cmd.exe /C`.  Several POSIX constructs
//! are not available or behave differently:
//!
//! - Use `%VAR%` for environment-variable expansion (not `$VAR`).
//! - The `test` builtin does not exist; use `if exist <file>` instead.
//! - `true` / `false` do not exist; use `exit /b 0` / `exit /b 1`.
//! - `2>&1` redirection works the same way.
//!
//! For cross-platform scripts, consider using `process.exec` with explicit
//! arguments, or guard shell-specific code with `sys.platform()`.
//!
//! # Known limitations
//!
//! - **No explicit `env` parameter** – environment variables can be set inline
//!   on POSIX (`FOO=bar command`) but this is not portable to Windows.  Use
//!   `process.exec` / `process.run` when you need explicit env control.
//! - **No `stdin`** – use `process.exec` when stdin must be provided.
//! - **No timeout** – use `process.run` (with `timeout_ms`) when you need one.

use crate::is_lsp_mode;
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use std::process::Command;

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Runs a shell command and returns its exit status, stdout, and stderr.
    ///
    /// The command is executed by the **platform shell**:
    /// - Unix: `/bin/sh -c <command>`
    /// - Windows: `cmd.exe /C <command>`
    ///
    /// Shell features such as pipes (`|`), redirections (`>`, `2>&1`),
    /// semicolons, and globs are fully supported.
    ///
    /// # ⚠  Shell-Injection Warning
    ///
    /// Do **not** interpolate untrusted input into `command`.  See the module
    /// documentation for details and safe alternatives.
    ///
    /// # Arguments
    ///
    /// * `command` – Shell command string to execute.
    /// * `check`   – If `True`, return an error when the command exits with a
    ///               non-zero status.  Defaults to `False`.
    /// * `cwd`     – Optional working directory for the command.
    ///
    /// # Returns
    ///
    /// A `dict` with:
    /// - `status` (`int`): exit code of the command (0 = success).
    /// - `stdout` (`str`): captured standard output.
    /// - `stderr` (`str`): captured standard error.
    ///
    /// # Errors
    ///
    /// Returns an error if `check=True` and the command exits with a non-zero
    /// status, or if the process cannot be spawned.
    ///
    /// # Example
    ///
    /// ```python
    /// result = sh.run("cat *.log | grep ERROR | wc -l", check=True)
    /// print(result["stdout"])
    ///
    /// # Capture stderr alongside stdout
    /// result = sh.run("some_tool 2>&1")
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

    /// Runs a shell command and returns its stdout as a trimmed string.
    ///
    /// Trailing newlines and carriage returns are stripped from the output.
    /// This is the most convenient function when you only need the output of a
    /// command and want errors to abort immediately.
    ///
    /// The command is executed by the **platform shell** (`/bin/sh -c` on
    /// Unix, `cmd.exe /C` on Windows).
    ///
    /// # ⚠  Shell-Injection Warning
    ///
    /// Do **not** interpolate untrusted input into `command`.  See the module
    /// documentation for details and safe alternatives.
    ///
    /// # Arguments
    ///
    /// * `command` – Shell command string to execute.
    /// * `check`   – If `True` (the default), return an error when the command
    ///               exits with a non-zero status.  Set to `False` to ignore
    ///               failures and return whatever output was produced.
    /// * `cwd`     – Optional working directory for the command.
    ///
    /// # Returns
    ///
    /// The command's stdout with trailing whitespace stripped.
    ///
    /// # Errors
    ///
    /// Returns an error if `check=True` (the default) and the command exits
    /// with a non-zero status, or if the process cannot be spawned.
    ///
    /// # Example
    ///
    /// ```python
    /// head = sh.capture("git rev-parse HEAD")
    ///
    /// # Suppress errors and fall back to a default
    /// branch = sh.capture("git rev-parse --abbrev-ref HEAD", check=False)
    /// if not branch:
    ///     branch = "unknown"
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

    /// Runs a shell command and returns its stdout split into individual lines.
    ///
    /// Each line of the command's standard output becomes one element of the
    /// returned list.  A trailing empty line (the newline after the last output
    /// line) is **not** included in the result.
    ///
    /// The command is executed by the **platform shell** (`/bin/sh -c` on
    /// Unix, `cmd.exe /C` on Windows).
    ///
    /// # ⚠  Shell-Injection Warning
    ///
    /// Do **not** interpolate untrusted input into `command`.  See the module
    /// documentation for details and safe alternatives.
    ///
    /// # Arguments
    ///
    /// * `command` – Shell command string to execute.
    /// * `check`   – If `True` (the default), return an error when the command
    ///               exits with a non-zero status.
    /// * `cwd`     – Optional working directory for the command.
    ///
    /// # Returns
    ///
    /// A `list[str]` — one element per output line.  Returns an empty list
    /// when the command produces no output.
    ///
    /// # Errors
    ///
    /// Returns an error if `check=True` (the default) and the command exits
    /// with a non-zero status, or if the process cannot be spawned.
    ///
    /// # Example
    ///
    /// ```python
    /// files = sh.lines("ls -1")
    /// for f in files:
    ///     print(f)
    ///
    /// # Safely handle commands that may find nothing
    /// matches = sh.lines("grep -rl 'TODO' src/", check=False)
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

    /// Runs a shell command and returns only its numeric exit code.
    ///
    /// This function **never** returns an error for a non-zero exit status —
    /// it only fails if the process cannot be spawned or waited on.  It is
    /// useful for conditional logic where the output of the command is not
    /// needed.
    ///
    /// The command is executed by the **platform shell** (`/bin/sh -c` on
    /// Unix, `cmd.exe /C` on Windows).
    ///
    /// # ⚠  Shell-Injection Warning
    ///
    /// Do **not** interpolate untrusted input into `command`.  See the module
    /// documentation for details and safe alternatives.
    ///
    /// # Arguments
    ///
    /// * `command` – Shell command string to execute.
    /// * `cwd`     – Optional working directory for the command.
    ///
    /// # Returns
    ///
    /// The command's exit code as an `int` (0 = success, non-zero = failure).
    /// Returns `1` if the process terminates without a numeric exit code (e.g.
    /// killed by a signal on Unix).
    ///
    /// # Errors
    ///
    /// Returns an error only if the process cannot be spawned or waited on.
    ///
    /// # Example
    ///
    /// ```python
    /// code = sh.exit_code("test -f config.json")
    /// if code == 0:
    ///     print("config exists")
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

/// Internal helper that spawns the platform shell to execute `command`.
///
/// On Unix this is `/bin/sh -c <command>`; on Windows `cmd.exe /C <command>`.
///
/// `command` is forwarded verbatim to the shell — **no quoting or escaping is
/// applied**.  Callers are responsible for ensuring the string is safe to pass
/// to the shell.  Embedding untrusted input without escaping constitutes a
/// shell-injection vulnerability.
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
