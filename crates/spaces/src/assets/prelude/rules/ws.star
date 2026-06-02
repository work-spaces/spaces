"""
Spaces workspace built-ins

Note: This file is name ws.star instead of workspace.star because of how
the linter treats starlark files with workspace in the name.

"""

WORKSPACE_SYSROOT = "sysroot"

def workspace_get_absolute_path() -> str:
    """
    Get the absolute path to the workspace

    Returns:
        The absolute path to the workspace
    """
    return workspace.get_absolute_path()

def workspace_get_path_to_checkout() -> str:
    """
    Get the path in the workspace where the current module is located

    Returns:
        The path to the checked out repo or archive
    """
    return workspace.get_path_to_checkout()

def workspace_get_path_to_log_file(name: str) -> str:
    """
    Gets the path to the log file for a given target.

    The log file location changes on every run. Calling this will
    effectively call `workspace_set_always_evaluate(True)` because
    the log path location changes with every run.

    Args:
        name: The name of the rule

    Returns:
        The relative workspace path to the log file
    """

    return workspace.get_path_to_log_file(name)

def workspace_get_cpu_count() -> int:
    """
    Get the number of CPUs available

    Use info_get_cpu_count(). This will be removed in a future release.

    Returns:
        The number of CPUs available
    """
    return workspace.get_cpu_count()

def workspace_get_env_var(name: str) -> str:
    """
    Get the value of an environment variable

    Args:
        name: The name of the environment variable

    Returns:
        The value of the environment variable
    """
    return workspace.get_env_var(name)

def workspace_is_env_var_set(name: str) -> bool:
    """
    Check if an environment variable is set

    Args:
        name: The name of the environment variable

    Returns:
        True if the environment variable is set, False otherwise
    """
    return workspace.is_env_var_set(name)

def workspace_get_env_var_or(name: str, or_value: str) -> str:
    """
    Get the value of an environment variable if it exists.

    Otherwise, the value passed to `or_value` is returned.

    Args:
        name: The name of the environment variable
        or_value: Other value to return

    Returns:
        If available, the value of the environment variable, otherwise `or_value`
    """
    if workspace_is_env_var_set(name):
        return workspace_get_env_var(name)

    return or_value

def workspace_is_env_var_set_to(name: str, expected: str) -> bool:
    """
    Returns true if an env variable is set to the expected value.

    Returns false if the env either does not exist or is not set to the expected value.

    Args:
        name: The name of the environment variable
        expected: The expected value stored in the env variable
    """
    if workspace_is_env_var_set(name):
        return workspace_get_env_var(name) == expected

    return False

def workspace_get_env_var_or_none(name: str) -> str | None:
    """
    Get the value of an environment variable if it exists. Otherwise, return `None`

    Args:
        name: The name of the environment variable

    Returns:
        If available, the value of the environment variable, otherwise `None`
    """
    if workspace_is_env_var_set(name):
        return workspace_get_env_var(name)

    return None

def workspace_is_reproducible() -> bool:
    """
    Check if the workspace is reproducible

    If any repos are on a branch rather than a commit, this will return False.
    Use a lock file (see `--create-lock-file`) to ensure reproducibility.

    Returns:
        True if the workspace is reproducible, False otherwise
    """
    return workspace.is_reproducible()

def _get_member_requirement(url: str, rev: str | None = None, semver: str | None = None) -> dict:
    version_requirment = {}
    if rev != None:
        version_requirment = {"required": {"Revision": rev}}
    elif semver != None:
        version_requirment = {"required": {"SemVer": semver}}
    return {
        "url": url,
    } | version_requirment

def workspace_is_path_to_member_available(
        url: str,
        rev: str | None = None,
        semver: str | None = None) -> bool:
    """
    Checks if a workspace member is available based on a url.

    It is an error to specify `rev` and `semver`.

    Args:
        url: The url of the workspace member
        rev: The revision of the workspace member (or None for any revision)
        semver: The semantic version of the workspace member (or None for any version)

    Returns:
        True if the member is available, False otherwise.
    """
    info.set_minimum_version("0.14.0")
    return workspace.is_path_to_member_available(
        member = _get_member_requirement(url, rev, semver),
    )

def workspace_get_path_to_member(url: str, rev: str | None = None, semver: str | None = None) -> str:
    """
    Gets the path in the workspace to a member pulled from url

    If the member is not found, the program will exit with an error.

    It is an error to specify `rev` and `semver`.

    Args:
        url: The url of the workspace member
        rev: The revision of the workspace member (or None for any revision)
        semver: The semantic version of the workspace member (or None for any version)

    Returns:
        The path to the workspace member.
    """
    return workspace.get_path_to_member(
        member = _get_member_requirement(url, rev, semver),
    )

