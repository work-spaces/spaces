"""
Spaces Process Module

This module provides ergonomic wrappers around process execution and management.
It supports:
- Simple command execution with output capture (exec, capture)
- Advanced process control (run with redirections and timeouts)
- Background process management (spawn, read_lines, is_running, kill, wait)
- Process identity queries (getpid, getppid, getpgid) and signal delivery (send_signal)
- Command pipelines (execute commands in series, piping output)

All functions handle errors gracefully and provide clear feedback when something
goes wrong. Process output can be captured, inherited, discarded, or redirected
to files.

Examples:
    # Simple command execution with output capture
    result = process_exec({"command": "echo", "args": ["Hello, World!"]})
    print(result["stdout"])  # Output: "Hello, World!"

    # Run a command with custom environment
    result = process_run({
        "command": "python",
        "args": ["script.py"],
        "env": {"DEBUG": "1", "LOG_LEVEL": "info"},
    })

    # Capture output directly as a string (raises on error)
    output = process_capture(["ls", "-la"])

    # Background process management with live output draining
    handle = process_spawn({
        "command": "long_running_task",
        "args": ["--timeout", "300"],
        "stdout": process_stdout_capture(),
        "stderr": process_stderr_capture(),
    }, allow_orphans = False)
    if process_is_running(handle):
        stdout_lines = process_read_lines(handle, "stdout")  # drain=True by default
        print(stdout_lines)
    result = process_wait(handle, timeout_ms = 5000)

    # Command pipelines
    result = process_pipeline([
        {"command": "cat", "args": ["input.txt"]},
        {"command": "grep", "args": ["pattern"]},
        {"command": "wc", "args": ["-l"]},
    ])
"""

# ============================================================================
# Process Options Builder and Helpers
# ============================================================================

def process_stdout_inherit() -> str:
    """
    Stdout handling option: inherit (show process output).

    When used, the process's stdout is displayed directly to the user, not
    captured. This is typically the default for spawned processes.

    Returns:
        str: The string "inherit" for use in process options
    """
    return "inherit"

def process_stdout_capture() -> str:
    """
    Stdout handling option: capture to string.

    When used, the process's stdout is captured and returned in the result
    dictionary. This is typically the default for run operations.

    Returns:
        str: The string "capture" for use in process options
    """
    return "capture"

def process_stdout_null() -> str:
    """
    Stdout handling option: discard (send to /dev/null).

    When used, the process's stdout is discarded and not displayed or captured.

    Returns:
        str: The string "null" for use in process options
    """
    return "null"

def process_stream_stdout() -> str:
    """
    Stream identifier for standard output.

    Returns:
        str: The string "stdout" for use with process stream APIs
    """
    return "stdout"

def process_stdout_file(path: str) -> dict:
    """
    Stdout handling option: redirect to file.

    When used, the process's stdout is written to the specified file. If the
    file does not exist, it will be created. If it already exists, it will be
    overwritten.

    Args:
        path (str): The file path where stdout should be written

    Returns:
        dict: A dictionary for use in process options with the file redirect
    """
    return {"file": path}

def process_stderr_inherit() -> str:
    """
    Stderr handling option: inherit (show process errors).

    When used, the process's stderr is displayed directly to the user, not
    captured. This is typically the default for spawned processes.

    Returns:
        str: The string "inherit" for use in process options
    """
    return "inherit"

def process_stderr_capture() -> str:
    """
    Stderr handling option: capture to string.

    When used, the process's stderr is captured and returned in the result
    dictionary. This is typically the default for run operations.

    Returns:
        str: The string "capture" for use in process options
    """
    return "capture"

def process_stderr_null() -> str:
    """
    Stderr handling option: discard (send to /dev/null).

    When used, the process's stderr is discarded and not displayed or captured.

    Returns:
        str: The string "null" for use in process options
    """
    return "null"

def process_stream_stderr() -> str:
    """
    Stream identifier for standard error.

    Returns:
        str: The string "stderr" for use with process stream APIs
    """
    return "stderr"

