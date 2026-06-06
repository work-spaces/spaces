"""
Spaces Text Module

This module provides fast, line-oriented parsing and processing of large text
files, particularly useful for analyzing build logs, compiler output, and other
structured text data.

The text module supports:
- Efficient line-by-line file scanning with callbacks
- Pattern matching with regex support
- Diagnostic creation and rendering in multiple formats
- Line counting, head/tail operations
- Window-based scanning for multi-line patterns

Examples:
    # Count lines in a file
    count = text_line_count("build.log")

    # Get first and last lines
    first_lines = text_head("build.log", 10)
    last_lines = text_tail("build.log", 10)

    # Scan file with callback
    def process_line(line, line_num):
        if "error" in line:
            return text_diagnostic(
                file="build.log",
                severity="error",
                message=line,
                line=line_num
            )
        return None

    diagnostics = text_scan_file("build.log", process_line)

    # Render diagnostics
    github_output = text_render_diagnostics(diagnostics, format="github")
    print(github_output)
"""

# ============================================================================
# File Scanning Functions
# ============================================================================

def text_scan_file(path: str, callback, encoding: str = "utf-8", strip_newline: bool = True):
    """
    Stream a file line-by-line and invoke a callback for each line.

    Reads the file efficiently without loading it entirely into memory,
    making it suitable for large files. The callback is invoked for each
    line with the line content and line number.

    Args:
        path: Path to the file to scan
        callback: Function with signature (line: str, line_num: int) -> value
                 Return None to skip, any other value to collect
        encoding: "utf-8" (strict) or "lossy" (replacement chars for invalid UTF-8)
        strip_newline: Whether to remove trailing newline characters

    Returns:
        list: All non-None values returned by the callback

    Examples:
        def find_errors(line, n):
            if "ERROR" in line:
                return {"line": n, "text": line}
            return None

        errors = text_scan_file("app.log", find_errors)
    """
    options = {"path": path}
    if encoding != "utf-8":
        options["encoding"] = encoding
    if strip_newline != True:
        options["strip_newline"] = strip_newline
    return text.scan_file(options, callback)

def text_scan_lines(content: str, callback):
    """
    Scan a string line-by-line with a callback.

    Similar to scan_file but operates on an already-loaded string.
    Splits on newlines and invokes the callback for each line.

    Args:
        content: String content to scan
        callback: Function with signature (line: str, line_num: int) -> value

    Returns:
        list: All non-None values returned by the callback
    """
    return text.scan_lines(content, callback)

# ============================================================================
# Line Count and Range Functions
# ============================================================================

def text_line_count(path: str) -> int:
    """
    Count the number of lines in a file.

    Efficiently counts lines by scanning for newline characters without
    UTF-8 validation, making it very fast even for large files.

    Args:
        path: Path to the file

    Returns:
        int: Number of lines in the file

    Examples:
        count = text_line_count("big_log.txt")
        print(f"File has {count} lines")
    """
    return text.line_count(path)

def text_read_line_range(path: str, start: int, end: int) -> list:
    """
    Read a range of lines from a file (1-based, inclusive).

    Args:
        path: Path to the file
        start: First line number (1-based)
        end: Last line number (1-based, inclusive)

    Returns:
        list[str]: Lines in the specified range

    Examples:
        lines = text_read_line_range("file.txt", 10, 20)
    """
    return text.read_line_range(path, start, end)

def text_head(path: str, n: int) -> list:
    """
    Get the first n lines of a file.

    Args:
        path: Path to the file
        n: Number of lines to read (must be non-negative)

    Returns:
        list[str]: First n lines

    Examples:
        first_ten = text_head("log.txt", 10)
    """
    return text.head(path, n)

def text_tail(path: str, n: int) -> list:
    """
    Get the last n lines of a file.

    Efficiently reads the file in a single pass using a circular buffer,
    making it suitable for large files.

    Args:
        path: Path to the file
        n: Number of lines to read (must be non-negative)

    Returns:
        list[str]: Last n lines

    Examples:
        last_ten = text_tail("log.txt", 10)
    """
    return text.tail(path, n)

# ============================================================================
# Pattern Matching Functions
# ============================================================================

def text_grep(path: str, pattern: str, ignore_case: bool = False, invert: bool = False, max = None):
    """
    Search for lines matching a regex pattern.

    Args:
        path: Path to the file
        pattern: Regular expression pattern
        ignore_case: Case-insensitive matching
        invert: Return non-matching lines instead
        max: Maximum number of matches (None for unlimited)

    Returns:
        list[dict]: Matches with keys: line, text, match, named

    Examples:
        errors = text_grep("build.log", r"error\\[E\\d+\\]")
        for err in errors:
            print(f"Line {err['line']}: {err['text']}")
    """
    options = {"path": path, "pattern": pattern}
    if ignore_case:
        options["ignore_case"] = ignore_case
    if invert:
        options["invert"] = invert
    if max != None:
        options["max"] = max
    return text.grep(options)