def workspace_get_path_to_member_or_none(
        url: str,
        rev: str | None = None,
        semver: str | None = None) -> str | None:
    """
    Gets the path in the workspace to a member pulled from url

    If the member is not found, returns None.

    It is an error to specify `rev` and `semver`.

    Args:
        url: The url of the workspace member
        rev: The revision of the workspace member (or None for any revision)
        semver: The semantic version of the workspace member (or None for any version)

    Returns:
        The path to the workspace member or None if not found.
    """
    if workspace_is_path_to_member_available(url, rev, semver):
        return workspace_get_path_to_member(url, rev, semver)

    return None

def workspace_get_path_to_member_with_semver(
        url: str,
        semver: str) -> str:
    """
    Get the path to a workspace member.

    If the the specified requirement is not found, the program will exit with an error.
    Not all workspace members have versions. The version is set manually during checkout
    or pulled from the git rev (tag).

    Args:
        url: The url of the workspace member
        semver: The semver requiement assuming the member has a version

    Returns:
        The path to the workspace member.
    """
    return workspace_get_path_to_member(
        url = url,
        semver = semver,
    )

def workspace_get_path_to_member_with_rev(
        url: str,
        rev: str) -> str:
    """
    Gets the path to a workspace member with the specified revision.

    If the the specified requirement is not found, the program will exit with an error.

    Args:
        url: The url of the workspace member
        rev: the git or sha256 hash

    Returns:
        The path to the workspace member.
    """
    return workspace_get_path_to_member(
        url = url,
        rev = rev,
    )

def workspace_check_member_semver(url: str, semver: str) -> bool:
    """
    Checks if the workspace satifies a requirement

    Args:
        url: The url of the workspace member
        semver: The semver requiement assuming the member has a version

    Returns:
        True if the workspace member is found satisfying semver, False otherwise
    """

    return workspace_is_path_to_member_available(url, rev = None, semver = semver)

def workspace_assert_member_semver(url: str, semver: str):
    """
    Fails if the workspace does not satifies a requirement

    Args:
        url: The url of the workspace member
        semver: The semver requiement assuming the member has a version
    """

    IS_AVAILABLE = workspace_is_path_to_member_available(url, semver = semver)
    if not IS_AVAILABLE:
        info.abort("The workspace member at {} does not satisfy the semver requirement {}".format(url, semver))

def workspace_assert_member_revision(url: str, rev: str):
    """
    Checks if the workspace satifies a requirement

    Args:
        url: The url of the workspace member
        rev: git/sha256 hash
    """

    IS_AVAILABLE = workspace_is_path_to_member_available(url, rev = rev)
    if not IS_AVAILABLE:
        info.abort("The workspace member at {} does not satisfy the revision requirement {}".format(url, rev))

def workspace_check_member_revision(url: str, rev: str) -> bool:
    """
    Checks if the workspace satifies a requirement

    Args:
        url: The url of the workspace member
        rev: git/sha256 hash

    Returns:
        True if the workspace member is found at the specified rev, False otherwise
    """

    return workspace_is_path_to_member_available(url, rev = rev)

def workspace_get_build_archive_info(name: str, archive: dict) -> dict:
    """
    Gets the archive info the specified rule and archive

    Args:
        name: rule name to get info for
        archive: archive object containing details of how to create the archive

    Returns:
        The archive info
    """
    return workspace.get_build_archive_info(
        rule_name = name,
        archive = archive,
    )

def workspace_set_always_evaluate(value: bool):
    """
    Set the always evaluate flag for the workspace.

    This will prevent spaces from skipping the evaluation phase when
    running rules in the workspace.

    """
    return workspace.set_always_evaluate(value)

def workspace_load_value(name: str):
    """
    Loads a value stored using checkout_store_value().

    Args:
        name: The key to load the value under.

    Returns:
        The stored value, or None if no value is stored under the key.
    """
    return workspace.load_value(name)

def workspace_load_value_or(name: str, or_value):
    """
    Loads a value stored using checkout_store_value(), returning a default if none is stored.

    Args:
        name: The key to load the value under.
        or_value: The value to return if no value is stored under the key.

    Returns:
        The stored value, or or_value if no value is stored under the key.
    """
    value = workspace_load_value(name)
    if value == None:
        return or_value
    return value

def workspace_get_path_to_home() -> str:
    """
    Returns the path to the user's home directory in the workspace store.

    Returns:
        The path to the user's home directory in the workspace store.
    """

    return workspace.get_path_to_home()
