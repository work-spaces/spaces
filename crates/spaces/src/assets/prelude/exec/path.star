"""
Spaces Path Module

This module provides ergonomic wrappers around filesystem path manipulation
operations. It supports path joining, component extraction, extension handling,
path normalization, and environment variable expansion.

All functions work seamlessly with both Unix-style (/) and Windows-style (\\)
path separators, automatically handling platform differences.

Examples:
    # Join path components
    full_path = path_join(["src", "main", "app.py"])

    # Extract directory and filename
    dir_part = path_dirname("/home/user/documents/file.txt")
    file_part = path_basename("/home/user/documents/file.txt")

    # Handle file extensions
    name_without_ext = path_stem("document.tar.gz")
    new_path = path_with_extension("input.txt", "md")

    # Work with paths
    abs_path = path_absolute("./config.json")
    normalized = path_normalize("a//b/../c/./d")

    # Expand user and environment variables
    home_path = path_expand_user("~/projects/my-app")
    config_path = path_expand_vars("$HOME/.config/app.conf")
"""

# ============================================================================
# Path Joining
# ============================================================================

def path_join(parts: list) -> str:
    """
    Join path segments using the platform separator.

    Intelligently combines multiple path components into a single path string,
    using the appropriate separator for the current platform (/ on Unix, \\ on Windows).

    Args:
        parts: A list of path segments to join (empty strings are handled gracefully)

    Returns:
        str: The joined path using the platform separator

    Raises:
        Error: If path joining fails

    Examples:
        # Join multiple segments
        full_path = path_join(["src", "components", "button.tsx"])
        # Result: "src/components/button.tsx" (or "src\\components\\button.tsx" on Windows)

        # Join with empty strings (they're handled)
        path = path_join(["home", "", "user", "files"])
        # Result: "home/user/files"

        # Join with file extensions
        script_path = path_join(["scripts", "build", "compile.sh"])

        # Mix relative and absolute segments
        path = path_join(["/usr", "local", "bin"])
        # Result: "/usr/local/bin"
    """
    return path.join(parts)

# ============================================================================
# Path Component Extraction
# ============================================================================

def path_dirname(filepath: str) -> str:
    """
    Extract the directory portion of a path.

    Returns the parent directory path without the final filename component.
    Useful for getting the folder containing a file.

    Args:
        filepath: The file path to extract the directory from

    Returns:
        str: The directory portion of the path (empty string if no parent exists)

    Raises:
        Error: If path extraction fails

    Examples:
        # Extract directory from file path
        directory = path_dirname("/home/user/documents/report.pdf")
        # Result: "/home/user/documents"

        # Extract from relative path
        directory = path_dirname("src/config/settings.json")
        # Result: "src/config"

        # Path with only filename
        directory = path_dirname("README.md")
        # Result: "" (empty string)

        # Trailing slash handling
        directory = path_dirname("/usr/local/bin/")
        # Result: "/usr/local"  (trailing slash is stripped; "bin" becomes the last component)
    """
    return path.dirname(filepath)

def path_basename(filepath: str) -> str:
    """
    Extract the final path component (filename).

    Returns the last component of a path (the filename), excluding any
    directory parts.

    Args:
        filepath: The file path to extract the filename from

    Returns:
        str: The filename (final path component)

    Raises:
        Error: If path extraction fails

    Examples:
        # Extract filename
        name = path_basename("/home/user/documents/report.pdf")
        # Result: "report.pdf"

        # Extract from relative path
        name = path_basename("src/components/Button.tsx")
        # Result: "Button.tsx"

        # Just a filename
        name = path_basename("README.md")
        # Result: "README.md"

        # Directory path with trailing slash
        name = path_basename("/usr/local/bin/")
        # Result: "bin"
    """
    return path.basename(filepath)

