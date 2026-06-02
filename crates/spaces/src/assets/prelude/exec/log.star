"""
Logging utilities for Starlark scripts.

This module provides logging functions for recording messages at different severity levels
(debug, info, warn, error, fatal). The logging system is configurable and can be tuned
via environment variables and runtime settings.

Example:
    # Basic logging
    log_trace("Low-level detail about execution")
    log_debug("Starting process...")
    log_info("Processing file: %s" % filename)
    log_warn("This feature is deprecated")
    log_error("Failed to process file: %s" % error)

    # Set log level
    log_set_level("debug")  # Enable debug messages
    log_set_level("info")   # Default level
    log_set_level("warn")   # Warn and above
    log_set_level("error")  # Errors only

    # Configure format
    log_set_format("text")  # Human-readable text format (default)
    log_set_format("json")  # JSON format for structured logging

    # Fatal logging (stops execution)
    if not file_exists:
        log_fatal("Required file not found")  # Terminates the script
"""

def log_set_level(level: str):
    """
    Sets the minimum log level for all subsequent log messages.

    Only messages at or above the specified level will be logged. This is useful
    for controlling verbosity during script execution. The log level can be
    overridden at runtime via the SPACES_ENV_LOG environment variable.

    Args:
        level: One of "trace", "debug", "info", "warn", "error", or "off".
               - "trace": Most verbose, includes everything
               - "debug": Detailed diagnostic information
               - "info": Informational messages (default)
               - "warn": Warning messages and above
               - "error": Error messages only
               - "off": Disable all logging

    Returns:
        None

    Raises:
        Error: If an invalid log level is provided

    Examples:
        >>> log_set_level("debug")  # Enable all messages
        >>> log_debug("Detailed information")  # Now visible
        >>> log_set_level("error")  # Only show errors
        >>> log_info("This won't be logged")  # Suppressed
        >>> log_error("This will be shown")  # Visible
    """
    return log.set_level(level)

def log_trace(message: str):
    """
    Logs a trace-level message.

    Trace messages are the most verbose level and are disabled by default.
    Enable them with ``log_set_level("trace")`` or the ``SPACES_ENV_LOG=trace``
    environment variable.  Use trace for very fine-grained diagnostic output
    such as per-iteration loop state or low-level I/O details.

    Args:
        message: The trace message to log.

    Returns:
        None

    Examples:
        >>> log_set_level("trace")
        >>> log_trace("Entering loop iteration %d" % i)
        >>> log_trace("Raw bytes: %s" % data)
    """
    return log.trace(message)

def log_set_format(format: str):
    """
    Sets the output format for log messages.

    Changes how log messages are formatted when displayed. The format applies
    to all subsequent log messages. Different formats are useful for different
    scenarios: human-readable text for interactive use, JSON for log aggregation.

    Args:
        format: One of "text" or "json".
                - "text": Human-readable format with timestamps (default)
                  Example: "[2024-01-15 10:30:45.123] [INFO] Processing started"
                - "json": Structured JSON format for log parsers
                  Example: {"timestamp": "2024-01-15T10:30:45.123Z", "level": "info", "message": "Processing started"}

    Returns:
        None

    Raises:
        Error: If an invalid format is provided

    Examples:
        >>> log_set_format("text")  # Human-readable (default)
        >>> log_info("Starting job")
        >>> log_set_format("json")  # For log aggregation systems
        >>> log_info("Job completed")
    """
    return log.set_format(format)

def log_debug(message: str):
    """
    Logs a debug-level message.

    Debug messages are the most verbose and should be used for detailed diagnostic
    information. They are typically disabled in production but enabled during
    development and troubleshooting.

    Args:
        message: The debug message to log. Can include string formatting.

    Returns:
        None

    Examples:
        >>> log_debug("Processing record %d" % record_id)
        >>> log_debug("Current state: %s" % state_dict)
        >>> log_debug("Execution took %.2f seconds" % elapsed_time)
    """
    return log.debug(message)

def log_info(message: str):
    """
    Logs an info-level message.

    Informational messages provide general information about program execution.
    This is the default log level and is suitable for normal operation messages
    that users should know about.

    Args:
        message: The info message to log.

    Returns:
        None

    Examples:
        >>> log_info("Starting backup process")
        >>> log_info("Processed 1000 records")
        >>> log_info("Backup completed successfully")
        >>> log_info("Configuration loaded from: %s" % config_path)
    """
    return log.info(message)

def log_warn(message: str):
    """
    Logs a warning-level message.

    Warning messages indicate potentially problematic situations that should be
    investigated but don't prevent normal execution. Use this for deprecated
    features, unusual conditions, or resource constraints.

    Args:
        message: The warning message to log.

    Returns:
        None

    Examples:
        >>> log_warn("Config file is older than 30 days")
        >>> log_warn("Running with less than 100MB free disk space")
        >>> log_warn("This API is deprecated, use new_api() instead")
        >>> log_warn("Performance degradation detected")
    """
    return log.warn(message)

def log_error(message: str):
    """
    Logs an error-level message.

    Error messages indicate failures in specific operations. The error is logged
    but execution continues. Use log_fatal() if you need to terminate execution.

    Args:
        message: The error message to log.

    Returns:
        None

    Examples:
        >>> log_error("Failed to connect to database")
        >>> log_error("Invalid input: %s" % error_details)
        >>> log_error("File not found: %s" % filepath)
        >>> log_error("Retry attempt 3 failed")
    """
    return log.error(message)

def log_fatal(message: str):
    """
    Logs an error message and terminates script execution.

    Fatal errors indicate unrecoverable problems that prevent further execution.
    The message is logged at error level, then script execution stops immediately.
    Use this for critical failures where recovery is impossible.

    Args:
        message: The fatal error message to log before termination.

    Returns:
        None (execution never returns)

    Raises:
        Error: Always raises an error with the provided message, terminating execution

    Examples:
        >>> if not os.path.exists(required_file):
        ...     log_fatal("Required file missing: %s" % required_file)
        >>> # This line would never execute if log_fatal was called
        >>> log_info("Script completed")

    Note:
        This function always terminates script execution. There is no way to
        catch or recover from a fatal error - it will always stop the script.
    """
    return log.fatal(message)
