"""
User friendly wrapper functions for the spaces checkout built-in functions.
"""

load("asset.star", "asset_home")
load("env.star", "env_assign")

_CHECKOUT_SHELL_SPACES_TOML = "shell.spaces.toml"

# Clone rules that are optional are not run
CHECKOUT_TYPE_OPTIONAL = "Optional"

# Clone rules that are default are always run
CHECKOUT_TYPE_DEFAULT = None

# Sparse checkout modes
CHECKOUT_SPARSE_MODE_CONE = "Cone"  # checkout directories
CHECKOUT_SPARSE_MODE_NO_CONE = "NoCone"  #checkout gitignore like expressions

# Ways to `clone` a repository
CHECKOUT_CLONE_DEFAULT = "Default"  # Just a normal clone
CHECKOUT_CLONE_WORKTREE = "Worktree"  # stores the bare repository in the spaces store
CHECKOUT_CLONE_SHALLOW = "Shallow"  # The rev must be a branch not a tag or commit

CHECKOUT_EXPECT_SUCCESS = "Success"
CHECKOUT_EXPECT_FAILURE = "Failure"
CHECKOUT_EXPECT_ANY = "Any"

# This is the only supported value
CHECKOUT_CLONE_TYPE_REVISION = "Revision"

def checkout_add_exec(
        name: str,
        command: str,
        help: str | None = None,
        args: list[str] = [],
        env: dict = {},
        deps: list[str] = [],
        working_directory: str | None = None,
        platforms: list[str] | None = None,
        log_level: str | None = None,
        redirect_stdout: str | None = None,
        timeout: float | None = None,
        visibility: str | dict[str, list[str]] | None = None,
        expect: str = CHECKOUT_EXPECT_SUCCESS):
    """
    Adds a command to the run dependency graph

    Args:
        name: The name of the rule.
        command: The command to execute.
        help: The help message for the rule.
        args: The arguments to pass to the command.
        deps: The rule dependencies that must be run before this command
        env: key value pairs of environment variables
        working_directory: The directory to run the command (default is workspace root).
        platforms: Platforms to run on (default is all).
        log_level: The log level to use None|App|Passthrough
        expect: The expected result of the command Success|Failure|Any. (default is Success)
        redirect_stdout: The file to redirect stdout to (prefer to parse the log file).
        timeout: Number of seconds to run before sending a kill signal.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visbility.star for more info.
    """

    checkout.add_exec(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "help": help,
            "type": "Run",
            "inputs": None,
            "visibility": visibility,
        },
        exec = {
            "command": command,
            "args": args,
            "working_directory": working_directory,
            "env": env,
            "expect": expect,
            "log_level": log_level,
            "redirect_stdout": redirect_stdout,
            "timeout": timeout,
        },
    )

def checkout_get_compile_commands_spaces_name() -> str:
    """
    Returns the name of the file used with checkout_add_compile_commands_dir().

    This is used internally between the checkout rule and compile_commands_merge()
    """
    return "compile_commands.spaces.json"

def checkout_type_optional() -> str:
    """
    Use `checkout_add_repo(type = checkout_type_optional())` to skip checkout

    Returns:
        str: CHECKOUT_TYPE_OPTIONAL
    """
    return CHECKOUT_TYPE_OPTIONAL

def checkout_type_default() -> None:
    """
    Use `checkout_add_repo(type = checkout_type_default())` to use default checkout behavior

    Returns:
        None: CHECKOUT_TYPE_DEFAULT
    """
    return CHECKOUT_TYPE_DEFAULT

def checkout_sparse_mode_cone() -> str:
    """
    Use `checkout_add_repo(sparse_mode = checkout_sparse_mode_cone())` for sparse cone mode.

    Returns:
        str: CHECKOUT_SPARSE_MODE_CONE
    """
    return CHECKOUT_SPARSE_MODE_CONE

def checkout_sparse_mode_no_cone() -> str:
    """
    Use `checkout_add_repo(sparse_mode = checkout_sparse_mode_no_cone())` for sparse no-cone mode.

    This mode uses gitignore-like expressions for sparse checkout.

    Returns:
        str: CHECKOUT_SPARSE_MODE_NO_CONE
    """
    return CHECKOUT_SPARSE_MODE_NO_CONE

def checkout_clone_default() -> str:
    """
    Use `checkout_add_repo(clone = checkout_clone_default())` for a normal git clone.

    Returns:
        str: CHECKOUT_CLONE_DEFAULT
    """
    return CHECKOUT_CLONE_DEFAULT