def process_stderr_merge() -> str:
    """
    Stderr handling option: merge into stdout.

    When used, the process's stderr is merged into stdout. This is useful when
    you want to capture both output streams together.

    Returns:
        str: The string "merge" for use in process options
    """
    return "merge"

def process_stderr_file(path: str) -> dict:
    """
    Stderr handling option: redirect to file.

    When used, the process's stderr is written to the specified file. If the
    file does not exist, it will be created. If it already exists, it will be
    overwritten.

    Args:
        path (str): The file path where stderr should be written

    Returns:
        dict: A dictionary for use in process options with the file redirect
    """
    return {"file": path}

def process_options(
        command: str,
        args: list[str] | None = None,
        env: dict | None = None,
        cwd: str | None = None,
        stdin: str | None = None,
        pseudo_terminal: bool | None = None,
        stdout: str | dict | None = None,
        stderr: str | dict | None = None,
        timeout_ms: int | None = None,
        check: bool = False,
        tee: bool | None = None,
        allow_orphans: bool | None = None,
        output_buffer_limit_bytes: int | None = None,
        setsid: bool | None = None,
        setpgid: int | None = None) -> dict:
    """
    Build a typed options dictionary for process execution.

    This function assembles a complete options dictionary for passing to
    process_run(), process_spawn(), or other process execution functions.
    All options have sensible defaults and type hints.

    Args:
        command: The command to execute (required).
        args: Command line arguments (default: None).
        env: Environment variables to set (default: None).
        cwd: Working directory for the command (default: None).
        stdin: Text to write to the process's stdin (default: None).
        pseudo_terminal: Request a pseudo terminal for the child process (default: None).
            When True, the child process runs in a PTY so interactive programs can
            use terminal semantics.
        stdout: Output handling option (default: None). Use process_stdout_inherit(),
            process_stdout_capture(), process_stdout_null(), or process_stdout_file(path).
        stderr: Error handling option (default: None). Use process_stderr_inherit(),
            process_stderr_capture(), process_stderr_null(), process_stderr_merge(),
            or process_stderr_file(path).
        timeout_ms: Maximum time in milliseconds (default: None).
        check: Raise error on non-zero exit (default: False).
        tee: Mirror captured output to the parent process streams. For run(),
            output is mirrored after capture. For spawn() with capture/piped
            streams, output is mirrored live while still being buffered.
        allow_orphans: For process_spawn only. If True, allow child process to
            outlive the parent program. If omitted/False, spawned processes are
            terminated when the parent exits.
        output_buffer_limit_bytes: For process_spawn only. Per-stream cap for
            in-memory captured output buffers when using capture/merge piping.
            Default is 1048576 (1 MiB). Set to 0 to disable in-memory buffering.
        setsid: If True, call setsid() in the child before exec, creating a new
            session and detaching from the controlling terminal (Unix only).
        setpgid: If set, call setpgid(0, value) in the child before exec.
            Use 0 to create a new process group using the child's own PID.
            Any positive value joins that existing process group (Unix only).

    Returns:
        dict: A complete options dictionary for process execution

    Examples:
        # Simple command with default options
        opts = process_options("echo", args=["Hello"])
        result = process_run(opts)

        # Capture output with helpers
        opts = process_options(
            "python",
            args=["script.py"],
            stdout=process_stdout_capture(),
            stderr=process_stderr_capture(),
            check=True,
        )
        result = process_run(opts)

        # Redirect to files
        opts = process_options(
            "build.sh",
            stdout=process_stdout_file("build.log"),
            stderr=process_stderr_file("build.err"),
        )
        result = process_run(opts)

        # With timeout and environment
        opts = process_options(
            "long_task",
            env={"DEBUG": "1"},
            timeout_ms=30000,
            stderr=process_stderr_merge(),
        )
        result = process_run(opts)

        # Spawn with live tee, bounded output buffer, and orphan policy
        opts = process_options(
            "dev_server",
            stdout=process_stdout_capture(),
            stderr=process_stderr_capture(),
            tee=True,
            allow_orphans=False,
            output_buffer_limit_bytes=4 * 1024 * 1024,
        )
        handle = process_spawn(opts)
    """
    options = {"command": command}

    if args != None:
        options["args"] = args

    if env != None:
        options["env"] = env

    if cwd != None:
        options["cwd"] = cwd

    if stdin != None:
        options["stdin"] = stdin

    if pseudo_terminal != None:
        options["pseudo_terminal"] = pseudo_terminal

    if stdout != None:
        options["stdout"] = stdout

    if stderr != None:
        options["stderr"] = stderr

    if timeout_ms != None:
        options["timeout_ms"] = timeout_ms

    if check != False:
        options["check"] = check

    if tee != None:
        options["tee"] = tee

    if allow_orphans != None:
        options["allow_orphans"] = allow_orphans

    if output_buffer_limit_bytes != None:
        options["output_buffer_limit_bytes"] = output_buffer_limit_bytes

    if setsid != None:
        options["setsid"] = setsid

    if setpgid != None:
        options["setpgid"] = setpgid

    return options