def text_dedent(content: str) -> str:
    """
    Remove common leading whitespace from all non-empty lines.

    Similar to Python's textwrap.dedent. Finds the longest common
    leading whitespace and removes it from all lines.

    Args:
        content: String to dedent

    Returns:
        str: Dedented string

    Examples:
        code = text_dedent('''
            def foo():
                pass
        ''')
    """
    return text.dedent(content)

# ============================================================================
# Window Scanning Functions
# ============================================================================

def text_scan_windows(content: str, n: int, callback):
    """
    Scan content with a sliding window of n consecutive lines.

    Invokes callback(window, start_line_num) for each window position,
    where window is a list of up to n lines.

    Args:
        content: String content to scan
        n: Window size (must be >= 1)
        callback: Function with signature (window: list[str], start_line: int) -> value

    Returns:
        list: All non-None values returned by the callback

    Examples:
        def find_multiline_error(window, start):
            if len(window) >= 2 and "error:" in window[0] and "-->" in window[1]:
                return {"start": start, "lines": window}
            return None

        errors = text_scan_windows(log_content, 3, find_multiline_error)
    """
    return text.scan_windows(content, n, callback)

def text_scan_windows_file(path: str, n: int, callback):
    """
    Scan a file with a sliding window, streaming from disk.

    Like scan_windows but reads from a file efficiently without
    loading the entire file into memory.

    Args:
        path: Path to the file
        n: Window size (must be >= 1)
        callback: Function with signature (window: list[str], start_line: int) -> value

    Returns:
        list: All non-None values returned by the callback
    """
    return text.scan_windows_file(path, n, callback)

# ============================================================================
# Multi-Pattern Regex Scanning
# ============================================================================

def text_regex_scan(content: str, patterns: list) -> list:
    """
    Scan content for multiple regex patterns simultaneously.

    Uses an efficient regex set to quickly determine if any pattern matches,
    then runs captures only on matching patterns.

    Args:
        content: String content to scan
        patterns: List of regex pattern strings

    Returns:
        list[dict]: Matches with keys: pattern_index, line, column, match, named

    Examples:
        patterns = [r"error\\[E\\d+\\]", r"warning\\[W\\d+\\]"]
        matches = text_regex_scan(log_content, patterns)
    """
    return text.regex_scan(content, patterns)

def text_regex_scan_file(path: str, patterns: list) -> list:
    """
    Scan a file for multiple regex patterns, streaming from disk.

    Args:
        path: Path to the file
        patterns: List of regex pattern strings

    Returns:
        list[dict]: Matches with keys: pattern_index, line, column, match, named
    """
    return text.regex_scan_file(path, patterns)

def text_regex_scan_tagged(content: str, patterns: list, first_match_only: bool = False) -> list:
    """
    Scan content for tagged regex patterns.

    Each pattern is a dict with "tag" and "pattern" keys. Results include
    the tag instead of pattern_index.

    Args:
        content: String content to scan
        patterns: List of dicts with "tag" (str) and "pattern" (str) keys
        first_match_only: If True, stop after finding the first match (default: False)

    Returns:
        list[dict]: Matches with keys: tag, line, column, match, named

    Examples:
        patterns = [
            {"tag": "error", "pattern": r"error\\[E\\d+\\]"},
            {"tag": "warning", "pattern": r"warning\\[W\\d+\\]"},
        ]
        matches = text_regex_scan_tagged(log_content, patterns)
    """
    return text.regex_scan_tagged(content, options = {
        "patterns": patterns,
        "first_match_only": first_match_only,
    })

def text_regex_scan_tagged_file(path: str, patterns: list, first_match_only: bool = False) -> list:
    """
    Scan a file for tagged regex patterns, streaming from disk.

    Args:
        path: Path to the file
        patterns: List of dicts with "tag" (str) and "pattern" (str) keys
        first_match_only: If True, stop after finding the first match (default: False)

    Returns:
        list[dict]: Matches with keys: tag, line, column, match, named
    """
    return text.regex_scan_tagged_file(path, options = {
        "patterns": patterns,
        "first_match_only": first_match_only,
    })

# ============================================================================
# Diagnostic Functions
# ============================================================================

