"""
Load the spaces starlark SDK and packages repositories.
"""

workspace.set_locks(locks = {
    "@star/sdk": "f989c2db7fdbc94c3599929e624413ed725f8a2a",
    "@star/packages": "v0.2.45",
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
        "rev": "aec9bda5b07f9f52d580e6468306814dc93dfeb6",
        "checkout": "Revision",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
        "rev": "5fceffdf208bfe29b4ac77b7826639520bab9989",
        "checkout": "Revision",
    },
)