# ============================================================================
# Advanced Process Execution
# ============================================================================

def process_run(options: dict) -> dict:
    """
    Run a process with advanced control over I/O redirection and timeouts.

    This function provides fine-grained control over process execution, including
    separate stdout/stderr handling, file redirection, timeout support, and
    optional exit code checking.

    Args:
        options: Use the return value of process_options().

    Returns:
        dict: A result dictionary with the following keys:
            - status (int): Exit code of the process
            - stdout (str): Captured standard output (if applicable)
            - stderr (str): Captured standard error (if applicable)
            - duration_ms (int): Time taken in milliseconds

    Raises:
        Error: If the command cannot be spawned, times out, or if check=true
               and the process exits with non-zero status

    Examples:
        # Capture both stdout and stderr
        result = process_run({
            "command": "python",
            "args": ["script.py"],
            "stdout": "capture",
            "stderr": "capture",
        })

        # Redirect to files
        result = process_run({
            "command": "build.sh",
            "stdout": {"file": "build.log"},
            "stderr": {"file": "build.err"},
        })

        # Run with timeout
        result = process_run({
            "command": "slow_operation",
            "timeout_ms": 30000,  # 30 seconds
        })

        # Run and raise on failure
        result = process_run({
            "command": "validate",
            "check": True,
        })
    """
    return process.run(options)

# ============================================================================
# Simple Output Capture
# ============================================================================

def process_capture(argv: list) -> str:
    """
    Run a command and return its trimmed stdout, raising on non-zero exit.

    This is a convenience function for the common case where you want to capture
    command output and ensure the command succeeded. The output is automatically
    trimmed of leading and trailing whitespace.

    Args:
        argv: List of command and arguments:
            - argv[0]: Command to execute
            - argv[1:]: Command arguments

    Returns:
        str: The process's stdout, trimmed of whitespace

    Raises:
        Error: If argv is empty, or if the command:
            - Cannot be spawned
            - Exits with non-zero status
            - Has stderr output (stderr is captured separately)

    Examples:
        # Get output from a simple command
        files = process_capture(["ls", "-1"])
        for line in files.split("\\n"):
            print(line)

        # Get the result of a calculation
        result = process_capture(["python", "-c", "print(2 + 2)"])
        print(result)  # "4"

        # This will raise an error if the command fails
        output = process_capture(["invalid_command"])  # Error!
    """
    return process.capture(argv)

# ============================================================================
# Background Process Management
# ============================================================================

