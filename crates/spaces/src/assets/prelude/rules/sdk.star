"""
SDK Repository Management

Provides helper functions to manage a shared SDK repository across workspace
members without duplicate checkouts.
"""

load(
    "checkout.star",
    "CHECKOUT_CLONE_DEFAULT",
    "CHECKOUT_CLONE_TYPE_REVISION",
    "checkout_add_repo",
    "checkout_store_value",
)
load(
    "ws.star",
    "workspace_get_path_to_checkout",
    "workspace_is_path_to_member_available",
    "workspace_load_value",
)

_SDK_NAMESPACE = "//@star/prelude/rules/sdk"
_SDK_OWNER_KEY = "owner"

def sdk_add_repo(
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
    Adds an SDK repo and associates with the calling repo.

    If the SDK is already in the workspace, no action is taken.

    Args:
        name: The name of the rule. This is also the location of the new repo in the workspace.
        url: The git repository URL to clone.
        rev: The branch or commit hash to checkout (can be overridden by workspace store value `PRELUDE_SDK_REV`).
        checkout_type: Revision (default is `CHECKOUT_CLONE_TYPE_REVISION`).
        clone: Clone mode: [checkout_clone_default()](#checkout_clone_default) | [checkout_clone_worktree()](#checkout_clone_worktree) | [checkout_clone_shallow()](#checkout_clone_shallow).
        is_evaluate_spaces_modules: Whether to evaluate spaces.star files in the repo (default is True).
        sparse_mode: Cone | NoCone.
        sparse_list: List of paths to include/exclude.
        working_directory: The working directory to clone the repository into.
        platforms: List of platforms to add the repo to.
        type: Use [checkout_type_optional()](#checkout_type_optional) to skip rule checkout.
        deps: List of dependencies for the rule.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.
    """
    if workspace_is_path_to_member_available(url):
        # No-op. The SDK is already avilable in th workspace
        return

    current_module_path = workspace_get_path_to_checkout()
    checkout_store_value(_SDK_OWNER_KEY, current_module_path, path = _SDK_NAMESPACE)
    effective_rev = workspace_load_value("PRELUDE_SDK_REV") or rev

    checkout_add_repo(
        name,
        url = url,
        rev = effective_rev,
        checkout_type = checkout_type,
        clone = clone,
        is_evaluate_spaces_modules = is_evaluate_spaces_modules,
        sparse_mode = sparse_mode,
        sparse_list = sparse_list,
        working_directory = working_directory,
        platforms = platforms,
        type = type,
        deps = deps,
        visibility = visibility,
    )

def sdk_finalize_checkout(sdk_checkout):
    """
    Executes the sdk_checkout callback as a lambda if this repo owns the SDK.

    Args:
        sdk_checkout: A lambda function (with no arguments) executed if the caller is the SDK owner.
    """
    current_module_path = workspace_get_path_to_checkout()
    sdk_module_path = workspace_load_value(_SDK_OWNER_KEY, path = _SDK_NAMESPACE)
    if sdk_module_path == current_module_path:
        sdk_checkout()
