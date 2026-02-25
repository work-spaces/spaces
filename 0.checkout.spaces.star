"""
Load the spaces starlark SDK and packages repositories.
"""

workspace.set_locks(locks = {
    "printer": "8d1c6ca54931eead8f89b9120c4d57d37173b624",
    "archiver": "v0.3.0",
    "@star/sdk": "v0.3.25",
    "@star/packages": "v0.2.40",
})

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
        "rev": "v0.3.25",
        "checkout": "Revision",
        "clone": "Default",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        "rev": "v0.2.40",
        "checkout": "Revision",
        "clone": "Default",
    },
)
