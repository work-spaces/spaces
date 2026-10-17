"""
Spaces starlark checkout/run script to make changes to spaces, printer, and archiver.
With VSCode/Zed integration
"""

load("//@star/packages/star/cmake.star", "cmake_add")
load("//@star/packages/star/musl-gcc.star", "musl_gcc_add", "musl_gcc_add_toolchain_file")
load("//@star/packages/star/package.star", "package_add")
load("//@star/packages/star/rust.star", "rust_add")
load("//@star/packages/star/spaces-cli.star", "spaces_add_devutils", "spaces_add_star_formatter")
load("//@star/packages/star/starship.star", "starship_add_bash")
load(
    "//@star/prelude/info.star",
    "info_get_path_to_store",
    "info_is_ci",
    "info_is_platform_linux",
)
load("//@star/prelude/rules/asset.star", "asset_hard_link", "asset_soft_link")
load(
    "//@star/prelude/rules/checkout.star",
    "checkout_add_any_assets",
    "checkout_add_env_vars",
    "checkout_add_home_assets",
    "checkout_add_home_store_env",
    "checkout_add_repo",
    "checkout_clone_default",
    "checkout_set_sandbox",
    "checkout_store_value",
)
load("//@star/prelude/rules/env.star", "env_assign", "env_inherit")

# Sandbox needs support in CI before this will work
load(
    "//@star/prelude/rules/sandbox.star",
    "sandbox_configure_for_os",
    "sandbox_new",
)
load(
    "//@star/prelude/rules/ws.star",
    "workspace_get_absolute_path",
    "workspace_get_path_to_checkout",
)
load(
    "//@star/sdk/star/checkout-config.star",
    "checkout_config_load_option",
)
load(
    "//@star/sdk/star/skill.star",
    "skill_register_checkout",
    "skill_register_checkout_spaces_skills",
)
load(
    "star/internal/config.star",
    "SPACES_CHECKOUT_INSTALL_SPACES_CONFIG_KEY",
    "SPACES_DEV_SKILL_CONFIG_KEY",
    "SPACES_ENABLE_SANDBOX_CONFIG_KEY",
    "config_register_checkout",
)

# Configure the top level workspace

SPACES_CHECKOUT_PATH = workspace_get_path_to_checkout()

spaces_add_devutils(
    "spaces0",
    "v0.22.2",
    devutils_version = "devutils-v0.1.16",
    system_paths = ["/usr/bin", "/bin"],
    is_activate_sccache = not info_is_ci(),
)

spaces_add_star_formatter("star_formatter", configure_zed = True, deps = [":spaces0"])

if not info_is_ci():
    SHORTCUTS = {
        "inspect": "spaces inspect",
        "install_dev": "spaces run //spaces:install_dev",
        "install_dev_lsp": "spaces run //spaces:install_dev_lsp",
        "install_release": "spaces run //spaces:install_release",
        "clippy": "spaces run //spaces:clippy",
        "format": "spaces run //spaces:format",
    }

    starship_add_bash(
        "starship0",
        shortcuts = SHORTCUTS,
        install_binary = False,
        deps = [":spaces0"],
    )

RUST_TOOLCHAIN_SOURCE = "{}/musl.rust-toolchain.toml".format(SPACES_CHECKOUT_PATH) if info_is_platform_linux() else "{}/default.rust-toolchain.toml".format(SPACES_CHECKOUT_PATH)

# Checkout configuration

config_register_checkout()

# This can be used for testing spaces sync
if checkout_config_load_option(SPACES_CHECKOUT_INSTALL_SPACES_CONFIG_KEY):
    checkout_add_repo(
        "install-spaces",
        url = "https://github.com/work-spaces/install-spaces",
        rev = "main",
    )

# Required for dbus and nono (linux only)
if info_is_platform_linux() or checkout_config_load_option(SPACES_ENABLE_SANDBOX_CONFIG_KEY):
    checkout_store_value("SPACES_DBUS_ENABLED", True)
    cmake_add("cmake4", "v4.3.1")
    package_add("github.com", "ninja-build", "ninja", "v1.13.2")
    package_add("github.com", "xpack-dev-tools", "pkg-config-xpack", "v0.29.2-3")

    checkout_add_repo(
        "deps/libexpat",
        url = "https://github.com/libexpat/libexpat",
        rev = "R_2_7_5",
        clone = checkout_clone_default(),
    )

    checkout_add_repo(
        "deps/dbus",
        url = "https://github.com/work-spaces/dbus.git",
        rev = "ff0666ad9ad4d996d2de6a257ade5244b623510c",
        clone = checkout_clone_default(),
    )

# Skills
skill_register_checkout(
    "spaces_dev",
    path = "skills/spaces-dev",
    config_key = SPACES_DEV_SKILL_CONFIG_KEY,
)

skill_register_checkout_spaces_skills()

checkout_add_any_assets(
    "cargo_workspace_assets",
    assets = [
        asset_hard_link(
            source = RUST_TOOLCHAIN_SOURCE,
            destination = "//rust-toolchain.toml",
        ),
        asset_hard_link(
            source = "{}/Cargo.workspace.toml".format(SPACES_CHECKOUT_PATH),
            destination = "//Cargo.toml",
        ),
        asset_soft_link(
            source = "{}/spaces/skills/spaces-dev".format(workspace_get_absolute_path()),
            destination = ".agents/skills/spaces-dev",
        ),
    ],
)

rust_add(
    "rust_toolchain",
    version = None,  # use rust-toolchain.toml
    deps = [":spaces0", ":cargo_workspace_assets"],
)

package_add("github.com", "cli", "cli", "v2.97.0")

cargo_vscode_task = {
    "type": "cargo",
    "problemMatcher": ["$rustc"],
    "group": "build",
}

spaces_store = info_get_path_to_store()

task_options = {
    "env": {
        "CARGO_HOME": "{}/cargo".format(spaces_store),
        "RUSTUP_HOME": "{}/rustup".format(spaces_store),
        "RUSTFLAGS": "--remap-path-prefix={}/=".format(workspace_get_absolute_path()),
    },
}

checkout_add_env_vars(
    "spaces_env",
    vars = [
        env_inherit(
            "GH_TOKEN",
            is_secret = True,
            is_required = info_is_ci(),
            help = "Allows access to gh in Spaces rules",
        ),
    ],
)

if info_is_platform_linux():
    musl_gcc_add("musl_gcc")
    musl_gcc_add_toolchain_file(
        "musl_gcc_toolchain",
        "sysroot/share/cmake/musl-toolchain.cmake",
    )

    if info_is_platform_linux():
        pkg_config_vars = [
            env_assign(
                "PKG_CONFIG_PATH",
                workspace_get_absolute_path() + "/build/install/lib/pkgconfig",
                help = "pkg-config path for building dbus with cargo",
            ),
            env_assign(
                "PKG_CONFIG_ALLOW_CROSS",
                "1",
                help = "Allow pkg-config to work for cross-compilation without sysroot",
            ),
        ]

        checkout_add_env_vars(
            "pkg_config_env",
            vars = pkg_config_vars,
        )

checkout_add_home_store_env("home_store_env")
checkout_add_home_assets(
    "home_assets",
    assets = [
        ".gitconfig",
        ".config/gh",
        ".ssh",
        ".gnupg",
        ".config/git",
        ".netrc",
    ],
)

if not info_is_ci():
    sandbox = sandbox_new("workspace-sandbox")
    sandbox_configure_for_os(sandbox)
    checkout_set_sandbox(sandbox)
