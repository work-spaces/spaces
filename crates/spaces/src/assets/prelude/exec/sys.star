"""
System Information Module

This module provides ergonomic access to system information including the operating system,
CPU architecture, hostname, username, home directory, CPU count, memory information, and
system configuration details.

All functions are designed for simple, direct access to common system properties without
requiring error handling in most cases. Use these for system introspection, platform detection,
and environment-aware configurations.

Examples:
    # Detect operating system
    if sys_os() == "windows":
        print("Running on Windows")
    elif sys_os() == "macos":
        print("Running on macOS")
    else:
        print("Running on Linux")

    # Get system information for logging
    info = {
        "os": sys_os(),
        "arch": sys_arch(),
        "hostname": sys_hostname(),
        "user": sys_username(),
        "home": sys_user_home(),
        "cpu_count": sys_cpu_count(),
        "total_memory_gb": sys_total_memory_bytes() / (1024**3),
        "endianness": sys_endianness(),
    }

    # Conditional logic based on environment
    if sys_is_ci():
        print("Running in CI environment")
    else:
        print("Running locally")

    # Platform-specific configuration
    if sys_arch() in ["x86_64", "aarch64"]:
        use_optimized_binary()
"""

# ============================================================================
# Operating System and Architecture Detection
# ============================================================================

def sys_os() -> str:
    """
    Returns the current operating system name.

    Returns one of the standard OS identifiers: "linux", "macos", "windows", etc.
    Use this for platform detection and conditional logic.

    Returns:
        str: The operating system identifier ("linux", "macos", "windows", etc.)

    Examples:
        >>> os_name = sys_os()
        >>> if os_name == "windows":
        ...     print("Running on Windows")
        >>> if os_name in ["linux", "macos"]:
        ...     print("Running on Unix-like system")
        >>> print(f"OS: {sys_os()}")
    """
    return sys.os()

def sys_arch() -> str:
    """
    Returns the current CPU architecture.

    Returns the architecture identifier for the CPU (e.g., "x86_64", "aarch64", "arm", "wasm32").
    Useful for selecting architecture-specific binaries or libraries.

    Returns:
        str: The CPU architecture identifier ("x86_64", "aarch64", "arm", "wasm32", etc.)

    Examples:
        >>> arch = sys_arch()
        >>> if arch == "aarch64":
        ...     print("Running on ARM64")
        >>> if arch == "x86_64":
        ...     print("Running on x86-64")
        >>> print(f"Architecture: {sys_arch()}")
        >>> # Select appropriate binary
        >>> binary = f"app-{sys_os()}-{sys_arch()}"
    """
    return sys.arch()

# ============================================================================
# Host and User Information
# ============================================================================

def sys_hostname() -> str:
    """
    Returns the hostname of the current machine.

    Gets the hostname as reported by the operating system. Useful for logging,
    identifying which machine performed an action, or machine-specific configuration.

    Returns:
        str: The hostname of the machine

    Raises:
        Error: If hostname cannot be determined (rare, usually succeeds)

    Examples:
        >>> hostname = sys_hostname()
        >>> print(f"Running on: {hostname}")
        >>> # Identify machine in logs
        >>> log(f"Task executed on {sys_hostname()}")
        >>> # Machine-specific configuration
        >>> if sys_hostname().startswith("prod-"):
        ...     use_production_config()
    """
    return sys.hostname()

def sys_username() -> str:
    """
    Returns the current username of the process.

    Gets the username of the user running the current process. Useful for logging,
    audit trails, and user-specific configuration.

    Returns:
        str: The current username

    Examples:
        >>> user = sys_username()
        >>> print(f"Running as: {user}")
        >>> if sys_username() == "root":
        ...     print("Running with root privileges")
        >>> # User-specific paths
        >>> config = f"/home/{sys_username()}/.myapp/config"
        >>> # Logging with user context
        >>> log(f"User {sys_username()} executed action")
    """
    return sys.username()

