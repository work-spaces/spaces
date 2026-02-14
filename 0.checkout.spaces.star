"""
Load the spaces starlark SDK and packages repositories.
"""

# Ensure tools checked out to sysroot/bin are available
# during checkout_add_exec() calls
checkout.update_env(
    rule = {"name": "sysroot_env_path"},
    env = {
        "paths": ["{}/sysroot/bin".format(workspace.get_absolute_path())],
    },
)

checkout.add_repo(
    rule = {"name": "@star/sdk"},
    repo = {
        "url": "https://github.com/work-spaces/sdk",
        "rev": "v0.3.22",
        "checkout": "Revision",
        "clone": "Default",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        "rev": "6fcc9f6167acf0df7468d7b43e4a1b76301da68a",
        "checkout": "Revision",
        "clone": "Default",
    },
)