def process_spawn(options: dict, allow_orphans: bool | None = None) -> int:
    """
    Spawn a background process and return an opaque handle for later management.

    This function starts a process in the background and returns a handle that can
    be used with process_read_lines, process_is_running, process_kill, and
    process_wait to manage the process. By default, stdout and stderr are
    inherited (shown to the user).

    Unless `allow_orphans=True` is provided (either in options or via the named
    function argument), spawned processes are automatically terminated when the
    parent Spaces process exits.

    Captured output buffers for spawned processes are bounded in-memory to 1 MiB
    per stream by default. Override with `output_buffer_limit_bytes` in options,
    or set it to 0 to disable in-memory buffering while still allowing tee/file
    output modes.

    Args:
        options: Use the return value of process_options().
        allow_orphans: Optional override for `options["allow_orphans"]`.

    Returns:
        int: An opaque process handle for use with other process functions

    Raises:
        Error: If the command cannot be spawned

    Examples:
        # Start a background server
        handle = process_spawn({
            "command": "server",
            "args": ["--port", "8080"],
        })

        # Start with output redirection
        handle = process_spawn({
            "command": "build.sh",
            "stdout": {"file": "build.log"},
            "stderr": {"file": "build.err"},
        })

        # Explicit orphan policy via function argument
        handle = process_spawn(
            {
                "command": "daemon",
                "stdout": process_stdout_capture(),
                "stderr": process_stderr_capture(),
            },
            allow_orphans = True,
        )

        # Later in your script, manage the process
        if process_is_running(handle):
            print("Build is still running")
        result = process_wait(handle, timeout_ms = 60000)
        print("Build completed with status:", result["status"])
    """
    opts = dict(options)
    if allow_orphans != None:
        opts["allow_orphans"] = allow_orphans
    return process.spawn(opts)

def process_read_lines(
        handle: int,
        stream: str,
        drain: bool = True,
        max_lines: int | None = None) -> list[str]:
    """
    Read currently available captured output lines from a spawned background process.

    Only complete newline-terminated lines are returned. Any trailing partial
    line remains buffered for the next call.

    Args:
        handle: The process handle returned by process_spawn.
        stream: Which stream to read. Must be "stdout" or "stderr".
        drain: Whether to consume returned lines from internal buffer
            (default: True).
            - True: Return and remove currently available complete lines.
            - False: Return a snapshot without consuming.
        max_lines: Optional cap on returned lines (default: None).
            - None: Return all currently available complete lines.
            - N: Return at most N complete lines.

    Returns:
        list[str]: Complete lines from the selected stream.

    Raises:
        Error: If the handle is invalid or output buffers cannot be accessed

    Notes:
        - When stderr mode is "merge" (non-file), merged stderr output is read
          through the "stdout" stream.
        - Since drain defaults to True, repeated calls typically return only new
          complete lines since the previous read.

    Examples:
        handle = process_spawn({
            "command": "long_task",
            "stdout": process_stdout_capture(),
            "stderr": process_stderr_capture(),
            "tee": True,
        })

        while process_is_running(handle):
            stdout_chunk = process_read_lines(handle, "stdout", max_lines = 50)
            for line in stdout_chunk:
                print(line)

        # Non-destructive snapshot
        snapshot = process_read_lines(handle, "stdout", drain = False)
    """
    if max_lines != None:
        return process.read_lines(handle, stream, drain, max_lines)
    if drain == True:
        return process.read_lines(handle, stream)
    return process.read_lines(handle, stream, drain)

def process_is_running(handle: int) -> bool:
    """
    Check if a background process is still running.

    Args:
        handle: The process handle returned by process_spawn

    Returns:
        bool: True if the process is still running, False if it has finished

    Raises:
        Error: If the handle is invalid or the process registry is corrupted

    Examples:
        handle = process_spawn({"command": "long_task"})
        while process_is_running(handle):
            print("Still running...")
            time.sleep(1)
        print("Process finished")
    """
    return process.is_running(handle)

