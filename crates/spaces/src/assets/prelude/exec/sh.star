"""
Spaces Shell (sh) Module

This module provides ergonomic wrappers around the built-in ``sh`` namespace,
which executes command strings through the **platform shell**:

- Unix    : ``/bin/sh -c <command>``
- Windows : ``cmd.exe /C <command>``

Because the command is interpreted by a shell, the full shell syntax is
available: pipes (``|``), redirections (``>``, ``2>&1``), semicolons,
environment-variable expansion, globs, and so on.

--------------------------------------------------------------------------------
Choosing between ``sh`` and ``process``
--------------------------------------------------------------------------------

+------------------------------------------+------------------+------------------+
| Concern                                  | Use ``sh``       | Use ``process``  |
+==========================================+==================+==================+
| Need pipes, globs, redirects             | ✓                |                  |
+------------------------------------------+------------------+------------------+
| Need a timeout                           |                  | ✓  (``run``)     |
+------------------------------------------+------------------+------------------+
| Need to set environment variables        |                  | ✓  (``exec``/    |
|                                          |                  | ``run``)         |
+------------------------------------------+------------------+------------------+
| Need to supply stdin                     |                  | ✓  (``exec``/    |
|                                          |                  | ``run``)         |
+------------------------------------------+------------------+------------------+
| Need async / background execution        |                  | ✓  (``spawn``)   |
+------------------------------------------+------------------+------------------+
| Avoid shell-injection risk               |                  | ✓  (argv list)   |
+------------------------------------------+------------------+------------------+
| Quick one-liner or shell pipeline        | ✓                |                  |
+------------------------------------------+------------------+------------------+

``process`` functions accept an explicit argv list and never invoke a shell, so
they are immune to shell injection.  ``sh`` trades that safety guarantee for the
convenience of full shell syntax.

--------------------------------------------------------------------------------
⚠  Shell-Injection Hazard
--------------------------------------------------------------------------------

The ``command`` string is forwarded **as-is** to the shell.
**Never interpolate untrusted or externally-supplied data into the command
string.**  Doing so allows arbitrary command execution.

    # UNSAFE — user_input could be "foo; rm -rf /"
    sh_run("process " + user_input)

    # SAFE — use process_exec / process_run with an explicit argv list
    process_exec({"command": "process", "args": [user_input]})

If you must embed a dynamic value in a shell command, shell-quote it first —
for example, wrap it in single quotes and escape any embedded single quotes as
``'\''``.

--------------------------------------------------------------------------------
Quoting rules
--------------------------------------------------------------------------------

Because the command goes through the shell, normal shell quoting applies:

- **Single quotes** (``'…'``) pass every character literally — no variable
  expansion or backslash interpretation inside.
- **Double quotes** (``"…"``) allow ``$VAR`` and ``\\`` escapes but protect
  spaces.
- **Unquoted** tokens are subject to word-splitting and glob expansion.

Example — count log files whose names may contain spaces::

    result = sh_run("find . -name '*.log' | wc -l", check=True)

--------------------------------------------------------------------------------
Windows notes
--------------------------------------------------------------------------------

On Windows the underlying shell is ``cmd.exe /C``.  Several POSIX constructs
are not available or behave differently:

- Use ``%VAR%`` for environment-variable expansion (not ``$VAR``).
- The ``test`` builtin does not exist; use ``if exist <file>`` instead.
- ``true`` / ``false`` do not exist; use ``exit /b 0`` / ``exit /b 1``.
- ``2>&1`` redirection works the same way as on Unix.

For cross-platform scripts consider using ``process_exec`` with explicit
arguments, or guard shell-specific code with ``sys.platform()``.

--------------------------------------------------------------------------------
Known limitations
--------------------------------------------------------------------------------

- **No explicit ``env`` parameter** — environment variables can be set inline
  on POSIX (``FOO=bar command``) but that syntax is not portable to Windows.
  Use ``process_exec`` / ``process_run`` when you need explicit env control.
- **No ``stdin``** — use ``process_exec`` when stdin must be provided.
- **No timeout** — use ``process_run`` (with ``timeout_ms``) when you need one.

Examples::

    # Run a command and capture its output
    output = sh_capture("git rev-parse HEAD")
    print(output)  # Single trimmed line

    # Get output as individual lines
    files = sh_lines("find . -name '*.txt'")
    for f in files:
        print(f)

    # Run a command and check exit code
    status = sh_exit_code("test -f config.json")
    if status == 0:
        print("Config exists")

    # Run a shell command with full output details
    result = sh_run("npm test", check=True)
    print("Status:", result["status"])
    print("Output:", result["stdout"])
"""

# ============================================================================
# Shell Command Execution
# ============================================================================

