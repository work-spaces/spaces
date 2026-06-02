"""
Environment and PATH manipulation builtins.

This module provides ergonomic access to environment variables, working directory
management, and PATH utilities for discovering and manipulating executables on the
system.

Example:
    # Get environment variables
    path = env_get("PATH")        # -> str or None (None when not set)
    home = env_get("HOME", default="/tmp")  # -> str with fallback (never None)

    # Distinguish "not set" from "set to empty string"
    env_get("MY_VAR")          # -> ""    (variable is set, value is empty)
    env_get("NOT_SET_VAR")     # -> None  (variable is absent)

    # Work with directories
    original = env_cwd()      # Get current directory
    env_chdir("/tmp")         # Change to /tmp
    env_chdir(original)       # Change back

    # Manipulate PATH
    paths = env_path_list()                        # Split PATH into a list

    # Find executables
    git_path = env_which("git")          # -> str or empty string
    python_paths = env_which_all("python")  # -> list[str]
"""

def env_get(name: str, default: str | None = None) -> str | None:
    """
    Gets an environment variable by name.

    Returns `None` when the variable is absent and no `default` was provided,
    making it possible to distinguish "variable not set" from "variable set to
    empty string". When a `default` is supplied, that value is returned instead
    of `None` for missing variables.

    Use `env_has()` as a lighter-weight existence check when the value is not
    needed.

    Args:
        name: The name of the environment variable (e.g., "PATH", "HOME", "USER")
        default: Value to return when the variable is not set. When omitted,
                 `None` is returned for missing variables.

    Returns:
        The value of the environment variable as a string, the default value if
        the variable is not set and a default was given, or None if the variable
        is not set and no default was supplied.

    Raises:
        Error: If the environment variable contains invalid UTF-8 (rare)

    Examples:
        >>> env_get("HOME")                      # -> "/home/user" (platform-dependent)
        >>> env_get("NONEXISTENT")               # -> None
        >>> env_get("NONEXISTENT", default="/tmp")  # -> "/tmp"
        >>> # Distinguish not-set from set-to-empty:
        >>> env_get("MY_VAR")          # -> ""    (set, but empty)
        >>> env_get("NOT_SET_VAR")     # -> None  (not set at all)
        >>> env_get("MY_VAR") == None  # -> False
    """
    if default != None:
        return env.get(name, default = default)
    return env.get(name)

def env_has(name: str) -> bool:
    """
    Checks whether an environment variable is present in the current process.

    Returns True if the variable exists (even if its value is an empty string).
    This is useful for checking if a variable has been explicitly set without
    caring about its value.

    Args:
        name: The name of the environment variable to check

    Returns:
        True if the variable is set in the environment, False otherwise

    Examples:
        >>> env_has("PATH")  # -> True (usually)
        >>> env_has("DEFINITELY_NOT_SET_VAR_12345")  # -> False
        >>> if env_has("CI"):
        ...     print("Running in CI environment")
        >>> if env_has("DEBUG"):
        ...     enable_debug_mode()
    """
    return env.has(name)

def env_all() -> dict[str, str]:
    """
    Returns all environment variables as a dictionary.

    Captures a snapshot of all environment variables at the time of the call.

    Non-UTF-8 keys or values are included with invalid bytes replaced by the
    Unicode replacement character (U+FFFD), consistent with the lossy conversion
    used by `env_cwd()` and `env_path_list()`.

    Returns:
        A dictionary mapping environment variable names (strings) to their values
        (strings). For example: {"PATH": "/usr/bin:...", "HOME": "/home/user", ...}

    Examples:
        >>> vars = env_all()
        >>> len(vars) > 0  # -> True
        >>> "PATH" in vars  # -> True (usually)
        >>> vars["HOME"]    # -> "/home/user" (platform-dependent)
        >>> # Find all variables starting with "RUST"
        >>> rust_vars = {k: v for k, v in env_all().items() if k.startswith("RUST")}
    """
    return env.all()

def env_cwd() -> str:
    """
    Returns the current working directory of the process.

    Returns the absolute path to the directory the process is currently
    operating in. This is affected by `env_chdir()` calls.

    Non-UTF-8 path components are replaced by the Unicode replacement character
    (U+FFFD) via lossy conversion.

    Returns:
        An absolute path string to the current working directory.

    Raises:
        Error: If the current directory cannot be determined (e.g., if it has
               been deleted)

    Examples:
        >>> cwd = env_cwd()
        >>> len(cwd) > 0        # -> True
        >>> cwd.startswith("/") # -> True (on Unix-like systems)
        >>> # Build paths relative to current directory
        >>> config_file = env_cwd() + "/config.yaml"
    """
    return env.cwd()