def process_kill(handle: int, signal: str = "SIGTERM") -> bool:
    """
    Send a signal to a background process to terminate it.

    Args:
        handle: The process handle returned by process_spawn
        signal: The signal to send (default: "SIGTERM"):
            - "SIGHUP": Hang-up (reload / terminal close)
            - "SIGINT": Interrupt (equivalent to Ctrl-C)
            - "SIGTERM": Graceful termination (allows cleanup)
            - "SIGKILL": Hard kill (immediate termination, no cleanup)
            - "SIGUSR1": User-defined signal 1
            - "SIGUSR2": User-defined signal 2
            - "SIGSTOP": Suspend process execution
            - "SIGCONT": Resume a stopped process

    Returns:
        bool: True if the signal was sent successfully

    Raises:
        Error: If the handle is invalid, signal is unsupported, or killing fails

    Examples:
        # Gracefully stop a process
        handle = process_spawn({"command": "server"})
        if process_is_running(handle):
            process_kill(handle, "SIGTERM")
            print("Termination signal sent")

        # Forcefully kill a process
        if process_is_running(handle):
            process_kill(handle, "SIGKILL")
            print("Process killed forcefully")
    """
    return process.kill(handle, signal)

def process_wait(handle: int, timeout_ms = None) -> dict:
    """
    Wait for a background process to complete and return its result.

    This function blocks until the process finishes, either due to completion
    or timeout. Once a process is waited on, the handle is consumed and cannot
    be used again. The returned dictionary includes exit status, output (if
    captured), and execution duration.

    If process_read_lines() was called earlier with drain=True (the default),
    already-consumed bytes will not appear again in this final wait result.

    Args:
        handle: The process handle returned by process_spawn
        timeout_ms: Maximum time to wait in milliseconds (optional):
            - If None: Wait indefinitely
            - If specified: Raise error if process doesn't finish in time

    Returns:
        dict: A result dictionary with the following keys:
            - status (int): Exit code of the process (0 = success)
            - stdout (str): Remaining captured stdout at wait time
            - stderr (str): Remaining captured stderr at wait time
            - duration_ms (int): Time from spawn to completion in milliseconds

    Raises:
        Error: If the handle is invalid, process doesn't finish within timeout,
               or waiting fails

    Examples:
        # Wait indefinitely for a process
        handle = process_spawn({"command": "task"})
        result = process_wait(handle)
        print("Status:", result["status"])

        # Wait with timeout
        handle = process_spawn({"command": "slow_operation"})
        result = process_wait(handle, timeout_ms = 10000)  # 10 seconds

        # If timeout is exceeded:
        result = process_wait(handle, timeout_ms = 1)  # Will likely timeout
        # Error: wait timed out after 1ms
    """
    if timeout_ms != None:
        return process.wait(handle, timeout_ms)
    return process.wait(handle)

# ============================================================================
# Process Identity and Signals
# ============================================================================

def process_getpid() -> int:
    """
    Return the PID of the current (Spaces) process.

    Returns:
        int: The current process ID.

    Examples:
        pid = process_getpid()
        print("Running as PID", pid)
    """
    return process.getpid()

def process_getppid() -> int:
    """
    Return the parent process ID (PPID) of the current process.

    On non-Unix platforms this returns 0.

    Returns:
        int: The parent process ID.

    Examples:
        ppid = process_getppid()
        print("Parent PID:", ppid)
    """
    return process.getppid()

def process_getpgid() -> int:
    """
    Return the process group ID (PGID) of the current process.

    On non-Unix platforms this returns 0.

    Returns:
        int: The current process group ID.

    Examples:
        pgid = process_getpgid()
        print("Process group:", pgid)
    """
    return process.getpgid()

def process_get_spawned_pid(handle: int) -> int:
    """
    Return the OS process ID (PID) of a spawned background process.

    This is useful for passing to process_send_signal() when you need to
    target the child by raw PID, or for querying OS-level process information.

    Args:
        handle: The process handle returned by process_spawn.

    Returns:
        int: The OS PID of the spawned process.

    Raises:
        Error: If the handle is invalid or the PID is unavailable
            (some PTY implementations may not expose a PID).

    Examples:
        handle = process_spawn({"command": "server", "args": ["--port", "8080"]})
        pid = process_get_spawned_pid(handle)
        print("Server PID:", pid)

        # Send a reload signal directly by PID
        process_send_signal(pid, "SIGHUP")
    """
    return process.get_spawned_pid(handle)