def checkout_clone_worktree() -> str:
    """
    Use `checkout_add_repo(clone = checkout_clone_worktree())` to store the bare repository in the spaces store.

    Returns:
        str: CHECKOUT_CLONE_WORKTREE
    """
    return CHECKOUT_CLONE_WORKTREE

def checkout_clone_shallow() -> str:
    """
    Use `checkout_add_repo(clone = checkout_clone_shallow())` for a shallow clone.

    Note: The rev must be a branch, not a tag or commit.

    Returns:
        str: CHECKOUT_CLONE_SHALLOW
    """
    return CHECKOUT_CLONE_SHALLOW

def checkout_add_repo(
        name: str,
        url: str,
        rev: str,
        checkout_type: str = CHECKOUT_CLONE_TYPE_REVISION,
        clone: str = CHECKOUT_CLONE_DEFAULT,
        is_evaluate_spaces_modules: bool | None = None,
        sparse_mode: str | None = None,
        sparse_list: list[str] | None = None,
        working_directory: str | None = None,
        platforms: list[str] | None = None,
        type: str | None = None,
        deps: list[str] = [],
        visibility: str | dict[str, list[str]] | None = None):
    """
    Clones a repository and checks it out at a specific revision.

    For `clone=checkout_clone_default()`, the repo
    is cloned first to the store as a bare repo and then cloned to the workspace.

    Example:

    ```python
    checkout_add_repo(
        "spaces",
        url = "https://github.com/work-spaces/spaces",
        rev = "main"
    )
    ```

    Args:
        name: The name of the rule. This is also the location of the new repo in the workspace.
        url: The git repository URL to clone
        rev: The branch or commit hash to checkout
        checkout_type: Revision
        clone: [checkout_clone_default()](#checkout_clone_default) | [checkout_clone_worktree()](#checkout_clone_worktree)
        is_evaluate_spaces_modules: Whether to evaluate spaces.star files in the repo (default is True).
        sparse_mode: Cone | NoCone
        sparse_list: List of paths to include/exclude
        deps: List of dependencies for the rule.
        type: use [checkout_type_optional()](#checkout_type_optional) to skip rule checkout
        platforms: List of platforms to add the repo to.
        working_directory: The working directory to clone the repository into.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visbility.star for more info.
    """

    effective_sparse_checkout = {
        "sparse_checkout": {"mode": sparse_mode, "list": sparse_list},
    } if sparse_mode != None else {}

    checkout.add_repo(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "type": type,
            "visibility": visibility,
        },
        repo = {
            "url": url,
            "rev": rev,
            "checkout": checkout_type,
            "clone": clone,
            "working_directory": working_directory,
            "is_evaluate_spaces_modules": is_evaluate_spaces_modules,
        } | effective_sparse_checkout,
    )

def checkout_add_archive(
        name: str,
        url: str,
        sha256: str,
        link: str = "Hard",
        includes: list[str] | None = None,
        excludes: list[str] | None = None,
        strip_prefix: str | None = None,
        add_prefix: str = "./",
        filename: str | None = None,
        platforms: list[str] | None = None,
        type: str | None = None,
        headers: dict | None = None,
        deps: list[str] = [],
        visibility: str | dict[str, list[str]] | None = None):
    """
    Adds an archive to the workspace.

    The archive is downloaded to the spaces store and hard-linked to the workspace.

    Args:
        name: The name of the rule.
        url: The URL of the archive to download.
        sha256: The SHA256 checksum of the archive.
        link: Hard | None
        includes: List of globs to include.
        excludes: List of globs to exclude.
        strip_prefix: Prefix to strip from the archive.
        add_prefix: Prefix to add to the archive.
        filename: The filename if it isn't the last part of the URL
        platforms: List of platforms to add the archive to.
        type: use [checkout_type_optional()](#checkout_type_optional) to skip rule checkout
        headers: key-value pairs of headers to use when downloading the archive.
        deps: List of dependencies for the rule.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visbility.star for more info.
    """
    checkout.add_archive(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "type": type,
            "visibility": visibility,
        },
        archive = {
            "url": url,
            "sha256": sha256,
            "link": link,
            "includes": includes,
            "excludes": excludes,
            "strip_prefix": strip_prefix,
            "add_prefix": add_prefix,
            "filename": filename,
            "headers": headers,
        },
    )