def path_split(filepath: str) -> tuple:
    """
    Split a path into directory and filename components.

    Separates a path into its parent directory and final filename in one operation.
    Equivalent to calling (path_dirname(), path_basename()) but more efficient.

    Args:
        filepath: The file path to split

    Returns:
        tuple: A (directory, filename) tuple

    Raises:
        Error: If path splitting fails

    Examples:
        # Split a file path
        dir, name = path_split("/home/user/documents/report.pdf")
        # Result: ("/home/user/documents", "report.pdf")

        # Split relative path
        dir, name = path_split("src/config/settings.json")
        # Result: ("src/config", "settings.json")

        # Split just a filename
        dir, name = path_split("README.md")
        # Result: ("", "README.md")

        # Use split for directory traversal
        dir, name = path_split("/usr/local/bin/python3")
        # Result: ("/usr/local/bin", "python3")
    """
    return path.split(filepath)

# ============================================================================
# File Stem and Extension Handling
# ============================================================================

def path_stem(filepath: str) -> str:
    """
    Extract the file stem (filename without extension).

    Returns the filename without its final extension. For files with multiple
    dots, only the last extension is removed (e.g., "archive.tar.gz" -> "archive.tar").

    Args:
        filepath: The file path to extract the stem from

    Returns:
        str: The filename without the final extension

    Raises:
        Error: If stem extraction fails

    Examples:
        # Extract stem from regular file
        stem = path_stem("/home/user/documents/report.pdf")
        # Result: "report"

        # File with multiple dots
        stem = path_stem("archive.tar.gz")
        # Result: "archive.tar"

        # File without extension
        stem = path_stem("Makefile")
        # Result: "Makefile"

        # Path with directory
        stem = path_stem("/var/log/application.log")
        # Result: "application"

        # Hidden file on Unix
        stem = path_stem(".bashrc")
        # Result: ".bashrc"  (leading-dot files with no further dots: whole name is the stem)
    """
    return path.stem(filepath)

def path_extension(filepath: str) -> str:
    """
    Extract the file extension without the dot.

    Returns the file extension (final component after the last dot), without
    including the dot itself. Returns empty string if no extension exists.

    Args:
        filepath: The file path to extract the extension from

    Returns:
        str: The file extension without the dot (empty if no extension)

    Raises:
        Error: If extension extraction fails

    Examples:
        # Extract simple extension
        ext = path_extension("document.pdf")
        # Result: "pdf"

        # From full path
        ext = path_extension("/home/user/config.json")
        # Result: "json"

        # File without extension
        ext = path_extension("Makefile")
        # Result: "" (empty string)

        # Multiple dots - only last extension
        ext = path_extension("archive.tar.gz")
        # Result: "gz"

        # Hidden file
        ext = path_extension(".gitignore")
        # Result: "" (empty string)
    """
    return path.extension(filepath)

def path_with_extension(filepath: str, new_ext: str) -> str:
    """
    Replace or add a file extension.

    Creates a new path with a different extension, replacing any existing
    extension or adding one if the file has none.

    Args:
        filepath: The original file path
        new_ext: The new extension (without the dot; dot will be added automatically)

    Returns:
        str: The path with the new extension

    Raises:
        Error: If the operation fails

    Examples:
        # Replace existing extension
        new_path = path_with_extension("document.txt", "md")
        # Result: "document.md"

        # Add extension to file without one
        new_path = path_with_extension("README", "txt")
        # Result: "README.txt"

        # From full path
        new_path = path_with_extension("/home/user/image.jpg", "png")
        # Result: "/home/user/image.png"

        # Convert archive format
        new_path = path_with_extension("data.tar.gz", "zip")
        # Result: "data.tar.zip"

        # Empty extension removes the extension
        new_path = path_with_extension("file.txt", "")
        # Result: "file"
    """
    return path.with_extension(filepath, new_ext)

# ============================================================================
# Path Type Checking
# ============================================================================

