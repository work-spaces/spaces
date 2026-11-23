"""
Load the spaces starlark SDK and packages repositories.
"""

load("//@star/prelude/rules/checkout.star", "checkout_add_env_vars", "checkout_add_repo")
load("//@star/prelude/rules/env.star", "env_append")

workspace.set_locks(locks = {
    "@star/sdk": "71bb17030a88c69e4a8970d26c60c96cec62912c",
    "@star/packages": "f1e59df56969189ce31619bf549b372dcdfcc7e2",
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
