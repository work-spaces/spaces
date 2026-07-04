"""
Spaces IO Module

Ergonomic wrappers around the built-in `io` namespace for stdin/stdout/stderr
operations in exec scripts.

The underlying built-ins are implemented in `starstd` and already handle:
- one-time stdin caching across multiple reads
- UTF-8 strict/lossy decoding modes
- optional size/line limits
- safe no-op behavior in LSP mode

This module adds friendly, consistently named `io_*` helpers for prelude users.

Examples:
    # Read stdin text
    content = io_read_stdin()

    # Read stdin lines with limits
    lines = io_read_stdin_lines(max_lines = 1000, max_bytes = 1_000_000)

    # Write output
    io_write_stdout("hello", newline = True)
    io_write_stderr("warning", newline = True, flush = True)

    # Print-like helpers
    io_print("done", flush = True)
    io_eprint("failed")
"""

# ============================================================================
# Stdin
# ============================================================================

def io_stdin_is_terminal() -> bool:
    """
    Returns whether stdin is attached to a terminal (TTY).

    Returns:
        bool: True when stdin is interactive; False for piped/redirected input.
    """
    return io.stdin_is_terminal()

def io_read_stdin_to_string(
        encoding: str = "utf-8",
        strip_trailing_newline: bool = False,
        max_bytes: int | None = None) -> str:
    """
    Read all stdin content as a string.

    Args:
        encoding: "utf-8" (strict, default) or "lossy".
        strip_trailing_newline: If True, remove one trailing newline (LF or CRLF).
        max_bytes: Optional positive byte limit.

    Returns:
        str: Decoded stdin content.

    Raises:
        Error: On unsupported encoding, decode failure (utf-8 mode), invalid
               bounds, or when `max_bytes` is exceeded.
    """
    if max_bytes != None:
        return io.read_stdin_to_string(
            encoding = encoding,
            strip_trailing_newline = strip_trailing_newline,
            max_bytes = max_bytes,
        )

    return io.read_stdin_to_string(
        encoding = encoding,
        strip_trailing_newline = strip_trailing_newline,
    )

def io_read_stdin(
        encoding: str = "utf-8",
        strip_trailing_newline: bool = False,
        max_bytes: int | None = None) -> str:
    """
    Convenience alias for io_read_stdin_to_string().

    Args:
        encoding: "utf-8" (strict, default) or "lossy".
        strip_trailing_newline: If True, remove one trailing newline (LF or CRLF).
        max_bytes: Optional positive byte limit.

    Returns:
        str: Decoded stdin content.

    Raises:
        Error: On unsupported encoding, decode failure (utf-8 mode), invalid
               bounds, or when `max_bytes` is exceeded.
    """
    return io_read_stdin_to_string(
        encoding = encoding,
        strip_trailing_newline = strip_trailing_newline,
        max_bytes = max_bytes,
    )

def io_read_stdin_lines(
        encoding: str = "utf-8",
        strip_newline: bool = True,
        max_lines: int | None = None,
        max_bytes: int | None = None) -> list[str]:
    """
    Read stdin and return it as a list of lines.

    Args:
        encoding: "utf-8" (strict, default) or "lossy".
        strip_newline: If True (default), strip line terminators.
        max_lines: Optional positive line-count limit.
        max_bytes: Optional positive byte limit.

    Returns:
        list[str]: stdin split into lines.

    Raises:
        Error: On unsupported encoding, decode failure (utf-8 mode), invalid
               bounds, or when `max_lines` / `max_bytes` is exceeded.
    """
    if max_lines != None and max_bytes != None:
        return io.read_stdin_lines(
            encoding = encoding,
            strip_newline = strip_newline,
            max_lines = max_lines,
            max_bytes = max_bytes,
        )

    if max_lines != None:
        return io.read_stdin_lines(
            encoding = encoding,
            strip_newline = strip_newline,
            max_lines = max_lines,
        )

    if max_bytes != None:
        return io.read_stdin_lines(
            encoding = encoding,
            strip_newline = strip_newline,
            max_bytes = max_bytes,
        )

    return io.read_stdin_lines(
        encoding = encoding,
        strip_newline = strip_newline,
    )

# ============================================================================
# Stdout/Stderr
# ============================================================================

def io_write_stdout(content: str, newline: bool = False, flush: bool = False):
    """
    Write text to stdout.

    Args:
        content: Text to write.
        newline: If True, append a newline after content.
        flush: If True, flush stdout after writing.
    """
    io.write_stdout(content, newline = newline)
    if flush:
        io.flush_stdout()

def io_write_stderr(content: str, newline: bool = False, flush: bool = False):
    """
    Write text to stderr.

    Args:
        content: Text to write.
        newline: If True, append a newline after content.
        flush: If True, flush stderr after writing.
    """
    io.write_stderr(content, newline = newline)
    if flush:
        io.flush_stderr()

def io_flush_stdout():
    """
    Flush stdout.
    """
    return io.flush_stdout()

def io_flush_stderr():
    """
    Flush stderr.
    """
    return io.flush_stderr()

def io_print(content: str = "", end: str = "\n", flush: bool = False):
    """
    Print-like helper that writes to stdout.

    Args:
        content: Text to write.
        end: Trailing text to append (default "\\n").
        flush: If True, flush stdout after writing.
    """
    if end == "\n":
        io.write_stdout(content, newline = True)
    elif end == "":
        io.write_stdout(content, newline = False)
    else:
        io.write_stdout(content + end, newline = False)

    if flush:
        io.flush_stdout()

def io_eprint(content: str = "", end: str = "\n", flush: bool = False):
    """
    Print-like helper that writes to stderr.

    Args:
        content: Text to write.
        end: Trailing text to append (default "\\n").
        flush: If True, flush stderr after writing.
    """
    if end == "\n":
        io.write_stderr(content, newline = True)
    elif end == "":
        io.write_stderr(content, newline = False)
    else:
        io.write_stderr(content + end, newline = False)

    if flush:
        io.flush_stderr()