def path_is_absolute(filepath: str) -> bool:
    """
    Check if a path is absolute (not relative).

    Determines whether a path is absolute (starting from the root) or relative
    to the current directory. Handles both Unix-style and Windows-style paths.

    Args:
        filepath: The path to check

    Returns:
        bool: True if the path is absolute, False if relative

    Examples:
        # Absolute Unix path
        result = path_is_absolute("/home/user/file.txt")
        # Result: True

        # Absolute Windows path
        result = path_is_absolute("C:\\Users\\User\\file.txt")
        # Result: True

        # Relative paths
        result = path_is_absolute("./config.json")
        # Result: False

        result = path_is_absolute("../data/file.txt")
        # Result: False

        result = path_is_absolute("README.md")
        # Result: False

        # Use in path handling
        if path_is_absolute(user_path):
            print("Already absolute")
        else:
            abs_path = path_absolute(user_path)
    """
    return path.is_absolute(filepath)

# ============================================================================
# Path Normalization and Resolution
# ============================================================================

def path_normalize(filepath: str) -> str:
    """
    Normalize a path lexically (without filesystem access).

    Cleans up path representation by collapsing:
    - Repeated separators (// becomes /)
    - Current directory references (. is removed)
    - Parent directory references (..) where possible

    This is purely lexical normalization - it does NOT access the filesystem
    or resolve symbolic links. Use path_canonicalize() for full resolution.

    Args:
        filepath: The path to normalize

    Returns:
        str: The normalized path

    Raises:
        Error: If normalization fails

    Examples:
        # Remove redundant separators
        normalized = path_normalize("a//b///c")
        # Result: "a/b/c"

        # Collapse relative references
        normalized = path_normalize("a/./b")
        # Result: "a/b"

        # Resolve parent references
        normalized = path_normalize("a/b/../c")
        # Result: "a/c"

        # Complex path
        normalized = path_normalize("./foo//bar/../baz/./file.txt")
        # Result: "foo/baz/file.txt"

        # Excess parent references
        normalized = path_normalize("a/b/../../c")
        # Result: "c"

        # Absolute paths
        normalized = path_normalize("/a//b/../c/./d")
        # Result: "/a/c/d"
    """
    return path.normalize(filepath)

def path_absolute(filepath: str) -> str:
    """
    Convert a relative path to absolute.

    Makes a path absolute by resolving it relative to the current working
    directory. Absolute paths are returned unchanged. Does NOT resolve
    symbolic links (use path_canonicalize for that).

    Args:
        filepath: The path to make absolute

    Returns:
        str: The absolute path

    Raises:
        Error: If the current working directory cannot be determined

    Examples:
        # Convert relative path to absolute
        abs_path = path_absolute("./config.json")
        # Result might be: "/home/user/project/config.json"

        # Parent directory reference
        abs_path = path_absolute("../data/file.txt")
        # Result might be: "/home/user/data/file.txt"

        # Absolute path unchanged
        abs_path = path_absolute("/etc/hosts")
        # Result: "/etc/hosts"

        # Just filename
        abs_path = path_absolute("README.md")
        # Result might be: "/home/user/project/README.md"

        # Use with file operations
        for relative_path in ["config.json", "data/input.csv"]:
            abs = path_absolute(relative_path)
            # Now safe to pass to file operations
    """
    return path.absolute(filepath)

def path_canonicalize(filepath: str) -> str:
    """
    Canonicalize a path (resolve symlinks and normalize).

    Performs full path resolution by:
    1. Resolving symbolic links
    2. Removing all . and .. components
    3. Normalizing separators
    4. Resolving to absolute path

    The path must exist on the filesystem. Use path_normalize() for paths
    that might not exist, or path_absolute() for non-existent paths.

    Args:
        filepath: The path to canonicalize (must exist)

    Returns:
        str: The canonical absolute path

    Raises:
        Error: If the path doesn't exist or cannot be read

    Examples:
        # Resolve symlink
        canonical = path_canonicalize("/usr/bin/python")
        # Result: "/usr/bin/python3.11" (the actual target)

        # Normalize existing path
        canonical = path_canonicalize("./config.json")
        # Result: "/home/user/project/config.json" (absolute, normalized)

        # Verify file exists before use
        if path_is_absolute(user_input):
            canonical = path_canonicalize(user_input)
        else:
            print("Path must be absolute")

    Note:
        This function requires the file to exist. For normalizing paths that
        don't exist, use path_normalize() instead.
    """
    return path.canonicalize(filepath)

