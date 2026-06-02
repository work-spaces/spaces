"""
Helper functions for managing rules that are grouped together.

This helps creating related rules in functins and informing
the caller about the rules that were added.
"""

def rules_new(name: str, rules: list[str]) -> dict:
    """
    Returns a new rule group with the given name and rules.

    Args:
        name (str): The name of the rule group.
        rules (list[str]): The rules to include in the group.

    Returns:
        dict[str, str]: A new rules object with the given name and rules.
    """

    rules_entries = {}
    for value in rules:
        rules_entries[value] = "{}_{}".format(name, value)

    return {"name": name, "rules": rules_entries}

def rules_as_rule(rules: dict, rule_name: str) -> str:
    """
    Returns the rule for the given rule name from the given rule group.

    Args:
        rules: The return value of rules_new()
        rule_name: The name of the rule to retrieve.

    """
    return rules["rules"][rule_name]

def rules_as_dep(rules: dict, rule_name: str) -> str:
    """
    Returns the rule as a dependency for the given rule name from the given rule group.

    Args:
        rules: The return value of rules_new()
        rule_name: The name of the rule to retrieve.

    """
    return ":{}".format(rules_as_rule(rules, rule_name))

def rules_as_deps(rules: dict, rule_names: list[str]) -> list[str]:
    """
    Returns a list of rule dependencies for the given rule names from the given rule group.

    Args:
        rules: The return value of rules_new()
        rule_names: The name of the rule to retrieve.

    """
    return [rules_as_dep(rules, name) for name in rule_names]

def rules_name(rules: dict) -> str:
    """
    Returns the name of the given rule group.

    Args:
        rules: The return value of rules_new()

    """
    return rules["name"]

def rules_name_as_dep(rules: dict) -> str:
    """
    Returns the name of the given rule group.

    Args:
        rules: The return value of rules_new()

    """
    return ":{}".format(rules_name(rules))