def checkout_update_asset(
        name: str,
        destination: str,
        value: dict | list,
        format: str | None = None,
        deps: list[str] = [],
        type: str | None = None,
        platforms: list[str] | None = None,
        visibility: str | dict[str, list[str]] | None = None):
    """
    Updates an asset in the workspace.

    This rule will merge the data of an existing JSON, TOML, or YAML file with the given value.

    Args:
        name: The name of the rule.
        destination: The destination path for the asset.
        format: The format of the asset (json | toml | yaml). Default will get extension from destination.
        value: The value of the asset as a dict to merge with the existing file.
        deps: List of dependencies for the asset.
        type: use [checkout_type_optional()](#checkout_type_optional) to skip rule checkout
        platforms: List of [platforms](/docs/builtins/#rule-options) to add the archive to.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visbility.star for more info.
    """

    effective_format = format if format != None else destination.split(".")[-1]

    checkout.update_asset(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "type": type,
            "visibility": visibility,
        },
        asset = {
            "destination": destination,
            "format": effective_format,
            "value": value,
        },
    )

def checkout_add(
        name: str,
        deps: list[str],
        type: str | None = None,
        platforms: list[str] | None = None,
        visibility: str | dict[str, list[str]] | None = None):
    """
    Adds a target to the workspace.

    Args:
        name: The name of the rule.
        deps: List of dependencies for the target.
        type: use [checkout_type_optional()](#checkout_type_optional) to skip rule checkout
        platforms: List of [platforms](/docs/builtins/#rule-options) to add the archive to.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visbility.star for more info.
    """
    checkout.add(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "type": type,
            "visibility": visibility,
        },
    )

def checkout_add_platform_archive(
        name: str,
        platforms: dict,
        deps: list[str] = [],
        type: str | None = None,
        visibility: str | dict[str, list[str]] | None = None):
    """
    Adds a platform archive to the checkout.

    Platform archives are used to add binary tools based on the host platform.

    Args:
        name: The name of the rule.
        platforms: List of [platforms](/docs/builtins/#rule-options) to add the archive to.
        deps: List of dependencies for the rule.
        type: use [checkout_type_optional()](#checkout_type_optional) to skip rule checkout
        visibility: Rule visibility: `Public|Private|Rules[]`. See visbility.star for more info.
    """
    checkout.add_platform_archive(
        rule = {
            "name": name,
            "type": type,
            "deps": deps,
            "visibility": visibility,
        },
        platforms = platforms,
    )

def checkout_add_oras_archive(
        name: str,
        url: str,
        artifact: str,
        tag: str,
        add_prefix: str,
        manifest_digest_path: str = "/layers/0/digest",
        manifest_artifact_path: str = "/layers/0/annotations/org.opencontainers.image.title",
        globs: list[str] | None = None,
        deps: list[str] = [],
        type: str | None = None,
        platforms: list[str] | None = None,
        visibility: str | dict[str, list[str]] | None = None):
    """
    Adds an oras archive to the workspace.

    Args:
        name: The name of the rule.
        url: The URL of the oras archive to download.
        artifact: The artifact name of the oras archive.
        tag: The tag of the oras archive.
        add_prefix: The prefix to add to the archive.
        manifest_digest_path: The path to the manifest digest in the oras archive.
        manifest_artifact_path: The path to the manifest artifact in the oras archive.
        globs: List of globs to include/exclude.
        deps: List of dependencies for the rule.
        type: use [checkout_type_optional()](#checkout_type_optional) to skip rule checkout
        platforms: List of [platforms](/docs/builtins/#rule-options) to add the archive to.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visbility.star for more info.
    """

    checkout.add_oras_archive(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "type": type,
            "visibility": visibility,
        },
        oras_archive = {
            "url": url,
            "artifact": artifact,
            "tag": tag,
            "manifest_digest_path": manifest_digest_path,
            "manifest_artifact_path": manifest_artifact_path,
            "add_prefix": add_prefix,
            "globs": globs,
        },
    )

def checkout_update_shell(name: str, shell_path: str, args: list[str] = [], deps: list[str] = [], visibility: str | dict[str, list[str]] | None = None):
    """

    Updates the workspace shell configuration that is used with `spaces shell`

    Args:
        name: The name of the rule.
        shell_path: The path to the shell executable.
        args: The arguments to pass to the shell.
        deps: The dependencies of the rule (allows controlling order of updating the file)
        visibility: The visibility of the rule.
        """
    checkout_update_asset(
        name,
        destination = _CHECKOUT_SHELL_SPACES_TOML,
        value = {
            "path": shell_path,
            "args": args,
        },
        deps = deps,
        visibility = visibility,
    )