def sh_run(command: str, check: bool = False, cwd = None) -> dict:
    """
    Run a shell command and capture its complete output and status.

    This is the most flexible shell execution function.  It runs the command
    through the platform shell (``/bin/sh -c`` on Unix, ``cmd.exe /C`` on
    Windows) and returns the exit status, stdout, and stderr as a dict.

    .. warning::
        ``command`` is passed verbatim to the shell.  **Do not interpolate
        untrusted input.**  See the module docstring for details.

    Args:
        command: Shell command string to execute.  Pipes, redirections, and
            other shell features work because the string is interpreted by the
            platform shell.
        check: If ``True``, raise an error when the command exits with a
            non-zero status.  If ``False`` (the default), return the result
            regardless of exit code.
        cwd: Optional working directory for the command.  If ``None`` (the
            default), the command runs in the current working directory.

    Returns:
        dict: A result dictionary with the following keys:

        - **status** (``int``): Exit code of the command (0 = success).
        - **stdout** (``str``): Captured standard output.
        - **stderr** (``str``): Captured standard error.

    Raises:
        Error: If ``check=True`` and the command exits with a non-zero status,
               or if the command cannot be executed.

    Note:
        ``sh_run`` defaults ``check=False`` so that callers always receive the
        full result dict and can inspect status, stdout, and stderr themselves.
        For a "fail fast" convenience call, prefer ``sh_capture`` or
        ``sh_lines`` (which default to ``check=True``).

        To merge stderr into stdout use the shell redirection ``2>&1``::

            result = sh_run("some_tool 2>&1")

        To set environment variables for the subprocess on POSIX use inline
        assignment syntax::

            result = sh_run("MY_VAR=hello sh -c 'echo $MY_VAR'")

        For portable env-var control across platforms use ``process_run``
        instead.

    Examples::

        # Simple command execution
        result = sh_run("echo 'Hello, World!'")
        print(result["status"])   # 0
        print(result["stdout"])   # "Hello, World!\\n"

        # Use shell features (pipes, globbing, etc.)
        result = sh_run("ls *.py | wc -l")
        print("Python files:", result["stdout"])

        # Run with check=True to raise on non-zero exit
        result = sh_run("cargo build", check=True)

        # Run in a specific directory
        result = sh_run("npm test", cwd="/path/to/project")
        print(result["stdout"])

        # Capture both stdout and stderr
        result = sh_run("some_command 2>&1")
        print(result["stdout"])
    """
    if cwd != None:
        return sh.run(command, check = check, cwd = cwd)
    else:
        return sh.run(command, check = check)

# ============================================================================
# Output Capture
# ============================================================================

def sh_capture(command: str, check: bool = True, cwd = None) -> str:
    """
    Run a shell command and return its trimmed stdout as a string.

    This is the most convenient function for capturing command output.  The
    output is automatically trimmed of trailing newlines and carriage returns.
    By default it will raise an error if the command fails, making it safe for
    scripts where failure should abort execution immediately.

    .. warning::
        ``command`` is passed verbatim to the shell.  **Do not interpolate
        untrusted input.**  See the module docstring for details.

    Args:
        command: Shell command string to execute.
        check: If ``True`` (the default), raise an error when the command exits
            with a non-zero status.  Set to ``False`` to ignore command
            failures and return whatever output was produced.
        cwd: Optional working directory for the command.

    Returns:
        str: The command's stdout, trimmed of trailing whitespace and newlines.

    Raises:
        Error: If ``check=True`` and the command exits with a non-zero status,
               or if the command cannot be executed.

    Examples::

        # Get the current git branch
        branch = sh_capture("git rev-parse --abbrev-ref HEAD")
        print("Current branch:", branch)

        # Get a single numeric value
        count = int(sh_capture("find . -name '*.py' | wc -l"))
        print("Found", count, "Python files")

        # Ignore errors and fall back to a default
        output = sh_capture("git rev-parse HEAD", check=False)
        if not output:
            print("Not in a git repository")

        # Capture output from a command in a specific directory
        listing = sh_capture("ls -1", cwd="/path/to/directory")
        print(listing)
    """
    if cwd != None:
        return sh.capture(command, check = check, cwd = cwd)
    else:
        return sh.capture(command, check = check)

def sh_lines(command: str, check: bool = True, cwd = None) -> list:
    """
    Run a shell command and return its output split into individual lines.

    This function runs a command and automatically splits its stdout into a
    list of strings, one per line.  A trailing empty line (the newline after
    the last output line) is **not** included in the result, making it
    convenient for iterating over command output without handling trailing
    empty strings.

    .. warning::
        ``command`` is passed verbatim to the shell.  **Do not interpolate
        untrusted input.**  See the module docstring for details.

    Args:
        command: Shell command string to execute.
        check: If ``True`` (the default), raise an error when the command exits
            with a non-zero status.  Set to ``False`` to ignore failures.
        cwd: Optional working directory for the command.

    Returns:
        list: A list of strings, one per line of output.  Returns an empty
              list if the command produces no output.

    Raises:
        Error: If ``check=True`` and the command exits with a non-zero status,
               or if the command cannot be executed.

    Note:
        Prefer ``printf`` over ``echo -e`` / ``echo -n`` in the command string.
        The behaviour of ``echo`` flags varies across POSIX shells — on macOS
        ``/bin/sh`` (bash 3.2 compiled with ``xpg_echo``), ``echo`` expands
        ``\\n`` by default and does **not** treat ``-e`` or ``-n`` as flags,
        printing them literally instead.

    Examples::

        # List files in a directory
        files = sh_lines("ls -1")
        for f in files:
            print("File:", f)

        # Get git tags
        tags = sh_lines("git tag --list 'v*'")
        latest = tags[-1] if tags else None
        print("Latest tag:", latest)

        # Filter output — safely handle commands that may produce nothing
        matching = sh_lines("grep -rl 'TODO' src/", check=False)
        if matching:
            print("TODOs found in:", matching)
        else:
            print("No TODOs")

        # Produce lines portably with printf (avoid echo -e)
        items = sh_lines("printf 'alpha\\nbeta\\ngamma\\n'")
        # items == ["alpha", "beta", "gamma"]
    """
    if cwd != None:
        return sh.lines(command, check = check, cwd = cwd)
    else:
        return sh.lines(command, check = check)

