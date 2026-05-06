"""
Load the spaces starlark SDK and packages repositories.
"""

workspace.set_locks(locks = {
    "@star/sdk": "v0.3.30",
    "@star/packages": "v0.2.48",
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
        "rev": "5bb98b3843598df995375e53f0837e07027af0dd",
        "checkout": "Revision",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        "rev": "main",
        "checkout": "Revision",
    },
)