def text_diagnostic(
        file: str,
        severity: str,
        message: str,
        line = None,
        column = None,
        end_line = None,
        end_column = None,
        code = None,
        source = None,
        related = None):
    """
    Create a diagnostic dictionary.

    Creates a structured diagnostic record suitable for rendering in various
    formats (GitHub Actions, SARIF, etc.).

    Args:
        file: Source file path
        severity: "error", "warning", "info", "hint", or "note"
        message: Diagnostic message
        line: Line number (1-based, optional)
        column: Column number (1-based, optional)
        end_line: End line number (1-based, optional)
        end_column: End column number (1-based, optional)
        code: Error/warning code (optional)
        source: Source tool name (optional)
        related: List of related diagnostics (optional)

    Returns:
        dict: Diagnostic record

    Examples:
        diag = text_diagnostic(
            file="src/main.rs",
            severity="error",
            message="undefined variable",
            line=42,
            column=5,
            code="E0425"
        )
    """
    options = {"file": file, "severity": severity, "message": message}
    if line != None:
        options["line"] = line
    if column != None:
        options["column"] = column
    if end_line != None:
        options["end_line"] = end_line
    if end_column != None:
        options["end_column"] = end_column
    if code != None:
        options["code"] = code
    if source != None:
        options["source"] = source

    kwargs = {}
    if related != None:
        kwargs["related"] = related

    return text.diagnostic(options, **kwargs)

def text_match_to_diagnostic(
        match,
        severity: str = "error",
        default_message: str = "",
        default_file = None,
        source = None,
        related = None):
    """
    Convert a regex match result to a diagnostic.

    Takes a regex match result (from text_regex_scan_tagged or similar) and
    converts it to a diagnostic by extracting named capture groups. This is
    more efficient than manually extracting fields in Starlark code.

    Named Capture Groups:
        The function looks for these named groups in the match:
        - file: File path
        - line: Line number (1-based)
        - column: Column number (1-based)
        - end_line: End line number (1-based)
        - end_column: End column number (1-based)
        - code: Error/warning code
        - message: Diagnostic message

    Args:
        match: Regex match result from text_regex_scan_tagged or similar
        severity: Severity level ("error", "warning", "info", "hint", or "note")
        default_message: Message to use if no "message" capture group exists
        default_file: Optional file to use if no "file" capture group exists
        source: Source identifier for the diagnostic (e.g., "eslint", "rustc")
        related: List of related diagnostics (optional)

    Returns:
        dict: Diagnostic record that can be rendered with text_render_diagnostics

    Examples:
        # Parse compiler errors
        matches = text_regex_scan_tagged(
            content,
            [{"tag": "error", "pattern": r"(?P<file>\\S+):(?P<line>\\d+): (?P<message>.*)$"}]
        )
        diags = [text_match_to_diagnostic(m, severity="error", source="mycompiler")
                 for m in matches]

        # With default file for matches without file capture
        matches = text_regex_scan_tagged(
            log,
            [{"tag": "warn", "pattern": r"WARNING: (?P<message>.*)$"}]
        )
        diags = [text_match_to_diagnostic(m,
                                          severity="warning",
                                          default_file="build.log",
                                          source="build")
                 for m in matches]
    """
    options = {
        "match": match,
        "severity": severity,
        "default_message": default_message,
    }
    if default_file != None:
        options["default_file"] = default_file
    if source != None:
        options["source"] = source
    if related != None:
        options["related"] = related

    return text.match_to_diagnostic(options)

def text_dedup_diagnostics(diagnostics: list) -> list:
    """
    Remove duplicate diagnostics.

    Deduplicates based on the JSON representation of each diagnostic,
    preserving the first occurrence of each unique diagnostic.

    Args:
        diagnostics: List of diagnostic dicts

    Returns:
        list: Deduplicated diagnostics

    Examples:
        unique = text_dedup_diagnostics(all_diagnostics)
    """
    return text.dedup_diagnostics(diagnostics)

def text_render_diagnostics(diagnostics: list, format: str = "human") -> str:
    """
    Render diagnostics in a specified format.

    Args:
        diagnostics: List of diagnostic dicts
        format: Output format - "human", "github", "json", or "sarif"

    Returns:
        str: Formatted diagnostics

    Formats:
        - "human": One line per diagnostic: "file:line:col: severity: message"
        - "github": GitHub Actions workflow commands (::error, ::warning, etc.)
        - "json": Pretty-printed JSON array
        - "sarif": SARIF 2.1.0 format for static analysis tools

    Examples:
        # For GitHub Actions
        output = text_render_diagnostics(diags, format="github")
        print(output)

        # For human review
        output = text_render_diagnostics(diags, format="human")
        print(output)
    """
    return text.render_diagnostics(diagnostics, format = format)