def checkout_update_shell_startup(
        name: str,
        script_name: str,
        contents: str,
        env_name: str | None = None,
        deps: list[str] = [],
        visibility: str | dict[str, list[str]] | None = None):
    """

    Updates the workspace shell configuration that is used with `spaces shell`

    Args:
        name: The name of the rule.
        script_name: The name of the startup file to generate and store at `.spaces/shell/<script_name>` in the workspace.
        contents: The contents of the startup file.
        env_name: If not None, this will be set to point to the workspace shell startup directory `.spaces/shell`.
        deps: The dependencies of the rule (allows controlling order of updating the file)
        visibility: The visibility of the rule (allows controlling who can see the rule)
    """

    effective_env_name = {"env_name": env_name} if env_name else {}
    checkout_update_asset(
        name,
        destination = _CHECKOUT_SHELL_SPACES_TOML,
        value = {
            "startup": {
                "name": script_name,
                "contents": contents,
            } | effective_env_name,
        },
        deps = deps,
        visibility = visibility,
    )

def checkout_update_shell_shortcuts(name: str, shortcuts: dict, deps: list[str] = [], visibility: str | dict[str, list[str]] | None = None):
    """

    Updates the `.spaces/shell/shortcuts.sh` file with shell functions. This file can be source when starting the shell.

    Args:
        name: The name of the rule.
        shortcuts: A dictionary of function names (key) and shell commands to execute (values).
        deps: A list of dependencies that allows override of shortcuts.
        visibility: The visibility of the rule (allows controlling who can see the rule)
    """
    checkout_update_asset(
        name,
        destination = _CHECKOUT_SHELL_SPACES_TOML,
        value = {
            "shortcuts": shortcuts,
        },
        deps = deps,
        visibility = visibility,
    )

def checkout_add_any_assets(
        name: str,
        assets: list[dict],
        deps: list[str] = [],
        type: str | None = None,
        platforms: list[str] | None = None,
        visibility: str | dict[str, list[str]] | None = None):
    """
    Adds a list of any assets to the workspace as a single rule.

    `assets` should be a list of dicts. Use asset.star: asset_hard_link(), asset_soft_link(), etc to create the entries.

    Args:
        name: The name of the rule.
        assets: A list of dict's that define assets to add.
        deps: List of dependencies for the rule.
        type: use [checkout_type_optional()](#checkout_type_optional) to skip rule checkout
        platforms: List of [platforms](/docs/builtins/#rule-options) rule applies to.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visbility.star for more info.
    """

    checkout.add_any_assets(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "type": type,
            "visibility": visibility,
        },
        assets = {"any": assets},
    )

def checkout_add_env_vars(
        name: str,
        vars: list[dict],
        deps: list[str] = [],
        type: str | None = None,
        platforms: list[str] | None = None,
        visibility: str | dict[str, list[str]] | None = None):
    """
    Adds environment variables to the workspace.

    Args:
        name: Name of the checkout rule
        vars: list of env objects from env.star
        deps: List of dependencies for the rule.
        type: use [checkout_type_optional()](#checkout_type_optional) to skip rule checkout
        platforms: List of [platforms](/docs/builtins/#rule-options) rule applies to.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visbility.star for more info.
    """

    checkout.add_env_vars(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "type": type,
            "visibility": visibility,
        },
        any_env = {
            "vars": vars,
        },
    )

def checkout_store_value(name: str, value, path: str | None = None):
    """
    Stores a value that can be retrieved using workspace_load_value().

    Values are persisted across checkout and run phases, allowing data
    computed during checkout to be accessed later. The value can be any
    serializable type (dict, list, string, int, float, bool, or None).

    Args:
        name: The key to store the value under.
        value: The value to store. Can be any type.
        path: Optional path to store under. When omitted, the member
            path for the calling module is used.
    """

    if path:
        checkout.store_value(name, value, path = path)
    else:
        checkout.store_value(name, value)

def checkout_add_home_store_env(name: str):
    """
    Assigns HOME to a user specific location in the spaces store.

    Args:
        name: Name of the checkout rule
    """

    checkout_add_env_vars(
        name,
        vars = [
            env_assign(
                "HOME",
                workspace.get_absolute_path() + "/" + workspace.get_path_to_home(),
                help = "Assigns HOME to a user specific location in the spaces store",
            ),
        ],
    )

def checkout_add_home_assets(name: str, assets: list[str]):
    """
    Adds home assets to the workspace.

    Each entry in `assets` is a path relative to $HOME. The file is copied into the spaces store
    under .spaces/store/home/$USER/<source> and hard-linked into the workspace at the same relative path.

    Args:
        name: Name of the checkout rule
        assets: list of paths relative to $HOME (e.g. [".ssh/config"])
    """
    checkout_add_any_assets(
        name,
        assets = [asset_home(source) for source in assets],
    )