def path_relative_to(target_path: str, base_path: str) -> str:
    """
    Compute a relative path from base to target.

    Creates a relative path that, when resolved from base_path, would reach
    target_path. Useful for creating relative links or displaying user-friendly
    path relationships.

    Args:
        target_path: The destination path
        base_path: The starting path (typically a directory)

    Returns:
        str: The relative path from base to target

    Raises:
        Error: If relative path cannot be computed

    Examples:
        # Basic relative path
        rel = path_relative_to("/home/user/docs/report.pdf", "/home/user")
        # Result: "docs/report.pdf"

        # Requires going up directories
        rel = path_relative_to("/home/user/docs/file.txt", "/home/user/projects")
        # Result: "../docs/file.txt"

        # Same directory
        rel = path_relative_to("/home/user/file1.txt", "/home/user")
        # Result: "file1.txt"

        # Complex path relationship
        rel = path_relative_to(
            "/var/www/html/assets/image.png",
            "/var/www/html/pages"
        )
        # Result: "../assets/image.png"

        # Use for creating relative symlinks or hrefs
        base = "/home/user/project"
        target = "/home/user/project/build/output.js"
        rel_link = path_relative_to(target, base)
        # Result: "build/output.js"
    """
    return path.relative_to(target_path, base_path)

# ============================================================================
# Environment Variable and User Expansion
# ============================================================================

def path_expand_user(filepath: str) -> str:
    """
    Expand ~ to the user's home directory.

    Replaces the leading ~ in a path with the user's home directory path.
    Handles both ~ alone and ~/... patterns.

    Args:
        filepath: The path with potential ~ to expand

    Returns:
        str: The path with ~ expanded to home directory

    Raises:
        Error: If the home directory cannot be determined

    Examples:
        # Expand home reference
        expanded = path_expand_user("~/projects/myapp")
        # Result: "/home/user/projects/myapp" (on Unix)
        # or: "C:\\Users\\User\\projects\\myapp" (on Windows)

        # Just home directory
        expanded = path_expand_user("~")
        # Result: "/home/user" (or equivalent on Windows)

        # Already expanded path
        expanded = path_expand_user("/etc/hosts")
        # Result: "/etc/hosts" (unchanged)

        # Paths without ~ are unchanged
        expanded = path_expand_user("./relative/path")
        # Result: "./relative/path"

        # Use with configuration files
        config_path = path_expand_user("~/.config/app/settings.json")
        # Now ready for file operations
    """
    return path.expand_user(filepath)

def path_expand_vars(filepath: str) -> str:
    """
    Expand environment variables in a path.

    Replaces $VAR and ${VAR} tokens in the path with their environment
    variable values. Useful for paths that depend on system configuration.

    Args:
        filepath: The path with potential environment variables to expand

    Returns:
        str: The path with environment variables substituted

    Examples:
        # Expand simple variable
        expanded = path_expand_vars("$HOME/.config/app.conf")
        # Result: "/home/user/.config/app.conf"

        # Expand with braces syntax
        expanded = path_expand_vars("${HOME}/.config/app.conf")
        # Result: "/home/user/.config/app.conf"

        # Multiple variables
        expanded = path_expand_vars("$HOME/$PROJECT_NAME/data")
        # Result: "/home/user/myproject/data"

        # Mixed variables
        expanded = path_expand_vars("${DATA_DIR}/processed/${JOB_ID}.csv")
        # Result: "/var/data/processed/job123.csv"

        # Non-existent variables expand to empty string
        expanded = path_expand_vars("$NONEXISTENT/file.txt")
        # Result: "/file.txt"

        # Literal $ if not followed by valid variable name
        expanded = path_expand_vars("price_$100")
        # Result: "price_$100" ($ is not followed by identifier)

        # Use with configuration paths
        data_path = path_expand_vars("$DATA_ROOT/input/${ENV}/data.json")
        # Now ready for file operations
    """
    return path.expand_vars(filepath)

