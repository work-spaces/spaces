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
        "rev": "6b0990f79cdcf251037a456327c7525a213d9b0c",
        "checkout": "Revision",
        "clone": "Default",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        "rev": "0ee78450af33f913f65da16968714f25925cee7d",
        "checkout": "Revision",
        "clone": "Default",
    },
)
