"""
Spaces Temporary File and Directory Module

This module provides ergonomic wrappers for temporary file and directory operations.
It supports:
- Creating temporary directories with automatic cleanup
- Creating temporary files with automatic cleanup
- Explicit cleanup of individual temporary resources
- Batch cleanup of all tracked temporary resources
- Customizable prefixes and suffixes for generated names
- Keep/persist options to retain temporary resources

All temporary files and directories are tracked by default and can be cleaned up
automatically at script end via cleanup_all(). For long-lived operations, you can
mark items to be kept or clean them up individually.

Examples:
    # Create a temporary directory for build artifacts
    build_dir = tmp_dir(prefix = "build-")
    # ... use build_dir ...
    # Cleanup happens automatically or call tmp_cleanup_all() at script end

    # Create a temporary file for logging
    log_file = tmp_file(suffix = ".log")
    # ... write logs to log_file ...

    # Create a cache directory that persists after script
    cache_dir = tmp_dir_keep(prefix = "cache-")

    # Cleanup all temporary resources at script end
    tmp_cleanup_all()
"""

# ============================================================================
# Temporary Directory Creation
# ============================================================================

def tmp_dir(prefix: str = "tmp-") -> str:
    """
    Create a temporary directory and register it for automatic cleanup.

    This function creates a temporary directory in the system's default temp
    location with the specified prefix. The directory is automatically registered
    for cleanup, so it will be deleted when tmp_cleanup_all() is called or at
    script end (if cleanup is configured to run automatically).

    Args:
        prefix: Prefix for the directory name (default: "tmp-")

    Returns:
        str: The full path to the created temporary directory

    Raises:
        Error: If the directory cannot be created

    Examples:
        # Create a temporary build directory
        build_dir = tmp_dir(prefix = "build-")
        # Build artifacts can be written to build_dir

        # Create a temporary output directory
        output_dir = tmp_dir(prefix = "output-")
        # Use output_dir for test results or intermediate files

        # Create with default prefix
        temp_dir = tmp_dir()  # Will have "tmp-" prefix
    """
    return tmp.dir(prefix = prefix)

def tmp_dir_keep(prefix: str = "tmp-") -> str:
    """
    Create a temporary directory that will NOT be automatically cleaned up.

    This function creates a temporary directory in the system's default temp
    location with the specified prefix. The directory is registered with the
    keep flag set to true, so it will NOT be deleted by tmp_cleanup_all().
    This is useful for cache directories or persistent temporary storage.

    Args:
        prefix: Prefix for the directory name (default: "tmp-")

    Returns:
        str: The full path to the created temporary directory

    Raises:
        Error: If the directory cannot be created

    Examples:
        # Create a persistent cache directory
        cache_dir = tmp_dir_keep(prefix = "cache-")
        # This directory will not be automatically cleaned up

        # Create a data directory that persists between runs
        data_dir = tmp_dir_keep(prefix = "data-")
        # Store data that should remain after the script finishes

        # Create with custom prefix
        persist_dir = tmp_dir_keep(prefix = "persist-")
    """
    return tmp.dir_keep(prefix = prefix)

# ============================================================================
# Temporary File Creation
# ============================================================================

def tmp_file(suffix: str = "") -> str:
    """
    Create a temporary file and register it for automatic cleanup.

    This function creates a temporary file in the system's default temp location
    with the specified suffix. The file is automatically registered for cleanup,
    so it will be deleted when tmp_cleanup_all() is called or at script end
    (if cleanup is configured to run automatically).

    Args:
        suffix: Suffix for the file name (default: "" - no suffix)
               Examples: ".log", ".txt", ".json", ".tmp"

    Returns:
        str: The full path to the created temporary file (empty file)

    Raises:
        Error: If the file cannot be created

    Examples:
        # Create a temporary log file
        log_file = tmp_file(suffix = ".log")
        # Write log entries to log_file

        # Create a temporary JSON file
        json_file = tmp_file(suffix = ".json")
        # Use json_file to store temporary JSON data

        # Create a temporary CSV file
        csv_file = tmp_file(suffix = ".csv")
        # Write CSV data to csv_file

        # Create with no suffix
        temp_file = tmp_file()

        # Usage pattern: write and read back
        temp_file = tmp_file(suffix = ".txt")
        fs_write_text(temp_file, "temporary data")
        content = fs_read_text(temp_file)
    """
    return tmp.file(suffix = suffix)

# ============================================================================
# Cleanup Operations
# ============================================================================

def tmp_cleanup(path: str) -> None:
    """
    Explicitly clean up a single registered temporary resource.

    This function immediately deletes a temporary file or directory that was
    previously created and tracked. After calling this function, the resource
    is no longer tracked and cannot be cleaned up again.

    Args:
        path: The path string returned by tmp_dir(), tmp_dir_keep(), or
              tmp_file().  Raises an error if the path is not tracked.

    Returns:
        None

    Raises:
        Error: If the path is not tracked or the deletion fails

    Examples:
        # Create and explicitly clean up a temp file
        temp_file = tmp_file(suffix = ".txt")
        # ... use temp_file ...
        tmp_cleanup(temp_file)

        # Create and explicitly clean up a temp directory
        work_dir = tmp_dir(prefix = "work-")
        # ... use work_dir ...
        tmp_cleanup(work_dir)
    """
    return tmp.cleanup(path)

def tmp_cleanup_all() -> None:
    """
    Clean up all registered temporary resources that are marked for cleanup.

    This function deletes all temporary files and directories that were created
    with tmp_dir(), tmp_file(), etc. and are not marked with the keep flag.
    Resources created with tmp_dir_keep() are NOT deleted.

    Call this function at the end of your script to ensure all temporary
    resources are properly cleaned up and don't accumulate on the filesystem.

    Returns:
        None

    Raises:
        Error: If cleanup of any resource fails (cleanup continues for others)

    Examples:
        # Typical usage pattern at script end
        def main():
            log_file = tmp_file(suffix = ".log")
            cache_dir = tmp_dir_keep(prefix = "cache-")
            work_dir = tmp_dir(prefix = "work-")

            # ... do work with temporary resources ...

            # At script end, clean up only non-keep resources
            tmp_cleanup_all()
            # log_file and work_dir are deleted
            # cache_dir persists

        # Pattern with multiple temp files
        def process_data():
            input_file = tmp_file(suffix = ".input")
            output_file = tmp_file(suffix = ".output")
            staging_dir = tmp_dir(prefix = "staging-")

            # ... process files ...

            # All temporary resources are cleaned up
            tmp_cleanup_all()
    """
    return tmp.cleanup_all()
