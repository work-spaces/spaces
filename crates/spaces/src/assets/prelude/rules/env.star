"""
Helper functions for creating environment variables.
"""

def _env_bool(value: bool) -> str:
    return "Yes" if value else "No"

def env_assign(
        name: str,
        value: str,
        help: str) -> dict:
    """
    Assigns an environment variable to the workspace.

    Args:
        name: The name of the environment variable.
        value: The value of the environment variable.
        help: Help text that will be added to the workspace.

    Returns:
        A dictionary containing the environment variable.
    """
    return {
        "name": name,
        "help": help,
        "value": {
            "Assign": {
                "value": value,
            },
        },
    }

def env_append(
        name: str,
        value: str,
        help: str,
        separator: str = ":") -> dict:
    """
    Appends a value to an environment variable.

    The value will be created if it does not exist.

    Args:
        name: The name of the environment variable.
        value: The value to append.
        separator: The separator to use.
        help: Help text that will be added to the workspace.

    Returns:
        A dictionary containing the environment variable.
    """
    return {
        "name": name,
        "help": help,
        "value": {
            "Append": {
                "value": value,
                "separator": separator,
            },
        },
    }

def env_prepend(
        name: str,
        value: str,
        help: str,
        separator: str = ":") -> dict:
    """
    Prepends a value to an environment variable.

    The value will be created if it does not exist.

    Args:
        name: The name of the environment variable.
        value: The value to append.
        separator: The separator to use.
        help: Help text that will be added to the workspace.

    Returns:
        A dictionary containing the environment variable.
    """
    return {
        "name": name,
        "help": help,
        "value": {
            "Prepend": {
                "value": value,
                "separator": separator,
            },
        },
    }

def env_inherit(
        name: str,
        help: str,
        assign_as_default: str | None = None,
        is_secret: bool = False,
        is_required: bool = False,
        is_save_at_checkout: bool = False) -> dict:
    """
    Inherits an environment variable.


    Args:
        name: The name of the environment variable.
        assign_as_default: The default value to assign if the variable is not set in the calling environment.
        is_secret: If true, the value will be redacted in the logs.
        is_required: If true and no value can be inherited and not default is provided, the operation will fail.
        is_save_at_checkout: Whether the variable should be saved at checkout.
        help: Help text that will be added to the workspace.

    Returns:
        A dictionary containing the environment variable.
    """
    return {
        "name": name,
        "help": help,
        "value": {
            "Inherit": {
                "assign_as_default": assign_as_default,
                "is_secret": _env_bool(is_secret),
                "is_required": _env_bool(is_required),
                "is_save_at_checkout": _env_bool(is_save_at_checkout),
            },
        },
    }

def env_script(
        name: str,
        script: str,
        help: str,
        shell: str | None = None,
        assign_as_default: str | None = None,
        is_secret: bool = False,
        is_required: bool = False,
        env: dict = {}) -> dict:
    """
    Evaluates a script to get the value of the environment variable.

    Args:
        name: The name of the environment variable.
        script: The script to evaluate to get the value of the variable.
        shell: The shell to use to evaluate the script.
        env: Environment variables to pass to script evaluation (no other variables will be passed).
        assign_as_default: The default value to assign if the variable is not set in the calling environment.
        is_secret: If true, the value will be redacted in the logs.
        is_required: If true and no value can be inherited and not default is provided, the operation will fail.
        help: Help text that will be added to the workspace.

    Returns:
        A dictionary containing the environment variable.
    """

    return {
        "name": name,
        "help": help,
        "value": {
            "Script": {
                "assign_as_default": assign_as_default,
                "script": script,
                "env": env,
                "shell": shell,
                "is_secret": _env_bool(is_secret),
                "is_required": _env_bool(is_required),
            },
        },
    }
