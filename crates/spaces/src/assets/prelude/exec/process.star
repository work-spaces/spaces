"""
Spaces Process Module

This module provides ergonomic wrappers around process execution and management.
It supports:
- Simple command execution with output capture (exec, capture)
- Advanced process control (run with redirections and timeouts)
- Background process management (spawn, is_running, kill, wait)
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

    # Background process management
    handle = process_spawn({
        "command": "long_running_task",
        "args": ["--timeout", "300"],
    })
    if process_is_running(handle):
        print("Process is still running...")
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
        stdout: str | dict | None = None,
        stderr: str | dict | None = None,
        timeout_ms: int | None = None,
        check: bool = False) -> dict:
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
        stdout: Output handling option (default: None). Use process_stdout_inherit(),
            process_stdout_capture(), process_stdout_null(), or process_stdout_file(path).
        stderr: Error handling option (default: None). Use process_stderr_inherit(),
            process_stderr_capture(), process_stderr_null(), process_stderr_merge(),
            or process_stderr_file(path).
        timeout_ms: Maximum time in milliseconds (default: None).
        check: Raise error on non-zero exit (default: False).

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

    if stdout != None:
        options["stdout"] = stdout

    if stderr != None:
        options["stderr"] = stderr

    if timeout_ms != None:
        options["timeout_ms"] = timeout_ms

    if check != False:
        options["check"] = check

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

def process_spawn(options: dict) -> int:
    """
    Spawn a background process and return an opaque handle for later management.

    This function starts a process in the background and returns a handle that can
    be used with process_is_running, process_kill, and process_wait to manage the
    process. By default, stdout and stderr are inherited (shown to the user).

    Args:
        options: Use the return value of process_options().

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

        # Later in your script, manage the process
        if process_is_running(handle):
            print("Build is still running")
        result = process_wait(handle, timeout_ms = 60000)
        print("Build completed with status:", result["status"])
    """
    return process.spawn(options)

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
            - "SIGTERM": Graceful termination (allows cleanup)
            - "SIGKILL": Hard kill (immediate termination, no cleanup)

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

    Args:
        handle: The process handle returned by process_spawn
        timeout_ms: Maximum time to wait in milliseconds (optional):
            - If None: Wait indefinitely
            - If specified: Raise error if process doesn't finish in time

    Returns:
        dict: A result dictionary with the following keys:
            - status (int): Exit code of the process (0 = success)
            - stdout (str): Captured standard output (empty if not captured)
            - stderr (str): Captured standard error (empty if not captured)
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
