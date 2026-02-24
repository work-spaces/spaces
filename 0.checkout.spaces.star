"""
Load the spaces starlark SDK and packages repositories.
"""

# Ensure tools checked out to sysroot/bin are available
# during checkout_add_exec() calls
checkout.add_env_vars(
    rule = {"name": "sysroot_env_path"},
    any_env = {
        "vars": [
            {
                "name": "PATH",
                "value": {
                    "Prepend": {
                        "value": "{}/sysroot/bin".format(workspace.get_absolute_path()),
                        "separator": ":",
                    },
                },
            },
        ],
    },
)

checkout.add_repo(
    rule = {"name": "@star/sdk"},
    repo = {
        "url": "https://github.com/work-spaces/sdk",
        "rev": "c9d2a811ca9bd23da77819ebe0265a82c526c4ae",
        "checkout": "Revision",
        "clone": "Default",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        "rev": "69a4c964219b1be8a9ba35ee457c5b5addfed9c9",
        "checkout": "Revision",
        "clone": "Default",
    },
)
