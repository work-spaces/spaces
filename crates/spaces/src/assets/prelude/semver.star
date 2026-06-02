"""
Spaces semver built-ins.

Wrappers around the `semver` Starlark module for parsing, comparing,
filtering, resolving, and extracting semantic versions.
"""

def semver_is_valid_version(version: str) -> bool:
    """
    Validates that the given string is a well-formed semantic version.

    Args:
        version: The version string to validate (e.g. `"1.2.3"`, `"1.2.3-rc.1+build.5"`).

    Returns:
        True if the string is a valid semantic version, False otherwise.
    """
    return semver.is_valid_version(version)

def semver_is_valid_requirement(requirement: str) -> bool:
    """
    Validates that the given string is a well-formed semantic version requirement.

    Args:
        requirement: The requirement string to validate (e.g. `"^1.2.0"`, `">=1.0, <2.0"`, `"*"`).

    Returns:
        True if the string is a valid semver requirement, False otherwise.
    """
    return semver.is_valid_requirement(requirement)

def semver_parse(version: str) -> dict:
    """
    Parses a semantic version string into its component parts.

    Args:
        version: The version string to parse.

    Returns:
        A dict with `major` (int), `minor` (int), `patch` (int), `pre` (str), and `build` (str).
    """

    return semver.parse(version)

def semver_matches(version: str, requirement: str) -> bool:
    """
    Returns True if the given version satisfies the given requirement.

    Args:
        version: The semantic version string.
        requirement: The semver requirement string.

    Returns:
        True if the version satisfies the requirement, False otherwise.
    """
    return semver.matches(version, requirement)

def semver_matches_all(version: str, requirements: list[str]) -> bool:
    """
    Returns True if the given version satisfies all of the given requirements.

    Args:
        version: The semantic version string.
        requirements: A list of semver requirement strings, all of which must be satisfied.

    Returns:
        True if the version satisfies every requirement, False otherwise.
    """
    return semver.matches_all(version, requirements)

def semver_compare(lhs: str, rhs: str) -> int:
    """
    Compares two semantic versions.

    Args:
        lhs: The first version string.
        rhs: The second version string.

    Returns:
        -1 if `lhs < rhs`, 0 if equal, 1 if `lhs > rhs`.
    """
    return semver.compare(lhs, rhs)

def semver_sort(versions: list[str]) -> list[str]:
    """
    Sorts a list of semantic versions in ascending order.

    Args:
        versions: The list of version strings to sort.

    Returns:
        The sorted list of versions.
    """
    return semver.sort(versions)

def semver_max(versions: list[str]) -> str:
    """
    Returns the maximum version from a non-empty list of semantic versions.

    Args:
        versions: A non-empty list of version strings.

    Returns:
        The greatest version in the list.
    """
    return semver.max(versions)

def semver_min(versions: list[str]) -> str:
    """
    Returns the minimum version from a non-empty list of semantic versions.

    Args:
        versions: A non-empty list of version strings.

    Returns:
        The smallest version in the list.
    """
    return semver.min(versions)

def semver_filter(versions: list[str], requirements: list[str]) -> list[str]:
    """
    Filters a list of versions to those that satisfy all of the given requirements.

    The returned versions preserve the order they appear in the input.

    Args:
        versions: The list of available version strings.
        requirements: The list of semver requirement strings; each version must satisfy all of them.

    Returns:
        The subset of `versions` that satisfy every requirement.
    """
    return semver.filter(versions, requirements)

def semver_resolve(versions: list[str], requirements: list[str]) -> str | None:
    """
    Resolves the highest version from a list of available versions that satisfies all of the given requirements.

    Args:
        versions: The list of available version strings to choose from.
        requirements: The list of semver requirement strings that the chosen version must satisfy.

    Returns:
        The highest matching version string, or `None` if no version satisfies the requirements.
    """
    return semver.resolve(versions, requirements)

def semver_resolve_all(versions: list[str], requirements: list[str]) -> list[str]:
    """
    Returns versions that satisfy all of the given requirements, sorted in descending order (highest first).

    Args:
        versions: The list of available version strings.
        requirements: The list of semver requirement strings that returned versions must satisfy.

    Returns:
        All matching versions, sorted from highest to lowest.
    """
    return semver.resolve_all(versions, requirements)

def semver_bump_major(version: str) -> str:
    """
    Increments the major component of a version, resetting minor, patch, pre, and build.

    Args:
        version: The version string to bump.

    Returns:
        The bumped version.
    """
    return semver.bump_major(version)

def semver_bump_minor(version: str) -> str:
    """
    Increments the minor component of a version, resetting patch, pre, and build.

    Args:
        version: The version string to bump.

    Returns:
        The bumped version.
    """
    return semver.bump_minor(version)

def semver_bump_patch(version: str) -> str:
    """
    Increments the patch component of a version, resetting pre and build.

    Args:
        version: The version string to bump.

    Returns:
        The bumped version.
    """
    return semver.bump_patch(version)

def semver_is_prerelease(version: str) -> bool:
    """
    Returns True if the version has a pre-release identifier (e.g. `1.2.3-rc.1`).

    Args:
        version: The version string to test.

    Returns:
        True if the version is a pre-release, False otherwise.
    """
    return semver.is_prerelease(version)

def semver_extract_version(name: str, suffixes: list[str] = []) -> str | None:
    """
    Extracts the first semantic version found anywhere in the given string.

    Useful for parsing a version out of a package name, archive filename, or tag.
    Optionally strips a list of suffixes (repeatedly, until none match) from the
    end of `name` before scanning, so archive extensions like `.tar.gz` are not
    greedily consumed as part of a pre-release identifier.

    Args:
        name: The package name (or any string) to scan for a version.
        suffixes: Optional list of suffixes to strip from the end of `name` before scanning.

    Returns:
        The first valid semantic version string found, or `None` if none is present.
    """
    return semver.extract_version(name, suffixes = suffixes)

def semver_extract_all_versions(name: str, suffixes: list[str] = []) -> list[str]:
    """
    Extracts every semantic version found in the given string, in the order they appear.

    Optionally strips a list of suffixes (repeatedly, until none match) from the
    end of `name` before scanning.

    Args:
        name: The string to scan for versions.
        suffixes: Optional list of suffixes to strip from the end of `name` before scanning.

    Returns:
        All valid semantic versions found in the input.
    """
    return semver.extract_all_versions(name, suffixes = suffixes)

def semver_validate_requirements(requirements: list[str]):
    """
    Validates a list of semver requirements, raising an error for the first invalid entry.

    Args:
        requirements: The list of semver requirement strings to validate.
    """
    semver.validate_requirements(requirements)
