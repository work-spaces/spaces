use crate::is_lsp_mode;
use anyhow::{Context, bail};
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exec {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub working_directory: Option<String>,
    pub stdin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunOptions {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub cwd: Option<String>,
    pub stdin: Option<String>,
    pub stdout: Option<StdoutSpec>,
    pub stderr: Option<StderrSpec>,
    pub timeout_ms: Option<u64>,
    pub check: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StdoutSpec {
    Mode(String), // "inherit" | "capture" | "null"
    File { file: String },
}

// DEFECT 1 FIX: Added File variant so {"file": "path"} can deserialize correctly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StderrSpec {
    Mode(String), // "inherit" | "capture" | "null" | "merge"
    File { file: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOutcome {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: i64,
}

// DEFECT 5 FIX: Added merge_stderr field so wait() can append stderr to stdout when requested.
#[derive(Debug)]
struct ChildHandle {
    child: Child,
    started: Instant,
    merge_stderr: bool,
}

static PROCESS_REGISTRY: OnceLock<Mutex<HashMap<u64, ChildHandle>>> = OnceLock::new();
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

fn process_registry() -> &'static Mutex<HashMap<u64, ChildHandle>> {
    PROCESS_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn execute_run(opts: RunOptions) -> anyhow::Result<RunOutcome> {
    let started = Instant::now();

    let mut cmd = Command::new(&opts.command);

    for a in opts.args.unwrap_or_default() {
        cmd.arg(a);
    }

    for (k, v) in opts.env.unwrap_or_default() {
        cmd.env(k, v);
    }

    if let Some(dir) = opts.cwd {
        cmd.current_dir(dir);
    }

    if opts.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }

    let stdout_spec = opts
        .stdout
        .unwrap_or_else(|| StdoutSpec::Mode("capture".to_string()));
    let stderr_spec = opts
        .stderr
        .unwrap_or_else(|| StderrSpec::Mode("capture".to_string()));

    let mut capture_stdout = false;
    let mut capture_stderr = false;

    match stdout_spec {
        StdoutSpec::Mode(mode) => match mode.as_str() {
            "inherit" => {
                cmd.stdout(Stdio::inherit());
            }
            "capture" => {
                capture_stdout = true;
                cmd.stdout(Stdio::piped());
            }
            "null" => {
                cmd.stdout(Stdio::null());
            }
            other => bail!("invalid stdout mode: {other}"),
        },
        StdoutSpec::File { file } => {
            let file_handle = std::fs::File::create(&file)
                .context(format_context!("failed to open stdout file: {file}"))?;
            cmd.stdout(Stdio::from(file_handle));
        }
    }

    // DEFECT 1 FIX: Added StderrSpec::File arm so stderr can be redirected to a file.
    let merge_stderr_into_stdout = match stderr_spec {
        StderrSpec::Mode(mode) => match mode.as_str() {
            "inherit" => {
                cmd.stderr(Stdio::inherit());
                false
            }
            "capture" => {
                capture_stderr = true;
                cmd.stderr(Stdio::piped());
                false
            }
            "null" => {
                cmd.stderr(Stdio::null());
                false
            }
            "merge" => {
                cmd.stderr(Stdio::piped());
                true
            }
            other => bail!("invalid stderr mode: {other}"),
        },
        StderrSpec::File { file } => {
            let file_handle = std::fs::File::create(&file)
                .context(format_context!("failed to open stderr file: {file}"))?;
            cmd.stderr(Stdio::from(file_handle));
            false
        }
    };

    let mut child = cmd.spawn().context(format_context!(
        "Failed to spawn child process {}",
        opts.command
    ))?;

    // DEFECT 3 FIX: Use take() so child_stdin is dropped immediately after write_all(),
    // sending EOF to the child. Without this, in the timeout polling loop the child never
    // receives EOF and try_wait() never returns Some for programs that read until EOF.
    if let Some(input) = opts.stdin {
        if let Some(mut child_stdin) = child.stdin.take() {
            child_stdin
                .write_all(input.as_bytes())
                .context(format_context!("Failed to write to stdin"))?;
            // child_stdin dropped here → EOF sent to child
        }
    }

    let output = if let Some(limit_ms) = opts.timeout_ms {
        loop {
            if child.try_wait()?.is_some() {
                break child.wait_with_output()?;
            }

            if started.elapsed().as_millis() as u64 >= limit_ms {
                let _ = child.kill();
                let _ = child.wait();
                bail!("process timed out after {limit_ms}ms");
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        child.wait_with_output()?
    };

    let mut stdout_text = String::new();
    let mut stderr_text = String::new();

    if capture_stdout || merge_stderr_into_stdout {
        stdout_text = String::from_utf8_lossy(&output.stdout).to_string();
    }

    if capture_stderr {
        stderr_text = String::from_utf8_lossy(&output.stderr).to_string();
    }

    if merge_stderr_into_stdout {
        let merged_err = String::from_utf8_lossy(&output.stderr);
        stdout_text.push_str(&merged_err);
    }

    let status = output.status.code().unwrap_or(1);

    if opts.check.unwrap_or(false) && status != 0 {
        bail!("process exited with status {status}");
    }

    Ok(RunOutcome {
        status,
        stdout: stdout_text,
        stderr: stderr_text,
        duration_ms: started.elapsed().as_millis() as i64,
    })
}

fn build_command(
    command: &str,
    args: Option<Vec<String>>,
    env: Option<HashMap<String, String>>,
    cwd: Option<String>,
    stdin: Option<String>,
) -> anyhow::Result<(Command, Option<String>)> {
    let mut cmd = Command::new(command);
    for a in args.unwrap_or_default() {
        cmd.arg(a);
    }
    for (k, v) in env.unwrap_or_default() {
        cmd.env(k, v);
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    Ok((cmd, stdin))
}

fn pipeline_execute(chain: Vec<RunOptions>) -> anyhow::Result<RunOutcome> {
    if chain.is_empty() {
        bail!("pipeline requires at least one run option");
    }

    let started = Instant::now();
    let mut previous_stdout: Option<String> = None;
    let mut final_stderr = String::new();
    let mut final_status = 0;

    let chain_len = chain.len();
    for (idx, mut opts) in chain.into_iter().enumerate() {
        if idx > 0 && opts.stdin.is_none() {
            opts.stdin = previous_stdout.take();
        }

        let is_final = idx == chain_len - 1;
        if !is_final {
            opts.stdout = Some(StdoutSpec::Mode("capture".to_string()));
            opts.stderr = Some(StderrSpec::Mode("capture".to_string()));
        }

        let outcome = execute_run(opts)?;
        final_status = outcome.status;
        final_stderr = outcome.stderr;
        previous_stdout = Some(outcome.stdout);
    }

    Ok(RunOutcome {
        status: final_status,
        stdout: previous_stdout.unwrap_or_default(),
        stderr: final_stderr,
        duration_ms: started.elapsed().as_millis() as i64,
    })
}

// This defines the functions that are visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Executes a process and captures its output and status.
    fn exec<'v>(
        exec: starlark::values::Value,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            let heap = eval.heap();
            let mut result_map = serde_json::Map::new();
            result_map.insert("status".to_string(), serde_json::Value::Number(0.into()));
            result_map.insert(
                "stdout".to_string(),
                serde_json::Value::String(String::new()),
            );
            result_map.insert(
                "stderr".to_string(),
                serde_json::Value::String(String::new()),
            );
            return Ok(heap.alloc(serde_json::Value::Object(result_map)));
        }
        let heap = eval.heap();

        let exec: Exec = serde_json::from_value(exec.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        let exec_stdin = exec.stdin;
        let invoke_command = exec.command.clone();

        let mut command = Command::new(exec.command);
        for arg in exec.args.unwrap_or_default() {
            command.arg(arg);
        }

        for (name, value) in exec.env.unwrap_or_default() {
            command.env(name, value);
        }

        if exec_stdin.is_some() {
            command.stdin(Stdio::piped());
        }

        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        if let Some(working_directory) = exec.working_directory {
            command.current_dir(working_directory);
        }

        let child_result = command.spawn();

        if let Ok(mut child) = child_result {
            if let Some(stdin) = exec_stdin {
                let child_stdin = child
                    .stdin
                    .as_mut()
                    .context(format_context!("stdin pipe was not available"))?;
                child_stdin
                    .write_all(stdin.as_bytes())
                    .context(format_context!("Failed to write to stdin"))?;
            }

            let output_result = child.wait_with_output();
            let (status, stdout, stderr) = match output_result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    (
                        output.status.code().unwrap_or(1),
                        stdout.to_string(),
                        stderr.to_string(),
                    )
                }
                Err(e) => (1, String::new(), e.to_string()),
            };

            let mut result_map = serde_json::Map::new();
            result_map.insert(
                "status".to_string(),
                serde_json::Value::Number(status.into()),
            );
            result_map.insert("stdout".to_string(), serde_json::Value::String(stdout));
            result_map.insert("stderr".to_string(), serde_json::Value::String(stderr));
            Ok(heap.alloc(serde_json::Value::Object(result_map)))
        } else {
            Err(child_result.unwrap_err()).context(format_context!(
                "Failed to spawn child process {invoke_command}"
            ))
        }
    }

    /// Streaming-capable run with explicit redirection and timeout/check behavior.
    fn run<'v>(
        options: starlark::values::Value,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            let heap = eval.heap();
            let outcome = RunOutcome {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 0,
            };
            return Ok(heap.alloc(
                serde_json::to_value(outcome)
                    .context(format_context!("failed to serialize result"))?,
            ));
        }
        let heap = eval.heap();

        let opts: RunOptions = serde_json::from_value(options.to_json_value()?)
            .context(format_context!("bad options for run"))?;
        let outcome = execute_run(opts)?;
        Ok(heap.alloc(
            serde_json::to_value(outcome).context(format_context!("failed to serialize result"))?,
        ))
    }

    /// Execute commands serially, piping stdout of each into stdin of the next.
    ///
    /// Input: list[RunOptions]
    /// Output: {"status": int, "stdout": str, "stderr": str, "duration_ms": int}
    fn pipeline<'v>(
        steps: starlark::values::Value,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            let heap = eval.heap();
            let outcome = RunOutcome {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
                duration_ms: 0,
            };
            return Ok(heap.alloc(
                serde_json::to_value(outcome)
                    .context(format_context!("failed to serialize result"))?,
            ));
        }
        let heap = eval.heap();

        let chain: Vec<RunOptions> = serde_json::from_value(steps.to_json_value()?)
            .context(format_context!("bad options for pipeline"))?;

        let outcome = pipeline_execute(chain)?;
        Ok(heap.alloc(
            serde_json::to_value(outcome).context(format_context!("failed to serialize result"))?,
        ))
    }

    /// `$(...)`-style helper: run a command and return trimmed stdout.
    /// Raises on non-zero status.
    fn capture<'v>(
        argv: starlark::values::Value,
        _eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let argv: Vec<String> = serde_json::from_value(argv.to_json_value()?)
            .context(format_context!("bad argv for capture"))?;

        if argv.is_empty() {
            bail!("capture requires at least one argv element");
        }

        let command = argv[0].clone();
        let args = if argv.len() > 1 {
            Some(argv[1..].to_vec())
        } else {
            None
        };

        let outcome = execute_run(RunOptions {
            command,
            args,
            env: None,
            cwd: None,
            stdin: None,
            stdout: Some(StdoutSpec::Mode("capture".to_string())),
            stderr: Some(StderrSpec::Mode("capture".to_string())),
            timeout_ms: None,
            check: Some(true),
        })?;

        Ok(outcome.stdout.trim().to_string())
    }

    /// Spawn a background process and return an opaque numeric handle.
    ///
    /// Example:
    /// handle = process.spawn({"command": "server", "args": ["--port", "8080"]})
    fn spawn<'v>(
        options: starlark::values::Value,
        _eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<u64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let opts: RunOptions = serde_json::from_value(options.to_json_value()?)
            .context(format_context!("bad options for spawn"))?;

        let (mut cmd, stdin_payload) = build_command(
            &opts.command,
            opts.args,
            opts.env,
            opts.cwd,
            opts.stdin.clone(),
        )?;

        // For background jobs: default to inheriting stdout/stderr unless explicitly configured.
        match opts
            .stdout
            .unwrap_or_else(|| StdoutSpec::Mode("inherit".to_string()))
        {
            StdoutSpec::Mode(mode) => match mode.as_str() {
                "inherit" => {
                    cmd.stdout(Stdio::inherit());
                }
                "capture" => {
                    cmd.stdout(Stdio::piped());
                }
                "null" => {
                    cmd.stdout(Stdio::null());
                }
                other => bail!("invalid stdout mode: {other}"),
            },
            StdoutSpec::File { file } => {
                let file_handle = std::fs::File::create(&file)
                    .context(format_context!("failed to open stdout file: {file}"))?;
                cmd.stdout(Stdio::from(file_handle));
            }
        }

        // DEFECT 1 FIX: Added StderrSpec::File arm.
        // DEFECT 5 FIX: "merge" now uses Stdio::piped() so stderr output is captured and
        // can be appended to stdout in wait(). Previously it used Stdio::inherit() which
        // sent stderr to the terminal instead of into the capture buffer.
        let mut merge_stderr = false;
        match opts
            .stderr
            .unwrap_or_else(|| StderrSpec::Mode("inherit".to_string()))
        {
            StderrSpec::Mode(mode) => match mode.as_str() {
                "inherit" => {
                    cmd.stderr(Stdio::inherit());
                }
                "capture" => {
                    cmd.stderr(Stdio::piped());
                }
                "null" => {
                    cmd.stderr(Stdio::null());
                }
                "merge" => {
                    // Pipe stderr so wait() can read and append it to stdout.
                    cmd.stderr(Stdio::piped());
                    merge_stderr = true;
                }
                other => bail!("invalid stderr mode: {other}"),
            },
            StderrSpec::File { file } => {
                let file_handle = std::fs::File::create(&file)
                    .context(format_context!("failed to open stderr file: {file}"))?;
                cmd.stderr(Stdio::from(file_handle));
            }
        }

        let mut child = cmd.spawn().context(format_context!(
            "Failed to spawn child process {}",
            opts.command
        ))?;

        // DEFECT 4 FIX: Use take() so child_stdin is dropped immediately after write_all(),
        // sending EOF to the spawned process. Without this, the process never gets EOF on stdin.
        if let Some(input) = stdin_payload {
            if let Some(mut child_stdin) = child.stdin.take() {
                child_stdin
                    .write_all(input.as_bytes())
                    .context(format_context!("Failed to write to stdin"))?;
                // child_stdin dropped here → EOF sent to child
            }
        }

        let handle = NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed);
        let mut registry = process_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("process registry lock poisoned"))?;
        registry.insert(
            handle,
            ChildHandle {
                child,
                started: Instant::now(),
                merge_stderr,
            },
        );

        Ok(handle)
    }

    /// Returns true if the process associated with the handle is still running.
    fn is_running(handle: u64) -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }
        let mut registry = process_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("process registry lock poisoned"))?;

        let Some(entry) = registry.get_mut(&handle) else {
            bail!("unknown process handle: {handle}");
        };

        Ok(entry.child.try_wait()?.is_none())
    }

    /// Send a signal to a background process.
    ///
    /// Supported values:
    /// - "SIGTERM" (default): graceful terminate
    /// - "SIGKILL": hard kill
    fn kill(handle: u64, signal: Option<String>) -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }
        let mut registry = process_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("process registry lock poisoned"))?;

        let Some(entry) = registry.get_mut(&handle) else {
            bail!("unknown process handle: {handle}");
        };

        let sig = signal.unwrap_or_else(|| "SIGTERM".to_string());

        // DEFECT 2 FIX: Previously both "SIGTERM" and "SIGKILL" called child.kill() which
        // always sends SIGKILL on Unix. Now SIGTERM uses libc::kill(pid, SIGTERM) on Unix
        // for a proper graceful-terminate signal.
        match sig.as_str() {
            "SIGTERM" => {
                #[cfg(unix)]
                {
                    let pid = entry.child.id() as libc::pid_t;
                    let ret = unsafe { libc::kill(pid, libc::SIGTERM) };
                    if ret != 0 {
                        bail!("kill(SIGTERM) failed: {}", std::io::Error::last_os_error());
                    }
                }
                #[cfg(not(unix))]
                {
                    // On non-Unix platforms there is no SIGTERM; best-effort terminate.
                    entry.child.kill()?;
                }
                Ok(true)
            }
            "SIGKILL" => {
                entry.child.kill()?;
                Ok(true)
            }
            other => bail!("unsupported signal: {other}"),
        }
    }

    /// Wait for background process completion.
    ///
    /// Returns:
    /// {"status": int, "stdout": str, "stderr": str, "duration_ms": int}
    fn wait<'v>(
        handle: u64,
        timeout_ms: Option<u64>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            let heap = eval.heap();
            let result = serde_json::json!({
                "status": 0,
                "stdout": "",
                "stderr": "",
                "duration_ms": 0,
            });
            return Ok(heap.alloc(result));
        }
        let heap = eval.heap();

        let mut registry = process_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("process registry lock poisoned"))?;

        let Some(mut entry) = registry.remove(&handle) else {
            bail!("unknown process handle: {handle}");
        };

        let started_poll = Instant::now();
        if let Some(limit_ms) = timeout_ms {
            loop {
                if entry.child.try_wait()?.is_some() {
                    break;
                }

                // DEFECT 6 FIX: Kill the child before bailing instead of putting it back
                // in the registry. The handle is consumed on timeout; leaving the child
                // running indefinitely was incorrect.
                if started_poll.elapsed().as_millis() as u64 >= limit_ms {
                    let _ = entry.child.kill();
                    let _ = entry.child.wait();
                    bail!("wait timed out after {limit_ms}ms");
                }

                std::thread::sleep(Duration::from_millis(10));
            }
        }

        // Capture duration and merge flag before child is consumed by wait_with_output().
        let merge_stderr = entry.merge_stderr;
        let duration_ms = entry.started.elapsed().as_millis() as i64;

        let output = entry.child.wait_with_output()?;
        let status = output.status.code().unwrap_or(1);

        let mut stdout_text = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();

        // DEFECT 5 FIX: If spawn was called with stderr="merge", append the captured
        // stderr bytes onto stdout so the caller sees the interleaved stream in stdout.
        if merge_stderr {
            stdout_text.push_str(&stderr_text);
        }

        let result = serde_json::json!({
            "status": status,
            "stdout": stdout_text,
            "stderr": if merge_stderr { String::new() } else { stderr_text },
            "duration_ms": duration_ms,
        });

        Ok(heap.alloc(result))
    }
}