# ============================================================================
# Path Components
# ============================================================================

def path_components(filepath: str) -> list:
    """
    Split a path into normalized components.

    Returns a list of individual path components, with special handling for:
    - Root directories (/)
    - Parent directory references (..)
    - Windows drive prefixes

    Useful for analyzing path structure or reconstructing paths.

    Args:
        filepath: The path to decompose

    Returns:
        list: A list of path components as strings

    Examples:
        # Unix path components
        parts = path_components("/home/user/projects/app.py")
        # Result: ["/", "home", "user", "projects", "app.py"]

        # Relative path components
        parts = path_components("src/components/Button.tsx")
        # Result: ["src", "components", "Button.tsx"]

        # Path with parent references
        parts = path_components("a/b/../c")
        # Result: ["a", "b", "..", "c"]

        # Windows path
        parts = path_components("C:\\Users\\User\\file.txt")
        # Result: ["C:\\", "Users", "User", "file.txt"]

        # Single component
        parts = path_components("file.txt")
        # Result: ["file.txt"]

        # Use for path analysis
        components = path_components("/var/log/app.log")
        if components[0] == "/":
            print("This is an absolute path")
    """
    return path.components(filepath)

# ============================================================================
# Parent Directory Navigation
# ============================================================================

def path_parent(filepath: str, levels: int = 1) -> str:
    """
    Get the parent directory, optionally multiple levels up.

    Returns the nth parent directory by traversing up the path n times.
    Returns empty string if there are no more parents.

    Args:
        filepath: The path to get the parent of
        levels: Number of parent directory levels to go up (default: 1)

    Returns:
        str: The parent path, or empty string if no more parents exist

    Raises:
        Error: If path traversal fails

    Examples:
        # Get immediate parent
        parent = path_parent("/home/user/projects/app.py")
        # Result: "/home/user/projects"

        # Get multiple levels up
        parent = path_parent("/home/user/projects/src/components/app.py", levels=3)
        # Result: "/home/user/projects"

        # Relative path
        parent = path_parent("a/b/c", levels=1)
        # Result: "a/b"

        # Go up multiple levels
        parent = path_parent("a/b/c/d", levels=2)
        # Result: "a/b"

        # More levels than exist
        parent = path_parent("file.txt", levels=5)
        # Result: "" (empty string - no more parents)

        # Useful for navigation
        current = "/usr/local/bin/python"
        bin_dir = path_parent(current)  # "/usr/local/bin"
        lib_dir = path_parent(current, levels=2)  # "/usr/local"
    """
    if levels <= 0:
        return filepath
    return path.parent(filepath, levels)

# ============================================================================
# Utility Functions
# ============================================================================

def path_separator() -> str:
    """
    Get the platform path separator.

    Returns the character used to separate path components on the current
    platform: "/" on Unix/macOS/Linux, "\\" on Windows.

    Returns:
        str: The path separator ("/" or "\\")

    Examples:
        # Get separator for current platform
        sep = path_separator()
        # Result: "/" on Unix, "\\" on Windows

        # Use for path construction
        sep = path_separator()
        path = "src" + sep + "main" + sep + "app.py"

        # Conditional path handling
        sep = path_separator()
        if sep == "/":
            print("Running on Unix-like system")
        else:
            print("Running on Windows")

        # Usually prefer path_join instead:
        path = path_join(["src", "main", "app.py"])  # More portable
    """
    return path.separator()
