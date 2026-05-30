//! Shared helpers for reporting failed external-process executions.
//!
//! Both [`crate::process`] and [`crate::sh`] surface errors when a spawned
//! process exits non-zero (or times out). The helpers in this module produce a
//! consistent, human-readable rendering of the command, working directory,
//! exit status, and captured stderr so the user gets enough context to debug
//! the failure.

/// Format a `command + args` invocation for inclusion in error messages.
///
/// Arguments containing whitespace (or empty arguments) are wrapped in double
/// quotes, with embedded double quotes backslash-escaped, so the rendering is
/// unambiguous when copy/pasted into a shell.
pub fn format_command_line(command: &str, args: Option<&[String]>) -> String {
    let mut out = String::from(command);
    if let Some(args) = args {
        for a in args {
            out.push(' ');
            if a.is_empty() || a.chars().any(char::is_whitespace) {
                out.push('"');
                out.push_str(&a.replace('"', "\\\""));
                out.push('"');
            } else {
                out.push_str(a);
            }
        }
    }
    out
}

/// Format a rich failure message for a non-zero process exit.
///
/// `label` is the short verb-phrase prefix (e.g. `"process"` or
/// `"shell command"`) that begins the message. The result has the shape:
///
/// ```text
/// <label> exited with status <status>
///   command: <command_line>
///   cwd: <cwd>            (only when Some)
///   stderr:               (only when stderr is non-empty after trimming)
///     <line 1>
///     <line 2>
/// ```
pub fn format_failure(
    label: &str,
    command_line: &str,
    cwd: Option<&str>,
    status: i32,
    stderr: &str,
) -> String {
    let mut msg = format!("{label} exited with status {status}\n  command: {command_line}");
    if let Some(cwd) = cwd {
        msg.push_str(&format!("\n  cwd: {cwd}"));
    }
    let trimmed = stderr.trim_end();
    if !trimmed.is_empty() {
        msg.push_str("\n  stderr:");
        for line in trimmed.lines() {
            msg.push_str("\n    ");
            msg.push_str(line);
        }
    }
    msg
}

/// Format a rich failure message for a process that exceeded its timeout.
///
/// Output shape:
///
/// ```text
/// <label> timed out after <limit_ms>ms
///   command: <command_line>
///   cwd: <cwd>            (only when Some)
/// ```
pub fn format_timeout(label: &str, command_line: &str, cwd: Option<&str>, limit_ms: u64) -> String {
    let mut msg = format!("{label} timed out after {limit_ms}ms\n  command: {command_line}");
    if let Some(cwd) = cwd {
        msg.push_str(&format!("\n  cwd: {cwd}"));
    }
    msg
}
