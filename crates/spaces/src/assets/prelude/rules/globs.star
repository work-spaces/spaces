"""
Defines a glob expression object used with deps and targets.
"""

def globs_new(includes: list[str], excludes: list[str] = []) -> dict:
    """
    Creates a globs expression object used with deps and targets.

    Args:
        includes: list of globs to include
        excludes: list of globs to exclude

    Returns:
        globs dict that can be passed to create deps and targets.
    """
    return {"Includes": includes, "Excludes": excludes}
