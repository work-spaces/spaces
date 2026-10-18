"""
Load the spaces starlark SDK and packages repositories.
"""

load("//@star/prelude/rules/checkout.star", "checkout_add_env_vars", "checkout_add_repo")
load("//@star/prelude/rules/env.star", "env_append")

workspace.set_locks(locks = {
    "@star/sdk": "030c97fe5e99638ea9390c4561380e263b1a5efb",
    "@star/packages": "05d95f0428280a33d0f19865cefbdec34deeea01",
})

# Ensure tools checked out to sysroot/bin are available
# during checkout_add_exec() calls
checkout_add_env_vars(
    "sysroot_env_path",
    vars = [
        env_append("PATH", "{}/sysroot/bin".format(workspace.get_absolute_path()), help = "Add sysroot/bin to the PATH"),
    ],
)

checkout_add_repo(
    "@star/sdk",
    url = "https://github.com/work-spaces/sdk",
    rev = "main",
)

checkout_add_repo(
    "@star/packages",
    url = "https://github.com/work-spaces/packages",
    rev = "main",
)