def env_chdir(path: str):
    """
    Changes the current working directory of the process.

    Changes the working directory for the current process and any subsequently
    spawned child processes. Does not affect parent processes.
    Use `env_cwd()` to save the current directory before changing, so it can be
    restored later.

    Args:
        path: The directory path to change to. Can be absolute or relative.
              Supports both Unix-style ("/path/to/dir") and Windows-style
              ("C:\\path\\to\\dir") paths.

    Returns:
        None

    Raises:
        Error: If the directory does not exist or is not accessible

    Examples:
        >>> original = env_cwd()
        >>> env_chdir("/tmp")
        >>> env_cwd().endswith("tmp")   # -> True (may resolve symlinks on macOS)
        >>> env_chdir(original)         # Restore original directory
        >>> env_cwd() == original       # -> True
        >>> env_chdir("subdir")         # Works with relative paths too
        >>> env_chdir("..")             # Go up one level
    """
    return env.chdir(path)

def env_path_list() -> list[str]:
    """
    Splits the PATH environment variable into a list of directory entries.

    Parses the system PATH variable and returns each directory as a separate
    element. Handles platform-specific path separators (: on Unix/macOS,
    ; on Windows). Returns an empty list if PATH is not set or is empty.

    Non-UTF-8 path components are replaced by the Unicode replacement character
    (U+FFFD) via lossy conversion.

    Returns:
        A list of directory paths in search order. Empty list if PATH is not set.

    Examples:
        >>> paths = env_path_list()
        >>> len(paths) > 0  # -> True (usually)
        >>> # Iterate through PATH directories
        >>> for directory in env_path_list():
        ...     print(f"Searching in: {directory}")
        >>> # Check if a directory is in PATH
        >>> "/usr/local/bin" in env_path_list()
    """
    return env.path_list()

def env_path_join(entries: list) -> str:
    """
    Joins a list of directory paths into a PATH-style string.

    Uses the platform separator (: on Unix/macOS, ; on Windows). This is the
    inverse of `env_path_list()`: use it to reconstruct PATH after modifying
    the list, without hard-coding platform-specific separators.

    Args:
        entries: A list of directory path strings to join.

    Returns:
        A single PATH-formatted string using the platform separator.

    Raises:
        Error: If any entry contains the platform separator character.

    Examples:
        >>> env_path_join(["/usr/bin", "/usr/local/bin"])
        "/usr/bin:/usr/local/bin"  # Unix/macOS
        >>> # Prepend a new directory to PATH:
        >>> old_paths = env_path_list()
        >>> # Remove a directory from PATH:
        >>> filtered = [p for p in env_path_list() if p != "/unwanted/dir"]
    """
    return env.path_join_entries(entries)

def env_which(name: str) -> str:
    """
    Finds the first executable matching the given name in PATH.

    Searches through all directories in the PATH environment variable in order
    for an executable file with the given name. On Windows, also checks PATHEXT
    for recognized executable extensions (e.g., .COM, .EXE, .BAT, .CMD).

    If the name contains path separators (/ or \\), it is treated as a direct
    path and checked for executability rather than searching PATH.

    Args:
        name: The name of the executable to find (e.g., "git", "python", "node")
              Can also be a relative path (e.g., "./script.sh" or "subdir/tool")

    Returns:
        The full absolute path to the executable if found, or empty string if not
        found.

    Examples:
        >>> git_path = env_which("git")
        >>> len(git_path) > 0  # -> True (if git is installed)
        >>> env_which("git").endswith("git") or env_which("git").endswith("git.exe")
        >>> env_which("definitely-not-a-real-program-12345")  # -> ""
        >>> # Use in conditionals
        >>> if env_which("cargo"):
        ...     print("Rust is installed")
    """
    return env.which(name)

def env_which_all(name: str) -> list[str]:
    """
    Finds all executables matching the given name in PATH.

    Searches through all directories in the PATH environment variable and returns
    all matching executables in PATH order. Useful for finding all versions of an
    interpreter, tool, or script. Handles platform-specific executable extensions
    and permissions. Duplicate paths are suppressed.

    On Unix-like systems, only returns files with executable permissions.
    On Windows, returns files with recognized executable extensions from PATHEXT.

    Args:
        name: The name of the executable to find (e.g., "python", "git", "node")

    Returns:
        A list of full absolute paths to all matching executables in PATH order.
        Returns empty list if no matches found.

    Examples:
        >>> pythons = env_which_all("python")
        >>> for python in pythons:
        ...     print(f"Found Python at: {python}")
        >>> env_which_all("definitely-not-real-12345")  # -> []
        >>> # The first entry matches env_which():
        >>> env_which_all("git")[0] == env_which("git")  # -> True (if git is found)
    """
    return env.which_all(name)
