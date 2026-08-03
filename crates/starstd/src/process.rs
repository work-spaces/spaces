use crate::is_lsp_mode;
use crate::process_error::{format_command_line, format_failure, format_timeout};
use anyhow::{Context, bail};
use anyhow_source_location::{format_context, format_error};
use portable_pty::{
    Child as PtyChild, CommandBuilder, ExitStatus as PtyExitStatus, PtySize, native_pty_system,
};
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exec {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub working_directory: Option<String>,
    pub stdin: Option<String>,
    #[serde(default, alias = "use_pty", alias = "pseudo_terminal")]
    pub pty: Option<bool>,
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
    pub stdout_path: Option<String>,
    pub stderr_path: Option<String>,
    pub tee: Option<bool>,
    pub allow_orphans: Option<bool>,
    pub output_buffer_limit_bytes: Option<u64>,
    #[serde(default, alias = "use_pty", alias = "pseudo_terminal")]
    pub pty: Option<bool>,
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
struct ManagedExitStatus {
    code: Option<i32>,
}

impl ManagedExitStatus {
    fn from_std(status: ExitStatus) -> Self {
        Self {
            code: status.code(),
        }
    }

    fn from_pty(status: PtyExitStatus) -> Self {
        Self {
            code: i32::try_from(status.exit_code()).ok(),
        }
    }

    fn code(&self) -> Option<i32> {
        self.code
    }
}

#[derive(Debug)]
enum ManagedChild {
    Std(Child),
    Pty(Box<dyn PtyChild + Send>),
}

type SpawnedPtyProcess = (
    ManagedChild,
    Box<dyn Read + Send>,
    Option<Box<dyn Write + Send>>,
);

impl ManagedChild {
    fn try_wait(&mut self) -> anyhow::Result<Option<ManagedExitStatus>> {
        match self {
            ManagedChild::Std(child) => child
                .try_wait()
                .map(|status| status.map(ManagedExitStatus::from_std))
                .map_err(Into::into),
            ManagedChild::Pty(child) => child
                .try_wait()
                .map(|status| status.map(ManagedExitStatus::from_pty))
                .map_err(Into::into),
        }
    }

    fn wait(&mut self) -> anyhow::Result<ManagedExitStatus> {
        match self {
            ManagedChild::Std(child) => child
                .wait()
                .map(ManagedExitStatus::from_std)
                .map_err(Into::into),
            ManagedChild::Pty(child) => child
                .wait()
                .map(ManagedExitStatus::from_pty)
                .map_err(Into::into),
        }
    }

    fn kill(&mut self) -> anyhow::Result<()> {
        match self {
            ManagedChild::Std(child) => child.kill().map_err(Into::into),
            ManagedChild::Pty(child) => child.kill().map_err(Into::into),
        }
    }

    fn id(&self) -> Option<u32> {
        match self {
            ManagedChild::Std(child) => Some(child.id()),
            ManagedChild::Pty(child) => child.process_id(),
        }
    }
}

#[derive(Debug)]
struct ChildHandle {
    child: ManagedChild,
    started: Instant,
    merge_stderr: bool,
    allow_orphans: bool,
    exit_status: Option<ManagedExitStatus>,
    stdout_buf: Arc<Mutex<Vec<u8>>>,
    stderr_buf: Arc<Mutex<Vec<u8>>>,
    stdout_reader: Option<JoinHandle<anyhow::Result<()>>>,
    stderr_reader: Option<JoinHandle<anyhow::Result<()>>>,
}

static PROCESS_REGISTRY: OnceLock<Mutex<HashMap<u64, ChildHandle>>> = OnceLock::new();
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);
static EXIT_CLEANUP_REGISTERED: OnceLock<bool> = OnceLock::new();

const DEFAULT_SPAWN_OUTPUT_BUFFER_LIMIT_BYTES: usize = 1024 * 1024;

fn process_registry() -> &'static Mutex<HashMap<u64, ChildHandle>> {
    PROCESS_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

extern "C" fn cleanup_spawned_processes_on_exit() {
    let Ok(mut registry) = process_registry().lock() else {
        return;
    };

    for (_, mut entry) in registry.drain() {
        if entry.allow_orphans {
            continue;
        }

        if let Ok(Some(_)) = entry.child.try_wait() {
            continue;
        }

        let _ = entry.child.kill();
        let _ = entry.child.wait();
        let _ = join_output_pump(entry.stdout_reader.take(), "stdout");
        let _ = join_output_pump(entry.stderr_reader.take(), "stderr");
    }
}

