"""
Helper to create dicts for passing to checkout_add_any_assets()
"""

ASSET_HARD_LINK = "HardLink"
ASSET_SOFT_LINK = "SoftLink"
ASSET_WHICH = "Which"
ASSET_CONTENT = "Asset"
ASSET_HOME = "Home"

def asset_hard_link(source: str, destination: str) -> dict:
    """
    Creates a hard-link asset that can be passed in the assets list to checkout_add_any_assets()

    Args:
        source: source of the hard link
        destination: destination of the hard link

    Returns:
        dict that can be passed to checkout_add_any_assets()
    """

    return {
        "type": ASSET_HARD_LINK,
        "source": source,
        "destination": destination,
    }

def asset_soft_link(source: str, destination: str) -> dict:
    """
    Creates a soft-link asset that can be passed in the assets list to checkout_add_any_assets()

    Args:
        source: source of the hard link
        destination: destination of the hard link

    Returns:
        dict that can be passed to checkout_add_any_assets()
    """

    return {
        "type": ASSET_SOFT_LINK,
        "source": source,
        "destination": destination,
    }

def asset_content(content: str, destination: str) -> dict:
    """
    Creates an asset (file from a starlark string) that can be passed in the assets list to checkout_add_any_assets()

    Args:
        content: content for populating the asset
        destination: destination of the asset

    Returns:
        dict that can be passed to checkout_add_any_assets()
    """

    return {
        "type": ASSET_CONTENT,
        "content": content,
        "destination": destination,
    }

def asset_which(which: str, destination: str) -> dict:
    """
    Creates an asset by using `which` that can be passed in the assets list to checkout_add_any_assets()

    Args:
        which: argument to pass to `which` to discover the program
        destination: destination of the asset

    Returns:
        dict that can be passed to checkout_add_any_assets()
    """

    return {
        "type": ASSET_WHICH,
        "which": which,
        "destination": destination,
    }

def asset_home(source: str) -> dict:
    """
    Creates an asset by copying a file from $HOME into the spaces store and hard-linking it into the workspace.

    The file is stored under .spaces/store/home/$USER/<source> and linked into the workspace at the same
    relative path as source.

    Args:
        source: path relative to $HOME of the file to copy (e.g. ".ssh/config")

    Returns:
        dict that can be passed to checkout_add_any_assets()
    """

    return {
        "type": ASSET_HOME,
        "source": source,
    }
