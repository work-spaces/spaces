"""
Spaces Filesystem (fs) Module

This module provides ergonomic wrappers around filesystem operations. It supports:
- File reading/writing (text, binary, lines)
- Format-specific operations (JSON, YAML, TOML)
- Directory operations (listing, creating, removing)
- Metadata queries (size, modification time, permissions)
- Symbolic links and permissions management

All paths are relative to the workspace root. Use these functions for safe,
consistent filesystem access in your Starlark scripts.

Examples:
    # Read and parse a JSON file
    data = fs_read_json("config.json")

    # Write formatted YAML
    fs_write_yaml("output.yaml", {"key": "value"})

    # List directory contents
    files = fs_read_directory("src")

    # Check if path exists
    if fs_exists("path/to/file"):
        content = fs_read_text("path/to/file")
"""

# ============================================================================
# FILE I/O - Text Operations
# ============================================================================

def fs_read_text(path: str) -> str:
    """
    Read the entire contents of a text file.

    Args:
        path: Path to the text file (relative to workspace root)

    Returns:
        str: The complete file contents as a string

    Raises:
        Error: If the file cannot be read or does not exist

    Examples:
        # Read a simple text file
        content = fs_read_text("README.md")

        # Read and process line by line
        text = fs_read_text("data.txt")
        for line in text.split("\\n"):
            print(line)
    """
    return fs.read_file_to_string(path)

def fs_write_text(path: str, content: str) -> None:
    """
    Write text content to a file, overwriting if it exists.

    Creates parent directories if they don't exist. If the file already exists,
    its contents will be completely replaced.

    Args:
        path: Path to the file (relative to workspace root)
        content: Text content to write

    Returns:
        None

    Raises:
        Error: If the file cannot be created or written

    Examples:
        # Write a simple text file
        fs_write_text("output.txt", "Hello, World!")

        # Write multi-line content
        lines = ["line 1", "line 2", "line 3"]
        fs_write_text("file.txt", "\\n".join(lines))
    """
    return fs.write_string_to_file(path = path, content = content)

def fs_append_text(path: str, content: str) -> None:
    """
    Append text content to the end of a file.

    If the file does not exist, it will be created. This is useful for
    building up log files or accumulating data.

    Args:
        path: Path to the file (relative to workspace root)
        content: Text content to append

    Returns:
        None

    Raises:
        Error: If the file cannot be created or written

    Examples:
        # Append a log entry
        fs_append_text("build.log", "Starting build...\\n")
        fs_append_text("build.log", "Build complete!\\n")

        # Accumulate results
        fs_append_text("results.txt", "Result: " + str(value) + "\\n")
    """
    return fs.append_string_to_file(path = path, content = content)

def fs_write_string_atomic(path: str, content: str, mode: int = 0o644) -> None:
    """
    Atomically write text to a file with proper permissions.

    This function writes to a temporary file first, then atomically renames it
    to the destination. This ensures the file is never left in a partial or
    corrupt state. Useful for configuration files or critical data.

    Args:
        path: Path to the file (relative to workspace root)
        content: Text content to write
        mode: Unix file permissions as octal number (default: 0o644)

    Returns:
        None

    Raises:
        Error: If the file cannot be written or permissions cannot be set

    Examples:
        # Atomically write a configuration file
        fs_write_string_atomic("config.conf", "setting=value\\n")

        # Write with restricted permissions
        fs_write_string_atomic("secrets.txt", api_key, mode=0o600)
    """
    return fs.write_string_atomic(path = path, content = content, mode = mode)

# ============================================================================
# FILE I/O - Binary and Lines
# ============================================================================

def fs_read_bytes(path: str) -> list:
    """
    Read a file as a list of byte values (0-255).

    Args:
        path: Path to the file (relative to workspace root)

    Returns:
        list: List of integers representing bytes (0-255)

    Raises:
        Error: If the file cannot be read

    Examples:
        # Read binary file
        bytes_list = fs_read_bytes("image.png")
        print("File size: {} bytes".format(len(bytes_list)))
    """
    return fs.read_bytes(path)

def fs_write_bytes(path: str, data: list) -> None:
    """
    Write a list of byte values to a file.

    Args:
        path: Path to the file (relative to workspace root)
        data: List of integers (0-255) representing bytes

    Returns:
        None

    Raises:
        Error: If bytes are out of range or file cannot be written

    Examples:
        # Create a simple binary file
        bytes_data = [0x89, 0x50, 0x4E, 0x47]  # PNG magic bytes
        fs_write_bytes("file.bin", bytes_data)
    """
    return fs.write_bytes(path, data)