fn ensure_exit_cleanup_registered() -> anyhow::Result<()> {
    let registered = EXIT_CLEANUP_REGISTERED.get_or_init(|| {
        // SAFETY: `cleanup_spawned_processes_on_exit` has C ABI and no captured state.
        unsafe { libc::atexit(cleanup_spawned_processes_on_exit) == 0 }
    });

    if *registered {
        Ok(())
    } else {
        bail!("failed to register process exit cleanup hook")
    }
}

fn append_bounded(buffer: &mut Vec<u8>, chunk: &[u8], max_bytes: usize) {
    if max_bytes == 0 {
        buffer.clear();
        return;
    }

    if chunk.len() >= max_bytes {
        buffer.clear();
        buffer.extend_from_slice(&chunk[chunk.len() - max_bytes..]);
        return;
    }

    let required_len = buffer.len().saturating_add(chunk.len());
    if required_len > max_bytes {
        let overflow = required_len - max_bytes;
        if overflow >= buffer.len() {
            buffer.clear();
        } else {
            buffer.drain(..overflow);
        }
    }

    buffer.extend_from_slice(chunk);
}

fn spawn_pty_output_pump<R: Read + Send + 'static>(
    mut reader: R,
    buffer: Arc<Mutex<Vec<u8>>>,
    max_bytes: usize,
    tee: bool,
    tee_to_stdout: bool,
    output_file: Option<std::fs::File>,
) -> JoinHandle<anyhow::Result<()>> {
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 8192];
        let mut output_file = output_file;

        loop {
            let n = match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(anyhow::anyhow!("failed reading child output: {e}")),
            };

            let bytes = &chunk[..n];
            {
                let mut guard = buffer
                    .lock()
                    .map_err(|_| anyhow::anyhow!("process output buffer lock poisoned"))?;
                append_bounded(&mut guard, bytes, max_bytes);
            }

            if let Some(file) = output_file.as_mut() {
                let _ = file.write_all(bytes);
                let _ = file.flush();
            }

            if tee {
                if tee_to_stdout {
                    let _ = std::io::stdout().write_all(bytes);
                    let _ = std::io::stdout().flush();
                } else {
                    let _ = std::io::stderr().write_all(bytes);
                    let _ = std::io::stderr().flush();
                }
            }
        }

        Ok(())
    })
}

fn spawn_output_pump<R: Read + Send + 'static>(
    mut reader: R,
    buffer: Arc<Mutex<Vec<u8>>>,
    max_bytes: usize,
    tee: bool,
    tee_to_stdout: bool,
) -> JoinHandle<anyhow::Result<()>> {
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 8192];

        loop {
            let n = match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(anyhow::anyhow!("failed reading child output: {e}")),
            };

            {
                let mut guard = buffer
                    .lock()
                    .map_err(|_| anyhow::anyhow!("process output buffer lock poisoned"))?;
                append_bounded(&mut guard, &chunk[..n], max_bytes);
            }

            if tee {
                if tee_to_stdout {
                    let _ = std::io::stdout().write_all(&chunk[..n]);
                    let _ = std::io::stdout().flush();
                } else {
                    let _ = std::io::stderr().write_all(&chunk[..n]);
                    let _ = std::io::stderr().flush();
                }
            }
        }

        Ok(())
    })
}

fn join_output_pump(
    join: Option<JoinHandle<anyhow::Result<()>>>,
    stream: &str,
) -> anyhow::Result<()> {
    let Some(join) = join else {
        return Ok(());
    };

    match join.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(anyhow::anyhow!("{stream} pump failed: {err}")),
        Err(_) => Err(anyhow::anyhow!("{stream} pump panicked")),
    }
}

fn read_output_buffer(buffer: &Arc<Mutex<Vec<u8>>>, drain: bool) -> anyhow::Result<Vec<u8>> {
    let mut guard = buffer
        .lock()
        .map_err(|_| anyhow::anyhow!("process output buffer lock poisoned"))?;

    if drain {
        Ok(guard.drain(..).collect())
    } else {
        Ok(guard.clone())
    }
}

