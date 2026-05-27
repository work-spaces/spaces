"""
Load the spaces starlark SDK and packages repositories.
"""

workspace.set_locks(locks = {
    "@star/sdk": "v0.3.31",
    "@star/packages": "5f8fdb5882ac36e8883145067693c12acbd03399",
})

# Ensure tools checked out to sysroot/bin are available
# during checkout_add_exec() calls
checkout.add_env_vars(
    rule = {"name": "sysroot_env_path"},
    any_env = {
        "vars": [{
            "name": "PATH",
            "help": "Add sysroot/bin to the PATH",
            "value": {
                "Append": {
                    "value": "{}/sysroot/bin".format(workspace.get_absolute_path()),
                    "separator": ":",
                },
            },
        }],
    },
)

checkout.add_repo(
    rule = {"name": "@star/sdk"},
    repo = {
        "url": "https://github.com/work-spaces/sdk",
        "rev": "main",
        "checkout": "Revision",
        "clone": "Default",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        "rev": "main",
        "checkout": "Revision",
        "clone": "Default",
    },
)