def process_get_spawned_pgid(handle: int) -> int:
    """
    Return the process group ID (PGID) of a spawned background process.

    When the process was started with setpgid=0 in its options it will have
    its own process group; this function retrieves that PGID so you can
    send signals to the whole group via process_send_signal().

    On non-Unix platforms this returns 0.

    Args:
        handle: The process handle returned by process_spawn.

    Returns:
        int: The PGID of the spawned process, or 0 on non-Unix platforms.

    Raises:
        Error: If the handle is invalid or the PGID cannot be queried.

    Examples:
        handle = process_spawn(
            process_options("make", args=["-j4"], setpgid=0)
        )
        pgid = process_get_spawned_pgid(handle)
        # Kill the entire process group (make + all sub-processes)
        process_send_signal(pgid, "SIGKILL")
    """
    return process.get_spawned_pgid(handle)

def process_send_signal(pid: int, signal: str = "SIGTERM") -> bool:
    """
    Send a signal to an arbitrary process by PID.

    This function sends a POSIX signal to any process the caller has
    permission to signal. Use process_kill() to signal a process that was
    started with process_spawn().

    Args:
        pid: The target process ID.
        signal: The signal name (default: "SIGTERM"):
            - "SIGHUP": Hang-up (reload / terminal close)
            - "SIGINT": Interrupt (equivalent to Ctrl-C)
            - "SIGTERM": Graceful termination (allows cleanup)
            - "SIGKILL": Hard kill (immediate termination, no cleanup)
            - "SIGUSR1": User-defined signal 1
            - "SIGUSR2": User-defined signal 2
            - "SIGSTOP": Suspend process execution
            - "SIGCONT": Resume a stopped process

    Returns:
        bool: True if the signal was delivered, False on non-Unix platforms.

    Raises:
        Error: If the signal name is unsupported or the kill(2) system call fails.

    Examples:
        # Reload a daemon by PID
        pid = int(process_capture(["cat", "/var/run/myservice.pid"]))
        process_send_signal(pid, "SIGHUP")

        # Suspend and later resume a process
        process_send_signal(pid, "SIGSTOP")
        # ... do something ...
        process_send_signal(pid, "SIGCONT")
    """
    return process.send_signal(pid, signal)

# ============================================================================
# Command Pipelines
# ============================================================================

def process_pipeline(steps: list[dict]) -> dict:
    """
    Execute a series of commands in a pipeline, piping output between them.

    This function runs multiple commands in sequence, with the stdout of each
    command fed as stdin to the next. This is similar to shell pipeline syntax
    like `cmd1 | cmd2 | cmd3`. The final result contains the exit status of
    the last command and its output.

    Args:
        steps: Use a list of return values from process_options().

    Returns:
        dict: A result dictionary with the following keys:
            - status (int): Exit code of the final command
            - stdout (str): Output from the final command
            - stderr (str): Captured errors (implementation-dependent)
            - duration_ms (int): Total execution time in milliseconds

    Raises:
        Error: If any command cannot be spawned, if a command times out,
               or if check=true and any command exits with non-zero status

    Examples:
        # Filter and count lines
        result = process_pipeline([
            {"command": "cat", "args": ["large_file.txt"]},
            {"command": "grep", "args": ["pattern"]},
            {"command": "wc", "args": ["-l"]},
        ])
        count = int(result["stdout"].strip())
        print(f"Found {count} matching lines")

        # Process build artifacts
        result = process_pipeline([
            {"command": "find", "args": ["build", "-name", "*.o"]},
            {"command": "wc", "args": ["-l"]},
        ])

        # Complex pipeline with environment
        result = process_pipeline([
            {
                "command": "git",
                "args": ["log", "--oneline"],
                "cwd": "/path/to/repo",
            },
            {
                "command": "head",
                "args": ["-20"],
            },
        ])
    """
    return process.pipeline(steps)