fn read_output_lines(
    buffer: &Arc<Mutex<Vec<u8>>>,
    drain: bool,
    max_lines: Option<usize>,
) -> anyhow::Result<Vec<String>> {
    let mut guard = buffer
        .lock()
        .map_err(|_| anyhow::anyhow!("process output buffer lock poisoned"))?;

    if max_lines == Some(0) {
        return Ok(Vec::new());
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut consumed = 0usize;

    for (idx, byte) in guard.iter().enumerate() {
        if *byte == b'\n' {
            let mut line = &guard[start..idx];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            lines.push(String::from_utf8_lossy(line).to_string());
            start = idx + 1;
            consumed = start;

            if let Some(limit) = max_lines
                && lines.len() >= limit
            {
                break;
            }
        }
    }

    if drain && consumed > 0 {
        guard.drain(..consumed);
    }

    Ok(lines)
}

fn build_pty_command(
    command: &str,
    args: Option<Vec<String>>,
    env: Option<HashMap<String, String>>,
    cwd: Option<String>,
) -> anyhow::Result<CommandBuilder> {
    let mut cmd = CommandBuilder::new(command);
    for a in args.unwrap_or_default() {
        cmd.arg(a);
    }
    for (k, v) in env.unwrap_or_default() {
        cmd.env(k, v);
    }
    if let Some(dir) = cwd {
        cmd.cwd(dir);
    }
    cmd.env("TERM", "xterm-256color");
    Ok(cmd)
}

fn spawn_pty_process(
    command: &str,
    args: Option<Vec<String>>,
    env: Option<HashMap<String, String>>,
    cwd: Option<String>,
    stdin_payload: Option<String>,
) -> anyhow::Result<SpawnedPtyProcess> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize::default())
        .context(format_context!(
            "failed to open pty for child process {command}"
        ))?;
    let cmd = build_pty_command(command, args, env, cwd)?;
    let child = pair.slave.spawn_command(cmd).context(format_context!(
        "failed to spawn child process {command} into pty"
    ))?;
    let reader = pair.master.try_clone_reader().context(format_context!(
        "failed to read from pty master for child process {command}"
    ))?;
    let writer = if stdin_payload.is_some() {
        Some(pair.master.take_writer().context(format_context!(
            "failed to acquire pty stdin writer for child process {command}"
        ))?)
    } else {
        None
    };
    Ok((ManagedChild::Pty(child), reader, writer))
}

