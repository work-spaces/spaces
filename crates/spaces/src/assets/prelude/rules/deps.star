"""
Staces starklark function module for adding deps to a rule.

Dependencies defined all the the inputs to a rule. To calculate
the rule digest, all the depenencies are hashed and hashed with
the rule's definition.

Types:

- Rule: A rule listed as a dependency will cause the depedency rule to be executed before the dependent rule.
- Target: A target listed as a dependency will cause the rule owning the dependency to be executed before the dependent rule. If none of the targets have been changed, the rule will be skipped.
- Glob: A glob as a dependency will cause the rule to only execute if any of the glob matches have changed.

"""

load("glob.star", "glob")

def deps_glob(
        includes: list[str],
        excludes: list[str] = []) -> dict:
    """
    Creates a glob that can be passed to deps to create the dependencies.

    Args:
        includes: list of glob expressions to include
        excludes: list of glob expressions to exclude

    Returns:
        glob dict that can be passed to deps.
    """
    return glob(includes, excludes)

def deps_run_once(rules: list[str] = []) -> list[dict]:
    """
    Creates a deps list that will run once and be skipped.

    Args:
        rules: list of rules to add as dependencies.

    Returns:
        List of deps that can be used with run/checkout targets
    """
    return [{"Rule": rule} for rule in rules] + [{"Glob": {"Includes": []}}]

def deps(
        rules: list[str] = [],
        globs: list[dict] = [],
        files: list[str] = []) -> list[dict]:
    """
    Create a list of deps using rules, globs and/or targets.

    Args:
        rules: list of rules to add as dependencies.
        globs: list of `deps_glob()` objects defining includes and excludes
        files: list of files to add as dependencies.

    Returns:
        List of deps that can be used with run/checkout targets
    """
    RULES = [{"Rule": rule} for rule in rules]
    GLOBS_INCLUDES = [{"Glob": {"Includes": glob["Includes"]}} for glob in globs]
    GLOBS_EXCLUDES = [{"Glob": {"Excludes": glob["Excludes"]}} for glob in globs]
    FILES = [{"Glob": {"Includes": [file]}} for file in files]
    return RULES + GLOBS_INCLUDES + GLOBS_EXCLUDES + FILES