def fs_read_lines(path: str) -> list:
    """
    Read a text file as a list of lines (newlines stripped).

    Args:
        path: Path to the text file (relative to workspace root)

    Returns:
        list: List of strings, one per line (without newline characters)

    Raises:
        Error: If the file cannot be read

    Examples:
        # Read and process lines
        lines = fs_read_lines("data.csv")
        for line in lines:
            fields = line.split(",")
            print(fields)

        # Filter empty lines
        lines = [l for l in fs_read_lines("file.txt") if l.strip()]
    """
    return fs.read_lines(path)

def fs_write_lines(path: str, lines: list) -> None:
    """
    Write a list of strings as lines to a file.

    Lines are joined with newline characters. Each string in the list
    becomes one line in the output file.

    Args:
        path: Path to the file (relative to workspace root)
        lines: List of strings to write

    Returns:
        None

    Raises:
        Error: If the file cannot be written

    Examples:
        # Write structured data
        lines = ["header", "line1", "line2", "line3"]
        fs_write_lines("output.txt", lines)

        # Write processed CSV
        csv_lines = ["name,age", "Alice,30", "Bob,25"]
        fs_write_lines("people.csv", csv_lines)
    """
    return fs.write_lines(path, lines)

# ============================================================================
# FORMAT - JSON Operations
# ============================================================================

def fs_read_json(path: str) -> dict:
    """
    Read and parse a JSON file into a dictionary.

    Args:
        path: Path to the JSON file (relative to workspace root)

    Returns:
        dict: Parsed JSON data as a Starlark dictionary

    Raises:
        Error: If the file cannot be read or is not valid JSON

    Examples:
        # Read configuration file
        config = fs_read_json("config.json")
        print(config["setting"])

        # Read and merge data
        data1 = fs_read_json("data1.json")
        data2 = fs_read_json("data2.json")
        combined = dict(data1, **data2)
    """
    return fs.read_json_to_dict(path)

def fs_write_json(path: str, data: dict, pretty: bool = True) -> None:
    """
    Write a dictionary to a JSON file.

    Args:
        path: Path to the JSON file (relative to workspace root)
        data: Dictionary or list to serialize
        pretty: If True (default), write formatted JSON with indentation

    Returns:
        None

    Raises:
        Error: If the data cannot be serialized or file cannot be written

    Examples:
        # Write configuration
        config = {"version": "1.0", "enabled": True}
        fs_write_json("config.json", config)

        # Write compact JSON
        fs_write_json("data.json", {"key": "value"}, pretty=False)
    """
    return fs.write_json_from_dict(path, data, pretty = pretty)

# ============================================================================
# FORMAT - YAML Operations
# ============================================================================

def fs_read_yaml(path: str) -> dict:
    """
    Read and parse a YAML file into a dictionary.

    Args:
        path: Path to the YAML file (relative to workspace root)

    Returns:
        dict: Parsed YAML data as a Starlark dictionary

    Raises:
        Error: If the file cannot be read or is not valid YAML

    Examples:
        # Read configuration
        config = fs_read_yaml("config.yaml")

        # Read and access nested values
        db_config = fs_read_yaml("database.yaml")
        host = db_config["database"]["host"]
    """
    return fs.read_yaml_to_dict(path)

def fs_write_yaml(path: str, data: dict) -> None:
    """
    Write a dictionary to a YAML file.

    Args:
        path: Path to the YAML file (relative to workspace root)
        data: Dictionary or list to serialize as YAML

    Returns:
        None

    Raises:
        Error: If the data cannot be serialized or file cannot be written

    Examples:
        # Write configuration
        config = {
            "app": "myapp",
            "settings": {
                "debug": False,
                "port": 8080,
            }
        }
        fs_write_yaml("config.yaml", config)
    """
    return fs.write_yaml_from_dict(path, data)

# ============================================================================
# FORMAT - TOML Operations
# ============================================================================

def fs_read_toml(path: str) -> dict:
    """
    Read and parse a TOML file into a dictionary.

    Args:
        path: Path to the TOML file (relative to workspace root)

    Returns:
        dict: Parsed TOML data as a Starlark dictionary

    Raises:
        Error: If the file cannot be read or is not valid TOML

    Examples:
        # Read Cargo.toml-like file
        manifest = fs_read_toml("Cargo.toml")
        version = manifest["package"]["version"]
    """
    return fs.read_toml_to_dict(path)