# ============================================================================
# Exit Code Checking
# ============================================================================

def sh_exit_code(command: str, cwd = None) -> int:
    """
    Run a shell command and return only its numeric exit code.

    This function is useful for conditional logic where you need to know
    whether a command succeeded or failed without capturing its output.
    Unlike the other ``sh_*`` functions, this **never raises an error** for a
    non-zero command exit status — it only fails if the process cannot be
    spawned or waited on.

    .. warning::
        ``command`` is passed verbatim to the shell.  **Do not interpolate
        untrusted input.**  See the module docstring for details.

    Args:
        command: Shell command string to execute.
        cwd: Optional working directory for the command.

    Returns:
        int: The command's exit code (0 = success, non-zero = failure).
             Returns ``1`` if the process terminates without a numeric exit
             code (e.g. killed by a signal on Unix).

    Raises:
        Error: Only if the command cannot be spawned or the process cannot be
               waited on.

    Note:
        On POSIX, ``test -f <path>`` checks whether ``<path>`` is a **regular
        file** (not a directory, symlink target that is a directory, or special
        file such as ``/dev/null``).  Use ``test -e`` to test for existence of
        any filesystem entry, or ``test -d`` for directories.

    Examples::

        # Check if a regular file exists
        status = sh_exit_code("test -f config.json")
        if status == 0:
            print("Config file exists")
        else:
            print("Config file not found")

        # Check if a command is available
        status = sh_exit_code("command -v python3")
        if status == 0:
            print("Python 3 is available")

        # Detect uncommitted changes
        status = sh_exit_code("git diff --quiet")
        if status != 0:
            print("Changes detected, rebuilding...")

        # Check for a pattern in a file
        status = sh_exit_code("grep -q 'pattern' file.txt")
        if status == 0:
            print("Pattern found")
        elif status == 1:
            print("Pattern not found")
        else:
            print("Search failed with status", status)
    """
    if cwd != None:
        return sh.exit_code(command, cwd = cwd)
    else:
        return sh.exit_code(command)

# ============================================================================
# Pipeline Execution
# ============================================================================

def sh_pipe(commands: list, check: bool = True, cwd = None) -> dict:
    """
    Run a pipeline of shell commands joined with `` | ``.

    This is a convenience wrapper that takes a list of command strings,
    joins them with `` | ``, and executes the result through the platform
    shell. It returns the same dict structure as ``sh_run``.

    .. warning::
        Each command string is passed verbatim to the shell.  **Do not
        interpolate untrusted input.**  See the module docstring for details.

    Args:
        commands: A list of shell command strings to pipe together.
        check: If ``True`` (the default), raise an error when the pipeline
            exits with a non-zero status.
        cwd: Optional working directory for the pipeline.

    Returns:
        dict: A result dictionary with the following keys:

        - **status** (``int``): Exit code of the last command in the pipeline.
        - **stdout** (``str``): Captured standard output from the pipeline.
        - **stderr** (``str``): Captured standard error from the pipeline.

    Raises:
        Error: If ``check=True`` and the pipeline exits with a non-zero status,
               or if the command list is empty, or if the pipeline cannot be
               executed.

    Examples::

        # Simple two-stage pipeline
        result = sh_pipe(["echo 'hello world'", "tr 'a-z' 'A-Z'"])
        print(result["stdout"])  # "HELLO WORLD\n"

        # Multi-stage pipeline for filtering and counting
        result = sh_pipe([
            "cat *.log",
            "grep ERROR",
            "wc -l"
        ])
        error_count = int(result["stdout"].strip())
        print("Found", error_count, "errors")

        # Run pipeline with check=False to inspect errors
        result = sh_pipe(["ls", "grep pattern"], check=False)
        if result["status"] != 0:
            print("Pipeline failed:", result["stderr"])

        # Execute pipeline in a specific directory
        result = sh_pipe(["find . -name '*.txt'", "head -5"], cwd="/tmp")
        print(result["stdout"])
    """
    if cwd != None:
        return sh.pipe(commands, check = check, cwd = cwd)
    else:
        return sh.pipe(commands, check = check)
