"""
Visibility Helper Functions
"""

VISIBILITY_PUBLIC = "Public"
VISIBILITY_PRIVATE = "Private"

_VISIBILITY_RULES = "Rules"

def visibility_public() -> str:
    """
    Get the public visibility string.

    Returns:
        the public visibility string.
    """
    return VISIBILITY_PUBLIC

def visibility_private() -> str:
    """
    Get the private visibility string.

    Returns:
        the private visibility string.
    """
    return VISIBILITY_PRIVATE

def visibility_rules(rules: list[str]) -> dict[str, list[str]]:
    """
    Get the rules visibility string.

    Args:
        rules: The list of rules as strings.

    Returns:
        Object with list of rules that can be passed to visibility arguments.
    """
    return {_VISIBILITY_RULES: rules}