def fs_write_toml(path: str, data: dict) -> None:
    """
    Write a dictionary to a TOML file.

    Args:
        path: Path to the TOML file (relative to workspace root)
        data: Dictionary to serialize as TOML

    Returns:
        None

    Raises:
        Error: If the data cannot be serialized or file cannot be written

    Examples:
        # Write configuration
        config = {
            "app": "myapp",
            "version": "1.0",
            "settings": {
                "debug": False,
            }
        }
        fs_write_toml("config.toml", config)
    """
    return fs.write_toml_from_dict(path, data)

# ============================================================================
# PATH CHECKS - File Type Detection
# ============================================================================

def fs_exists(path: str) -> bool:
    """
    Check if a file or directory exists.

    Args:
        path: Path to check (relative to workspace root)

    Returns:
        bool: True if the path exists, False otherwise

    Examples:
        # Conditional file operations
        if fs_exists("config.json"):
            config = fs_read_json("config.json")
        else:
            config = {"default": True}

        # Check before removing
        if fs_exists("temp.txt"):
            fs_remove("temp.txt")
    """
    return fs.exists(path)

def fs_is_file(path: str) -> bool:
    """
    Check if a path is a regular file.

    Args:
        path: Path to check (relative to workspace root)

    Returns:
        bool: True if the path is a regular file, False otherwise

    Examples:
        # Process only files
        items = fs_read_directory("src")
        files = [item for item in items if fs_is_file(item)]
    """
    return fs.is_file(path)

def fs_is_directory(path: str) -> bool:
    """
    Check if a path is a directory.

    Args:
        path: Path to check (relative to workspace root)

    Returns:
        bool: True if the path is a directory, False otherwise

    Examples:
        # Recursive directory processing
        def process_directory(dir_path):
            for item in fs_read_directory(dir_path):
                if fs_is_directory(item):
                    process_directory(item)
                elif fs_is_file(item):
                    content = fs_read_text(item)
    """
    return fs.is_directory(path)

def fs_is_symlink(path: str) -> bool:
    """
    Check if a path is a symbolic link.

    Args:
        path: Path to check (relative to workspace root)

    Returns:
        bool: True if the path is a symbolic link, False otherwise

    Examples:
        # Find all symlinks in a directory
        items = fs_read_directory(".")
        symlinks = [item for item in items if fs_is_symlink(item)]
    """
    return fs.is_symlink(path)

def fs_is_text_file(path: str) -> bool:
    """
    Check if a file is a text file (vs binary).

    Args:
        path: Path to the file (relative to workspace root)

    Returns:
        bool: True if the file appears to be text, False if binary

    Raises:
        Error: If the path is not a file or cannot be read

    Examples:
        # Process only text files
        items = fs_read_directory("src")
        for item in items:
            if fs_is_file(item) and fs_is_text_file(item):
                process_text_file(item)
    """
    return fs.is_text_file(path)

# ============================================================================
# DIRECTORY OPERATIONS
# ============================================================================

def fs_read_directory(path: str) -> list:
    """
    List the contents of a directory.

    Returns a list of full paths to entries in the directory. The list
    is not sorted and includes both files and subdirectories.

    Args:
        path: Path to the directory (relative to workspace root)

    Returns:
        list: List of full paths to directory entries

    Raises:
        Error: If the directory cannot be read or does not exist

    Examples:
        # List all files in a directory
        entries = fs_read_directory("src")

        # Filter for Python files
        py_files = [e for e in fs_read_directory("src") if e.endswith(".py")]

        # Count files and directories
        all_items = fs_read_directory(".")
        num_files = len([i for i in all_items if fs_is_file(i)])
        num_dirs = len([i for i in all_items if fs_is_directory(i)])
    """
    return fs.read_directory(path)

def fs_mkdir(path: str, parents: bool = False, exist_ok: bool = False) -> None:
    """
    Create a directory.

    Args:
        path: Path to the directory to create (relative to workspace root)
        parents: If True, create parent directories as needed (like mkdir -p)
        exist_ok: If True, don't error if directory already exists

    Returns:
        None

    Raises:
        Error: If the directory cannot be created

    Examples:
        # Create a single directory
        fs_mkdir("output")

        # Create directory tree
        fs_mkdir("build/artifacts/temp", parents=True, exist_ok=True)

        # Create if missing
        if not fs_exists("logs"):
            fs_mkdir("logs")
    """
    return fs.mkdir(path, parents = parents, exist_ok = exist_ok)

