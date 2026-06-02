"""
Defines a shortcut for checkout_update_shortcuts()
"""

def shortcut(command, help):
    """
    Defines a shortcut with the given command and help text.

    Args:
        command: The command to execute.
        help: The help text for the shortcut.

    Returns:
        dict: A dictionary with the command and help text.
    """
    return {
        "command": command,
        "help": help,
    }