def sys_user_home() -> str:
    """
    Returns the home directory path of the current user.

    Gets the user's home directory. This is platform-aware: /home/username on Linux,
    /Users/username on macOS, C:\\Users\\username on Windows.

    Returns:
        str: Absolute path to the user's home directory

    Raises:
        Error: If home directory cannot be determined (rare)

    Examples:
        >>> home = sys_user_home()
        >>> config_file = f"{home}/.myapp/config.yaml"
        >>> data_dir = f"{home}/.local/share/myapp"
        >>> # Create user-specific directory
        >>> cache_dir = f"{sys_user_home()}/.cache/myapp"
        >>> # Store application data
        >>> db_path = f"{sys_user_home()}/.myapp/data.db"
        >>> print(f"Home: {sys_user_home()}")
    """
    return sys.user_home()

# ============================================================================
# System Resources and Capabilities
# ============================================================================

def sys_cpu_count() -> int:
    """
    Returns the logical CPU count of the system.

    Returns the number of logical CPU cores available. This is the number of
    concurrent threads the system can run. Useful for parallelization decisions.

    Returns:
        int: The number of logical CPU cores

    Examples:
        >>> cores = sys_cpu_count()
        >>> print(f"Available CPUs: {cores}")
        >>> # Decide parallelism level
        >>> if sys_cpu_count() >= 4:
        ...     use_parallel_processing(sys_cpu_count())
        >>> else:
        ...     use_single_threaded()
        >>> # Worker pool sizing
        >>> workers = max(1, sys_cpu_count() - 1)
    """
    return sys.cpu_count()

def sys_total_memory_bytes() -> int:
    """
    Returns the total system memory in bytes.

    Gets the total amount of RAM installed on the system. Can be converted to
    other units (GB, MB) by dividing by 1024.

    Returns:
        int: Total system memory in bytes

    Examples:
        >>> memory_bytes = sys_total_memory_bytes()
        >>> memory_gb = memory_bytes / (1024 ** 3)
        >>> memory_mb = memory_bytes / (1024 ** 2)
        >>> print(f"Total memory: {memory_gb:.2f} GB")
        >>> # Size resource allocations based on available memory
        >>> if sys_total_memory_bytes() > 16 * 1024 ** 3:
        ...     enable_large_cache()
        >>> # Calculate buffer sizes
        >>> buffer_size = min(100 * 1024 ** 2, sys_total_memory_bytes() // 10)
    """
    return sys.total_memory_bytes()

def sys_total_memory_gb() -> float:
    """
    Returns the total system memory in gigabytes.

    Convenience function that returns total_memory_bytes() converted to gigabytes.

    Returns:
        float: Total system memory in gigabytes

    Examples:
        >>> memory_gb = sys_total_memory_gb()
        >>> print(f"System has {memory_gb:.1f} GB of RAM")
        >>> if sys_total_memory_gb() < 2.0:
        ...     print("Low memory system")
        >>> # Memory-aware resource allocation
        >>> if sys_total_memory_gb() >= 16.0:
        ...     load_large_dataset_into_memory()
    """
    return sys.total_memory_bytes() / (1024.0 * 1024.0 * 1024.0)

# ============================================================================
# System Configuration and Properties
# ============================================================================

def sys_endianness() -> str:
    """
    Returns the byte order (endianness) of the system.

    Returns either "little" or "big" to indicate whether the system uses
    little-endian or big-endian byte order. Most modern systems are little-endian.

    Returns:
        str: Either "little" or "big"

    Examples:
        >>> endian = sys_endianness()
        >>> if sys_endianness() == "little":
        ...     print("Little-endian system (most common)")
        >>> else:
        ...     print("Big-endian system (rare)")
        >>> # Binary format compatibility
        >>> format_spec = ">" if sys_endianness() == "big" else "<"
    """
    return sys.endianness()