def fs_copy(src: str, dst: str, recursive: bool = False, overwrite: bool = False, follow_symlinks: bool = True) -> None:
    """
    Copy a file or directory to a destination.

    Args:
        src: Source path (file or directory)
        dst: Destination path
        recursive: If True, recursively copy directories (required for directories)
        overwrite: If True, overwrite destination if it exists
        follow_symlinks: If True, follow symlinks; if False, copy the link itself

    Returns:
        None

    Raises:
        Error: If source is directory but recursive=False, or if copy fails

    Examples:
        # Copy a single file
        fs_copy("original.txt", "backup.txt")

        # Copy with overwrite
        fs_copy("new_config.json", "config.json", overwrite=True)

        # Copy entire directory tree
        fs_copy("src", "backup/src", recursive=True, overwrite=True)
    """
    return fs.copy(src, dst, recursive = recursive, overwrite = overwrite, follow_symlinks = follow_symlinks)

def fs_move(src: str, dst: str, overwrite: bool = False) -> None:
    """
    Move or rename a file or directory.

    Creates parent directories for the destination if needed.

    Args:
        src: Source path
        dst: Destination path
        overwrite: If True, overwrite destination if it exists

    Returns:
        None

    Raises:
        Error: If destination exists and overwrite=False, or if move fails

    Examples:
        # Rename a file
        fs_move("old_name.txt", "new_name.txt")

        # Move to different directory
        fs_move("file.txt", "archive/file.txt")

        # Move with overwrite
        fs_move("temp.json", "config.json", overwrite=True)
    """
    return fs.move(src = src, dst = dst, overwrite = overwrite)

def fs_remove(path: str, recursive: bool = False, missing_ok: bool = True) -> None:
    """
    Remove a file or directory.

    Args:
        path: Path to remove (relative to workspace root)
        recursive: If True, recursively remove directories and contents
        missing_ok: If True, don't error if path doesn't exist

    Returns:
        None

    Raises:
        Error: If path doesn't exist and missing_ok=False, or if removal fails

    Examples:
        # Remove a file
        fs_remove("temp.txt")

        # Remove directory and contents
        fs_remove("build", recursive=True)

        # Remove without error if missing
        fs_remove("optional.txt", missing_ok=True)
    """
    return fs.remove(path, recursive = recursive, missing_ok = missing_ok)

# ============================================================================
# SYMBOLIC LINKS
# ============================================================================

def fs_symlink(target: str, link: str) -> None:
    """
    Create a symbolic link.

    Creates a symlink at `link` that points to `target`.

    Args:
        target: The target path that the symlink points to
        link: The symlink path to create (relative to workspace root)

    Returns:
        None

    Raises:
        Error: If the symlink cannot be created

    Examples:
        # Create a simple symlink
        fs_symlink("/usr/bin/python3", "python")

        # Create symlink to directory
        fs_symlink("src", "source")
    """
    return fs.symlink(target, link)

def fs_read_link(path: str) -> str:
    """
    Read the target of a symbolic link.

    Args:
        path: Path to the symbolic link (relative to workspace root)

    Returns:
        str: The target path that the symlink points to

    Raises:
        Error: If the path is not a symlink or cannot be read

    Examples:
        # Check where a symlink points
        target = fs_read_link("python")
        print("Symlink points to: " + target)
    """
    return fs.read_link(path)

# ============================================================================
# METADATA OPERATIONS
# ============================================================================

def fs_metadata(path: str) -> dict:
    """
    Get metadata about a file or directory.

    Returns a dictionary with file information including size, timestamps,
    type, and permissions.

    Args:
        path: Path to the file or directory (relative to workspace root)

    Returns:
        dict: Dictionary with keys:
            - size: File size in bytes (int)
            - modified: Last modification time as seconds since epoch (float)
            - created: Creation time as seconds since epoch (float or None)
            - is_dir: Whether it's a directory (bool)
            - is_file: Whether it's a regular file (bool)
            - is_symlink: Whether it's a symbolic link (bool)
            - permissions: Human-readable permissions string (str, e.g., "rw-r--r--")
            - mode: Numeric permissions in octal (int)

    Raises:
        Error: If the path cannot be accessed

    Examples:
        # Get file info
        info = fs_metadata("file.txt")
        print("Size: {} bytes".format(info["size"]))
        print("Permissions: {}".format(info["permissions"]))

        # Find large files
        files = fs_read_directory("src")
        large = [f for f in files if fs_is_file(f) and fs_metadata(f)["size"] > 1000000]
    """
    return fs.metadata(path)

