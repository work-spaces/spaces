"""
Sandbox configuration for the workspace.

Use these functions to build a sandbox dict for `workspace_set_sandbox()`.

Example:

```python
load("sandbox.star", "sandbox_new", "sandbox_allow_read", "sandbox_allow_write", "sandbox_allow_exec", "sandbox_with_network_blocked")

sandbox = sandbox_new("workspace-sandbox")
sandbox_allow_read(sandbox, "/path/to/inputs")
sandbox_allow_write(sandbox, "/path/to/outputs")
sandbox_allow_exec(sandbox, "/usr/bin")
sandbox_with_network_blocked(sandbox)

```
"""

SANDBOX_NETWORK_BLOCKED = "Blocked"
SANDBOX_NETWORK_UNRESTRICTED = "Unrestricted"

def sandbox_new(name: str = "") -> dict:
    """
    Create a new sandbox configuration dict.

    Args:
        name: Rule name used in sandbox violation diagnostics.

    Returns:
        A new sandbox configuration dict.
    """
    return {
        "name": name,
        "read": [],
        "write": [],
        "exec": [],
        "network": SANDBOX_NETWORK_UNRESTRICTED,
        "deny": [],
    }

def sandbox_allow_read(sandbox: dict, path: str):
    """
    Allow read-only access to a path (file or directory).

    Args:
        sandbox: The sandbox configuration dict.
        path: The path to allow read access to.

    """
    sandbox["read"].append(path)

def sandbox_allow_write(sandbox: dict, path: str):
    """
    Allow read-write access to a path (file or directory).

    Args:
        sandbox: The sandbox configuration dict.
        path: The path to allow read-write access to.

    """
    sandbox["write"].append(path)

def sandbox_allow_exec(sandbox: dict, path: str):
    """
    Allow read-only access to an executable or toolchain directory.

    Semantically distinct from `sandbox_allow_read`: this is for tools
    and compilers the rule may invoke, not data inputs.

    Args:
        sandbox: The sandbox configuration dict.
        path: The path to the executable or toolchain directory.

    """
    sandbox["exec"].append(path)

def sandbox_with_scratch(sandbox: dict, path: str):
    """
    Set the scratch (temp) directory for intermediate artifacts.

    Grants read-write access to the scratch directory.

    Args:
        sandbox: The sandbox configuration dict.
        path: The scratch directory path.

    """
    sandbox["scratch"] = path

def sandbox_with_network_blocked(sandbox: dict):
    """
    Block all network access for the sandboxed rule.

    Use this for compile/link rules that have no reason to reach the network.

    Args:
        sandbox: The sandbox configuration dict.

    """
    sandbox["network"] = SANDBOX_NETWORK_BLOCKED

def sandbox_with_network_unrestricted(sandbox: dict):
    """
    Allow unrestricted network access for the sandboxed rule (this is the default).

    Args:
        sandbox: The sandbox configuration dict.

    """
    sandbox["network"] = SANDBOX_NETWORK_UNRESTRICTED

def sandbox_deny_path(sandbox: dict, path: str):
    """
    Deny access to a path, even if it falls inside a broader read or write grant.

    For example, deny a credential file inside a home directory that was
    granted read access. Note: if any `read` or `write` path is an ancestor
    of a deny path, spaces will return an error — use narrower grants instead.

    Args:
        sandbox: The sandbox configuration dict.
        path: The path to deny access to.

    """
    sandbox["deny"].append(path)

def sandbox_configure_for_os(sandbox: dict):
    """
    Configure the sandbox for the current OS.

    Grants read/write access to common temporary directories (/tmp, /var/tmp)
    and read access to /var. On macOS, also grants exec access to Xcode
    Command Line Tools and common system tool paths.

    Args:
        sandbox: The sandbox configuration dict.

    """

    # Common read/write paths on both Linux and macOS
    sandbox_allow_write(sandbox, "/tmp")
    sandbox_allow_write(sandbox, "/var/tmp")
    sandbox_allow_read(sandbox, "/etc")
    sandbox_allow_read(sandbox, "/var")
    sandbox_allow_read(sandbox, "/dev")
    sandbox_allow_write(sandbox, "/dev")
    sandbox_allow_exec(sandbox, "/dev")

    sandbox_allow_read(sandbox, "/private")
    sandbox_allow_write(sandbox, "/private")
    sandbox_allow_exec(sandbox, "/private")

    if info.is_platform_macos():
        sandbox_allow_read(sandbox, "/System")
        sandbox_allow_read(sandbox, "/Library/Preferences")
        sandbox_allow_exec(sandbox, "/Library/Developer/CommandLineTools")
        sandbox_allow_exec(sandbox, "/Applications/Xcode.app/Contents")
