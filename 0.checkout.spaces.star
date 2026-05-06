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
<<<<<<< HEAD
        "rev": "main",
=======
        "rev": "aec9bda5b07f9f52d580e6468306814dc93dfeb6",
>>>>>>> a79a6ec (#705. Add support for sandboxing with nono)
        "checkout": "Revision",
    },
)

checkout.add_repo(
    rule = {"name": "@star/packages"},
    repo = {
        "url": "https://github.com/work-spaces/packages",
<<<<<<< HEAD
        "rev": "main",
=======
        "rev": "5fceffdf208bfe29b4ac77b7826639520bab9989",
>>>>>>> a79a6ec (#705. Add support for sandboxing with nono)
        "checkout": "Revision",
    },
)