/// Build a `Command` and run it, capturing/streaming output per `opts`.
fn execute_run(opts: RunOptions) -> anyhow::Result<RunOutcome> {
    let started = Instant::now();

    // Snapshot a human-readable rendering of the command for error messages,
    // since `opts` will be partially consumed below.
    let command_line = format_command_line(&opts.command, opts.args.as_deref());
    let cwd_display = opts.cwd.clone();

    if opts.pty.unwrap_or(false) {
        return execute_run_with_pty(opts, started, command_line, cwd_display);
    }

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
    // Only keep a cloned stdout file handle around when stderr="merge" needs it (2>&1 style).
    let stderr_is_merge = matches!(&stderr_spec, StderrSpec::Mode(m) if m == "merge");
    let mut stdout_file = None;

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
            if stderr_is_merge {
                let dup = file_handle.try_clone().context(format_context!(
                    "failed to clone stdout file handle: {file}"
                ))?;
                stdout_file = Some(dup);
            }
            cmd.stdout(Stdio::from(file_handle));
        }
    }

    // DEFECT 1 FIX: Added StderrSpec::File arm so stderr can be redirected to a file.
    // When stderr is "merge" and stdout is targeting a file, redirect stderr to a clone
    // of that same file (OS-level 2>&1) so stderr actually reaches the file.
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
                if let Some(file) = stdout_file.take() {
                    cmd.stderr(Stdio::from(file));
                    false
                } else {
                    cmd.stderr(Stdio::piped());
                    true
                }
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
    if let Some(input) = opts.stdin
        && let Some(mut child_stdin) = child.stdin.take()
    {
        child_stdin
            .write_all(input.as_bytes())
            .context(format_context!("Failed to write to stdin"))?;
        // child_stdin dropped here → EOF sent to child
    }

    let output = if let Some(limit_ms) = opts.timeout_ms {
        loop {
            if child.try_wait()?.is_some() {
                break child.wait_with_output()?;
            }

            if started.elapsed().as_millis() as u64 >= limit_ms {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "{}",
                    format_timeout("process", &command_line, cwd_display.as_deref(), limit_ms)
                );
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

    // Write captured output to disk if paths specified
    if let Some(path) = opts.stdout_path {
        std::fs::write(&path, &output.stdout)
            .context(format_context!("Failed to write stdout to file: {path}"))?;
    }

    if let Some(path) = opts.stderr_path {
        std::fs::write(&path, &output.stderr)
            .context(format_context!("Failed to write stderr to file: {path}"))?;
    }

    // Tee output to parent process streams if requested
    if opts.tee.unwrap_or(false) {
        if capture_stdout || merge_stderr_into_stdout {
            std::io::stdout()
                .write_all(&output.stdout)
                .context(format_context!("Failed to tee stdout"))?;
        }
        if capture_stderr {
            std::io::stderr()
                .write_all(&output.stderr)
                .context(format_context!("Failed to tee stderr"))?;
        }
    }

    if opts.check.unwrap_or(false) && status != 0 {
        // Use the shared helper so process/shell failures share one format.
        // When stderr was redirected (file/inherit/null) `stderr_text` is
        // empty and the helper omits the `stderr:` section.
        bail!(
            "{}",
            format_failure(
                "process",
                &command_line,
                cwd_display.as_deref(),
                status,
                &stderr_text,
            )
        );
    }

    Ok(RunOutcome {
        status,
        stdout: stdout_text,
        stderr: stderr_text,
        duration_ms: started.elapsed().as_millis() as i64,
    })
}

fn execute_run_with_pty(
    opts: RunOptions,
    started: Instant,
    command_line: String,
    cwd_display: Option<String>,
) -> anyhow::Result<RunOutcome> {
    let stdout_spec = opts
        .stdout
        .clone()
        .unwrap_or_else(|| StdoutSpec::Mode("capture".to_string()));
    let stderr_spec = opts
        .stderr
        .clone()
        .unwrap_or_else(|| StderrSpec::Mode("capture".to_string()));
    let tee = opts.tee.unwrap_or(false);

    let output_buffer_limit_bytes = opts
        .output_buffer_limit_bytes
        .map(usize::try_from)
        .transpose()
        .map_err(|_| {
            anyhow::anyhow!("output_buffer_limit_bytes is too large for this platform's usize")
        })?
        .unwrap_or(DEFAULT_SPAWN_OUTPUT_BUFFER_LIMIT_BYTES);

    let mut capture_output;
    let mut tee_to_stdout = tee;
    let mut tee_to_stderr = false;
    let mut output_file = None;

    match stdout_spec {
        StdoutSpec::Mode(mode) => match mode.as_str() {
            "inherit" => {
                tee_to_stdout = true;
                capture_output = true;
            }
            "capture" => {
                capture_output = true;
            }
            "null" => {
                capture_output = false;
            }
            other => bail!("invalid stdout mode: {other}"),
        },
        StdoutSpec::File { file } => {
            let file_handle = std::fs::File::create(&file)
                .context(format_context!("failed to open stdout file: {file}"))?;
            output_file = Some(file_handle);
            capture_output = true;
        }
    }

    match stderr_spec {
        StderrSpec::Mode(mode) => match mode.as_str() {
            "inherit" => {
                tee_to_stderr = true;
            }
            "capture" | "merge" => {
                capture_output = true;
            }
            "null" => {}
            other => bail!("invalid stderr mode: {other}"),
        },
        StderrSpec::File { file } => {
            let file_handle = std::fs::File::create(&file)
                .context(format_context!("failed to open stderr file: {file}"))?;
            output_file = Some(file_handle);
            capture_output = true;
        }
    }

    let stdout_buf = Arc::new(Mutex::new(Vec::new()));
    let (mut child, reader, mut stdin_writer) = spawn_pty_process(
        &opts.command,
        opts.args,
        opts.env,
        opts.cwd,
        opts.stdin.clone(),
    )?;

    if let Some(input) = opts.stdin
        && let Some(mut writer) = stdin_writer.take()
    {
        writer
            .write_all(input.as_bytes())
            .context(format_context!("Failed to write to stdin"))?;
    }

    let output_reader = spawn_pty_output_pump(
        reader,
        Arc::clone(&stdout_buf),
        output_buffer_limit_bytes,
        tee || tee_to_stdout || tee_to_stderr,
        tee_to_stdout || (tee && !tee_to_stderr),
        output_file,
    );

    let exit_status = if let Some(limit_ms) = opts.timeout_ms {
        loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }

            if started.elapsed().as_millis() as u64 >= limit_ms {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_output_pump(Some(output_reader), "stdout");
                bail!(
                    "{}",
                    format_timeout("process", &command_line, cwd_display.as_deref(), limit_ms)
                );
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    } else {
        child.wait()?
    };

    join_output_pump(Some(output_reader), "stdout")?;

    let stdout_bytes = read_output_buffer(&stdout_buf, true)?;
    let stdout_text = if capture_output {
        String::from_utf8_lossy(&stdout_bytes).to_string()
    } else {
        String::new()
    };
    let stderr_text = String::new();
    let status = exit_status.code().unwrap_or(1);

    if let Some(path) = opts.stdout_path {
        std::fs::write(&path, &stdout_bytes)
            .context(format_context!("Failed to write stdout to file: {path}"))?;
    }

    if let Some(path) = opts.stderr_path {
        std::fs::write(&path, &stderr_text)
            .context(format_context!("Failed to write stderr to file: {path}"))?;
    }

    if opts.check.unwrap_or(false) && status != 0 {
        bail!(
            "{}",
            format_failure(
                "process",
                &command_line,
                cwd_display.as_deref(),
                status,
                &stderr_text,
            )
        );
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
            .map_err(|err| format_error!("while parsing options for exec because {err:?}"))?;

        if exec.pty.unwrap_or(false) {
            let _started = Instant::now();
            let stdout_buf = Arc::new(Mutex::new(Vec::new()));
            let (mut child, reader, mut stdin_writer) = spawn_pty_process(
                &exec.command,
                exec.args.clone(),
                exec.env.clone(),
                exec.working_directory.clone(),
                exec.stdin.clone(),
            )?;

            if let Some(input) = exec.stdin
                && let Some(mut writer) = stdin_writer.take()
            {
                writer
                    .write_all(input.as_bytes())
                    .context(format_context!("Failed to write to stdin"))?;
            }

            let output_reader = spawn_pty_output_pump(
                reader,
                Arc::clone(&stdout_buf),
                DEFAULT_SPAWN_OUTPUT_BUFFER_LIMIT_BYTES,
                false,
                true,
                None,
            );

            let exit_status = child.wait()?;
            join_output_pump(Some(output_reader), "stdout")?;
            let stdout_bytes = read_output_buffer(&stdout_buf, true)?;
            let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
            let status = exit_status.code().unwrap_or(1);

            let mut result_map = serde_json::Map::new();
            result_map.insert(
                "status".to_string(),
                serde_json::Value::Number(status.into()),
            );
            result_map.insert("stdout".to_string(), serde_json::Value::String(stdout));
            result_map.insert(
                "stderr".to_string(),
                serde_json::Value::String(String::new()),
            );
            return Ok(heap.alloc(serde_json::Value::Object(result_map)));
        }

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
                let child_stdin = child.stdin.as_mut().ok_or_else(|| {
                    format_error!(
                        "while writing stdin for exec because stdin pipe was not available"
                    )
                })?;
                child_stdin.write_all(stdin.as_bytes()).map_err(|err| {
                    format_error!("while writing to stdin for exec because {err:?}")
                })?;
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
            Err(child_result.unwrap_err()).map_err(|err| {
                format_error!("while spawning child process {invoke_command} because {err:?}")
            })
        }
    }

    /// Streaming-capable run with explicit redirection and timeout/check behavior.
    ///
    /// # New Parameters (optional)
    ///
    /// * `stdout_path` – If set, writes the command's captured stdout to this file path
    ///   (creating/truncating the file). The returned `stdout` field still contains the
    ///   captured string.
    /// * `stderr_path` – If set, writes the command's captured stderr to this file path
    ///   (creating/truncating the file). The returned `stderr` field still contains the
    ///   captured string.
    /// * `tee` – When `True`, also forwards stdout/stderr to the calling process's
    ///   stdout/stderr after capturing.
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
            return Ok(heap
                .alloc(serde_json::to_value(outcome).map_err(|err| {
                    format_error!("while serializing run result because {err:?}")
                })?));
        }
        let heap = eval.heap();

        let opts: RunOptions = serde_json::from_value(options.to_json_value()?)
            .map_err(|err| format_error!("while parsing options for run because {err:?}"))?;
        let outcome = execute_run(opts)?;
        Ok(heap.alloc(
            serde_json::to_value(outcome)
                .map_err(|err| format_error!("while serializing run result because {err:?}"))?,
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
            return Ok(heap.alloc(serde_json::to_value(outcome).map_err(|err| {
                format_error!("while serializing pipeline result because {err:?}")
            })?));
        }
        let heap = eval.heap();

        let chain: Vec<RunOptions> = serde_json::from_value(steps.to_json_value()?)
            .map_err(|err| format_error!("while parsing options for pipeline because {err:?}"))?;

        let outcome = pipeline_execute(chain)?;
        Ok(heap.alloc(
            serde_json::to_value(outcome).map_err(|err| {
                format_error!("while serializing pipeline result because {err:?}")
            })?,
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
            .map_err(|err| format_error!("while parsing argv for capture because {err:?}"))?;

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
            stdout_path: None,
            stderr_path: None,
            tee: None,
            allow_orphans: None,
            output_buffer_limit_bytes: None,
            pty: None,
        })?;

        Ok(outcome.stdout.trim().to_string())
    }

    /// Spawn a background process and return an opaque numeric handle.
    ///
    /// By default (`allow_orphans` omitted/false), spawned processes are
    /// terminated automatically when the parent program exits. Set
    /// `allow_orphans` to true to opt out.
    ///
    /// Captured output for spawned processes is buffered in-memory per stream
    /// and is bounded to 1 MiB by default. Override with
    /// `output_buffer_limit_bytes` in options (`0` disables in-memory buffering).
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
            .map_err(|err| format_error!("while parsing options for spawn because {err:?}"))?;

        ensure_exit_cleanup_registered()?;
        let allow_orphans = opts.allow_orphans.unwrap_or(false);
        let tee = opts.tee.unwrap_or(false);
        let output_buffer_limit_bytes = opts
            .output_buffer_limit_bytes
            .map(usize::try_from)
            .transpose()
            .map_err(|_| {
                anyhow::anyhow!("output_buffer_limit_bytes is too large for this platform's usize")
            })?
            .unwrap_or(DEFAULT_SPAWN_OUTPUT_BUFFER_LIMIT_BYTES);

        if opts.pty.unwrap_or(false) {
            let stdout_spec = opts
                .stdout
                .clone()
                .unwrap_or_else(|| StdoutSpec::Mode("inherit".to_string()));
            let stderr_spec = opts
                .stderr
                .clone()
                .unwrap_or_else(|| StderrSpec::Mode("inherit".to_string()));
            let mut _capture_output = matches!(
                stdout_spec,
                StdoutSpec::Mode(ref mode) if mode == "capture"
            );
            let mut output_file = None;
            let mut tee_to_stdout = tee;
            let mut tee_to_stderr = false;

            match stdout_spec {
                StdoutSpec::Mode(mode) => match mode.as_str() {
                    "inherit" => {
                        tee_to_stdout = true;
                        _capture_output = true;
                    }
                    "capture" => {
                        _capture_output = true;
                    }
                    "null" => {
                        _capture_output = false;
                    }
                    other => bail!("invalid stdout mode: {other}"),
                },
                StdoutSpec::File { file } => {
                    let file_handle = std::fs::File::create(&file).map_err(|err| {
                        format_error!("while opening stdout file {file} for spawn because {err:?}")
                    })?;
                    output_file = Some(file_handle);
                    _capture_output = true;
                }
            }

            match stderr_spec {
                StderrSpec::Mode(mode) => match mode.as_str() {
                    "inherit" => {
                        tee_to_stderr = true;
                    }
                    "capture" | "merge" => {
                        _capture_output = true;
                    }
                    "null" => {}
                    other => bail!("invalid stderr mode: {other}"),
                },
                StderrSpec::File { file } => {
                    let file_handle = std::fs::File::create(&file).map_err(|err| {
                        format_error!("while opening stderr file {file} for spawn because {err:?}")
                    })?;
                    output_file = Some(file_handle);
                    _capture_output = true;
                }
            }

            let stdout_buf = Arc::new(Mutex::new(Vec::new()));
            let (child, reader, mut stdin_writer) = spawn_pty_process(
                &opts.command,
                opts.args,
                opts.env,
                opts.cwd,
                opts.stdin.clone(),
            )?;

            if let Some(input) = opts.stdin
                && let Some(mut writer) = stdin_writer.take()
            {
                writer.write_all(input.as_bytes()).map_err(|err| {
                    format_error!("while writing to stdin for spawn because {err:?}")
                })?;
            }

            let stdout_reader = Some(spawn_pty_output_pump(
                reader,
                Arc::clone(&stdout_buf),
                output_buffer_limit_bytes,
                tee || tee_to_stdout || tee_to_stderr,
                tee_to_stdout || (tee && !tee_to_stderr),
                output_file,
            ));

            let handle = NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed);
            let mut registry = process_registry()
                .lock()
                .map_err(|_| anyhow::anyhow!("process registry lock poisoned"))?;
            registry.insert(
                handle,
                ChildHandle {
                    child,
                    started: Instant::now(),
                    merge_stderr: false,
                    allow_orphans,
                    exit_status: None,
                    stdout_buf,
                    stderr_buf: Arc::new(Mutex::new(Vec::new())),
                    stdout_reader,
                    stderr_reader: None,
                },
            );

            return Ok(handle);
        }

        let (mut cmd, stdin_payload) = build_command(
            &opts.command,
            opts.args,
            opts.env,
            opts.cwd,
            opts.stdin.clone(),
        )?;

        // For background jobs: default to inheriting stdout/stderr unless explicitly configured.
        let mut stdout_file: Option<std::fs::File> = None;
        let mut pipe_stdout = false;
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
                    pipe_stdout = true;
                }
                "null" => {
                    cmd.stdout(Stdio::null());
                }
                other => bail!("invalid stdout mode: {other}"),
            },
            StdoutSpec::File { file } => {
                let file_handle = std::fs::File::create(&file).map_err(|err| {
                    format_error!("while opening stdout file {file} for spawn because {err:?}")
                })?;
                let dup = file_handle.try_clone().map_err(|err| {
                    format_error!(
                        "while cloning stdout file handle {file} for spawn because {err:?}"
                    )
                })?;
                cmd.stdout(Stdio::from(file_handle));
                stdout_file = Some(dup);
            }
        }

        // DEFECT 1 FIX: Added StderrSpec::File arm.
        // DEFECT 5 FIX: "merge" now uses Stdio::piped() so stderr output is captured and
        // can be appended to stdout in wait(). Previously it used Stdio::inherit() which
        // sent stderr to the terminal instead of into the capture buffer.
        let mut merge_stderr = false;
        let mut pipe_stderr = false;
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
                    pipe_stderr = true;
                }
                "null" => {
                    cmd.stderr(Stdio::null());
                }
                "merge" => {
                    if let Some(file) = stdout_file.take() {
                        // stdout is a file: send stderr to the same file (2>&1).
                        cmd.stderr(Stdio::from(file));
                    } else {
                        // Pipe stderr so wait()/read_lines() can consume and append it to stdout.
                        cmd.stderr(Stdio::piped());
                        merge_stderr = true;
                        pipe_stderr = true;
                    }
                }
                other => bail!("invalid stderr mode: {other}"),
            },
            StderrSpec::File { file } => {
                let file_handle = std::fs::File::create(&file).map_err(|err| {
                    format_error!("while opening stderr file {file} for spawn because {err:?}")
                })?;
                cmd.stderr(Stdio::from(file_handle));
            }
        }

        let mut child = cmd.spawn().map_err(|err| {
            format_error!(
                "while spawning child process {} for spawn because {err:?}",
                opts.command
            )
        })?;

        // DEFECT 4 FIX: Use take() so child_stdin is dropped immediately after write_all(),
        // sending EOF to the spawned process. Without this, the process never gets EOF on stdin.
        if let Some(input) = stdin_payload
            && let Some(mut child_stdin) = child.stdin.take()
        {
            child_stdin
                .write_all(input.as_bytes())
                .map_err(|err| format_error!("while writing to stdin for spawn because {err:?}"))?;
            // child_stdin dropped here → EOF sent to child
        }

        let stdout_buf = Arc::new(Mutex::new(Vec::new()));
        let stderr_buf = Arc::new(Mutex::new(Vec::new()));

        let stdout_reader = if pipe_stdout {
            let Some(stdout) = child.stdout.take() else {
                let _ = child.kill();
                let _ = child.wait();
                bail!("stdout pipe was not available for spawn capture")
            };
            Some(spawn_output_pump(
                stdout,
                Arc::clone(&stdout_buf),
                output_buffer_limit_bytes,
                tee,
                true,
            ))
        } else {
            None
        };

        let stderr_reader = if pipe_stderr {
            let Some(stderr) = child.stderr.take() else {
                let _ = child.kill();
                let _ = child.wait();
                bail!("stderr pipe was not available for spawn capture")
            };
            let tee_to_stdout = merge_stderr;
            Some(spawn_output_pump(
                stderr,
                Arc::clone(&stderr_buf),
                output_buffer_limit_bytes,
                tee,
                tee_to_stdout,
            ))
        } else {
            None
        };

        let handle = NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed);
        let mut registry = process_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("process registry lock poisoned"))?;
        registry.insert(
            handle,
            ChildHandle {
                child: ManagedChild::Std(child),
                started: Instant::now(),
                merge_stderr,
                allow_orphans,
                exit_status: None,
                stdout_buf,
                stderr_buf,
                stdout_reader,
                stderr_reader,
            },
        );

        Ok(handle)
    }

    /// Read currently available captured output lines for a running background process.
    ///
    /// `stream` must be either "stdout" or "stderr" and determines which captured
    /// stream is read.
    ///
    /// Returns only complete lines (newline-terminated). Any trailing partial line
    /// remains buffered for the next call.
    ///
    /// By default (`drain` omitted/true), returned complete lines are consumed from the
    /// selected stream's internal buffer. Set `drain` to false to snapshot complete
    /// lines without consuming.
    ///
    /// Set `max_lines` to limit returned complete lines from the selected stream.
    /// If omitted, all currently available complete lines are returned.
    fn read_lines(
        handle: u64,
        stream: &str,
        drain: Option<bool>,
        max_lines: Option<u64>,
    ) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let drain = drain.unwrap_or(true);
        let max_lines = max_lines
            .map(usize::try_from)
            .transpose()
            .map_err(|_| anyhow::anyhow!("max_lines is too large for this platform's usize"))?;

        let mut registry = process_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("process registry lock poisoned"))?;

        let Some(entry) = registry.get_mut(&handle) else {
            bail!("unknown process handle: {handle}");
        };

        match stream {
            "stdout" => {
                let mut stdout_lines = read_output_lines(&entry.stdout_buf, drain, max_lines)?;
                if entry.merge_stderr {
                    let stderr_lines = read_output_lines(&entry.stderr_buf, drain, max_lines)?;
                    stdout_lines.extend(stderr_lines);
                }
                Ok(stdout_lines)
            }
            "stderr" => {
                if entry.merge_stderr {
                    Ok(Vec::new())
                } else {
                    read_output_lines(&entry.stderr_buf, drain, max_lines)
                }
            }
            _ => bail!("invalid stream: {stream}; expected \"stdout\" or \"stderr\""),
        }
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

        if entry.exit_status.is_some() {
            return Ok(false);
        }

        match entry.child.try_wait()? {
            None => Ok(true),
            Some(status) => {
                // Cache completion so subsequent is_running() calls stay false,
                // and finish output pumps so trailing bytes are readable via read_lines().
                entry.exit_status = Some(status);
                join_output_pump(entry.stdout_reader.take(), "stdout")?;
                join_output_pump(entry.stderr_reader.take(), "stderr")?;
                Ok(false)
            }
        }
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
                    let Some(pid) = entry.child.id() else {
                        bail!("process id unavailable for child")
                    };
                    let pid = pid as libc::pid_t;
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
        drop(registry);

        let started_poll = Instant::now();
        let exit_status = if let Some(status) = entry.exit_status.take() {
            status
        } else if let Some(limit_ms) = timeout_ms {
            loop {
                if let Some(status) = entry.child.try_wait()? {
                    break status;
                }

                // DEFECT 6 FIX: Kill the child before bailing instead of putting it back
                // in the registry. The handle is consumed on timeout; leaving the child
                // running indefinitely was incorrect.
                if started_poll.elapsed().as_millis() as u64 >= limit_ms {
                    let _ = entry.child.kill();
                    let _ = entry.child.wait();
                    let _ = join_output_pump(entry.stdout_reader.take(), "stdout");
                    let _ = join_output_pump(entry.stderr_reader.take(), "stderr");
                    bail!("wait timed out after {limit_ms}ms");
                }

                std::thread::sleep(Duration::from_millis(10));
            }
        } else {
            entry.child.wait()?
        };

        // Capture duration and merge flag before reading final buffers.
        let merge_stderr = entry.merge_stderr;
        let duration_ms = entry.started.elapsed().as_millis() as i64;

        join_output_pump(entry.stdout_reader.take(), "stdout")?;
        join_output_pump(entry.stderr_reader.take(), "stderr")?;

        let mut stdout_bytes = read_output_buffer(&entry.stdout_buf, true)?;
        let stderr_bytes = read_output_buffer(&entry.stderr_buf, true)?;

        let (stdout_text, stderr_text) = if merge_stderr {
            stdout_bytes.extend_from_slice(&stderr_bytes);
            (
                String::from_utf8_lossy(&stdout_bytes).to_string(),
                String::new(),
            )
        } else {
            (
                String::from_utf8_lossy(&stdout_bytes).to_string(),
                String::from_utf8_lossy(&stderr_bytes).to_string(),
            )
        };

        let status = exit_status.code().unwrap_or(1);

        let result = serde_json::json!({
            "status": status,
            "stdout": stdout_text,
            "stderr": stderr_text,
            "duration_ms": duration_ms,
        });

        Ok(heap.alloc(result))
    }
}
