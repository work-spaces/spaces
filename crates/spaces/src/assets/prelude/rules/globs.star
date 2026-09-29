"""
Defines a glob expression object used with deps and targets.
"""

_INCLUDES_KEY = "Includes"
_EXCLUDES_KEY = "Excludes"

def globs_new(includes: list[str], excludes: list[str] = []) -> dict:
    """
    Creates a globs expression object used with deps and targets.

    Args:
        includes: list of globs to include
        excludes: list of globs to exclude

    Returns:
        globs dict that can be passed to create deps and targets.
    """
    return {_INCLUDES_KEY: includes, _EXCLUDES_KEY: excludes}

def globs_includes(globs: dict) -> list[str]:
    """
    Gets the Includes part of the globs.

    Args:
        globs: Return value of `globs_new()`

    Returns:
        Includes list for the globs
    """
    return globs.get[_INCLUDES_KEY]

def globs_excludes(globs: dict) -> list[str]:
    """
    Gets the Includes part of the globs.

    Args:
        globs: Return value of `globs_new()`

    Returns:
        Includes list for the globs
    """
    return globs.get[_EXCLUDES_KEY]