def fs_size(path: str) -> int:
    """
    Get the size of a file in bytes.

    Args:
        path: Path to the file (relative to workspace root)

    Returns:
        int: File size in bytes

    Raises:
        Error: If the file cannot be accessed

    Examples:
        # Check file size
        size = fs_size("large_file.bin")
        print("File is {} bytes".format(size))

        # Find files above threshold
        for file in fs_read_directory("data"):
            if fs_is_file(file) and fs_size(file) > 1000000:
                print("Large file: " + file)
    """
    return fs.size(path)

def fs_modified(path: str) -> float:
    """
    Get the modification time of a file.

    Args:
        path: Path to the file (relative to workspace root)

    Returns:
        float: Modification time as seconds since Unix epoch (January 1, 1970)

    Raises:
        Error: If the file cannot be accessed

    Examples:
        # Get modification time
        mtime = fs_modified("config.json")
        print("Last modified: " + str(mtime))

        # Compare modification times
        config_mtime = fs_modified("config.json")
        backup_mtime = fs_modified("config.backup.json")
        if config_mtime > backup_mtime:
            print("config.json is newer than config.backup.json")
    """
    return fs.modified(path)

def fs_touch(path: str, create: bool = True, update_mtime: bool = True) -> None:
    """
    Update file modification time or create an empty file.

    Args:
        path: Path to the file (relative to workspace root)
        create: If True, create file if it doesn't exist
        update_mtime: If True, update modification time to now

    Returns:
        None

    Raises:
        Error: If file cannot be created or modified

    Examples:
        # Create an empty file
        fs_touch("placeholder.txt")

        # Update modification time
        fs_touch("config.conf", update_mtime=True)

        # Create without error if exists
        fs_touch("marker", create=True)
    """
    return fs.touch(path, create = create, update_mtime = update_mtime)

# ============================================================================
# PERMISSIONS
# ============================================================================

def fs_set_permissions(path: str, mode: int) -> None:
    """
    Set file permissions using numeric mode (Unix octal notation).

    Args:
        path: Path to the file (relative to workspace root)
        mode: Numeric permissions in octal (e.g., 0o644, 0o755, 0o600)

    Returns:
        None

    Raises:
        Error: If permissions cannot be set (may not be supported on all OS)

    Examples:
        # Make file readable/writable by owner only
        fs_set_permissions("secrets.txt", 0o600)

        # Make script executable
        fs_set_permissions("script.sh", 0o755)

        # Standard file permissions
        fs_set_permissions("file.txt", 0o644)
    """
    return fs.set_permissions(path, mode = mode)

def fs_chmod(path: str, spec: str) -> None:
    """
    Change file permissions using symbolic notation (Unix-like).

    Supports specification like "u+rx", "g-w", "a=r", etc.
    Only available on Unix-like systems.

    Args:
        path: Path to the file (relative to workspace root)
        spec: Chmod specification as string ([ugoa][+-=][rwx]+), e.g., "u+x", "u+rx", "a-w"

    Returns:
        None

    Raises:
        Error: If spec is invalid or permissions cannot be set

    Examples:
        # Add execute for owner
        fs_chmod("script.sh", "u+x")

        # Remove write for group and others
        fs_chmod("file.txt", "g-w")
        fs_chmod("file.txt", "o-w")

        # Set exact permissions for owner
        fs_chmod("secrets", "u=rw")
    """
    return fs.chmod(path, spec)

def fs_chown(path: str, user: str, group: str) -> None:
    """
    Change file ownership (Unix-like systems only).

    Args:
        path: Path to the file (relative to workspace root)
        user: Username to set as owner
        group: Group name to set as group owner

    Returns:
        None

    Raises:
        Error: If user/group don't exist or chown fails

    Examples:
        # Change ownership
        fs_chown("config.conf", user="app", group="app")

        # Change to root
        fs_chown("system.conf", user="root", group="root")
    """
    return fs.chown(path, user = user, group = group)