def sys_executable() -> str:
    """
    Returns the path to the current executable.

    Gets the absolute path to the currently running executable file (the spaces interpreter,
    or the binary running the Starlark script). Useful for spawning child processes,
    self-location, and relative path resolution.

    Returns:
        str: Absolute path to the current executable

    Raises:
        Error: If executable path cannot be determined (rare)

    Examples:
        >>> exe_path = sys_executable()
        >>> print(f"Running: {exe_path}")
        >>> # Resolve relative to executable location
        >>> exe_dir = exe_path.replace(exe_path.split("/")[-1], "")
        >>> resources_dir = f"{exe_dir}/resources"
        >>> # Self-spawn or version detection
        >>> current_exe = sys_executable()
    """
    return sys.executable()

# ============================================================================
# Environment Detection
# ============================================================================

def sys_is_ci() -> bool:
    """
    Returns True if running in a continuous integration (CI) environment.

    Detects common CI environment markers including GitHub Actions, GitLab CI,
    CircleCI, Travis CI, Jenkins, TeamCity, and others. Useful for conditional
    behavior in CI environments (e.g., stricter checks, different output, timeouts).

    Returns:
        bool: True if running in a detected CI environment, False otherwise

    Examples:
        >>> if sys_is_ci():
        ...     print("Running in CI")
        >>> else:
        ...     print("Running locally")
        >>> # Adjust behavior for CI
        >>> if sys_is_ci():
        ...     skip_interactive_prompts()
        ...     disable_debug_output()
        >>> else:
        ...     enable_debug_logging()
        >>> # Different test configurations
        >>> if sys_is_ci():
        ...     use_stricter_timeouts()
        ...     fail_on_warnings()
    """
    return sys.is_ci()

# ============================================================================
# System Information Reporting
# ============================================================================

def sys_info() -> dict:
    """
    Returns a dictionary with all available system information.

    Gathers and returns a comprehensive snapshot of system information in a
    single convenient dictionary. Useful for logging, debugging, and configuration.

    Returns:
        dict: Dictionary containing all system information with keys:
            - "os": Operating system name
            - "arch": CPU architecture
            - "hostname": Machine hostname
            - "username": Current username
            - "home": Home directory path
            - "cpu_count": Number of logical CPUs
            - "total_memory_bytes": Total RAM in bytes
            - "total_memory_gb": Total RAM in gigabytes
            - "endianness": Byte order ("little" or "big")
            - "executable": Path to current executable
            - "is_ci": True if running in CI environment

    Examples:
        >>> info = sys_info()
        >>> print(f"OS: {info['os']}, Arch: {info['arch']}")
        >>> # Full system report
        >>> for key in info:
        ...     print(f"{key}: {info[key]}")
        >>> # Conditional logic based on system info
        >>> if info["os"] == "windows":
        ...     print("Windows configuration needed")
        >>> # Save system info for logs
        >>> load("//@star/sdk/star/std/json.star", "json_encode")
        >>> load("//@star/sdk/star/std/fs.star", "fs_write_text")
        >>> fs_write_text("sys-info.json", json_encode(info))
    """
    return {
        "os": sys_os(),
        "arch": sys_arch(),
        "hostname": sys_hostname(),
        "username": sys_username(),
        "home": sys_user_home(),
        "cpu_count": sys_cpu_count(),
        "total_memory_bytes": sys_total_memory_bytes(),
        "total_memory_gb": sys_total_memory_gb(),
        "endianness": sys_endianness(),
        "executable": sys_executable(),
        "is_ci": sys_is_ci(),
    }

# ============================================================================
# Process Control
# ============================================================================

def sys_exit(code: int = 0) -> None:
    """
    Exits the program with the specified exit code.

    Terminates the current process immediately with the given exit code. Use 0 for
    successful termination and non-zero values (typically 1) to indicate errors.
    This function does not return; the process is terminated.

    Args:
        code: Exit code to use (default: 0 for success). Use non-zero values to
              indicate error conditions to the calling environment.

    Examples:
        >>> # Exit successfully
        >>> sys_exit(0)
        >>> # Exit with error code
        >>> sys_exit(1)
        >>> # Exit due to an error condition
        >>> if not file_exists(required_file):
        ...     print(f"Error: {required_file} not found")
        ...     sys_exit(1)
        >>> # Indicate success after processing
        >>> sys_exit()
    """
    sys.exit(code)
