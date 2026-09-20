"""
Defines a glob expression object used with deps and targets.
"""

load("globs.star", "globs_new")

def glob(includes: list[str], excludes: list[str] = []) -> dict:
    """
    Creates a glob expression object used with deps and targets

    Deprecated: use `globs_new` instead.

    Args:
        includes: list of glob expressions to include
        excludes: list of glob expressions to exclude

    Returns:
        glob dict that can be passed to create deps and targets.
    """
    return globs_new(includes, excludes)
