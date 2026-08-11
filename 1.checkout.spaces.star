"""
Spaces starlark checkout/run script to make changes to spaces, printer, and archiver.
With VSCode/Zed integration
"""

load("//@star/packages/star/cmake.star", "cmake_add")
load("//@star/packages/star/musl-gcc.star", "musl_gcc_add", "musl_gcc_add_toolchain_file")
load("//@star/packages/star/package.star", "package_add")
load("//@star/packages/star/rust.star", "rust_add")
load("//@star/packages/star/sccache.star", "sccache_add")
load("//@star/packages/star/spaces-cli.star", "spaces_add_devutils", "spaces_add_star_formatter")
load("//@star/packages/star/starship.star", "starship_add_bash")
load(
    "//@star/prelude/info.star",
    "info_get_path_to_store",
    "info_is_ci",
    "info_is_platform_linux",
)
load("//@star/prelude/rules/asset.star", "asset_hard_link")
load(
    "//@star/prelude/rules/checkout.star",
    "checkout_add_any_assets",
    "checkout_add_env_vars",
    "checkout_add_home_assets",
    "checkout_add_home_store_env",
    "checkout_add_repo",
    "checkout_clone_default",
    "checkout_store_value",
)
load("//@star/prelude/rules/env.star", "env_assign", "env_inherit")
load(
    "//@star/prelude/rules/ws.star",
    "workspace_get_absolute_path",
    "workspace_get_path_to_checkout",
    "workspace_load_value",
)
load(
    "//@star/sdk/star/info.star",
    "info_get_path_to_store",
    "info_is_ci",
    "info_is_platform_linux",
)
#load(
#     "//@star/sdk/star/sandbox.star",
#     "sandbox_configure_for_os",
#     "sandbox_new",
#)

# Configure the top level workspace

SPACES_CHECKOUT_PATH = workspace_get_path_to_checkout()

spaces_add_devutils(
    "spaces0",
    "v0.20.7",
    devutils_version = "devutils-v0.1.15",
    system_paths = ["/usr/bin", "/bin"],
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

RUST_TOOLCHAIN_SOURCE = "{}/musl.rust-toolchain.toml".format(SPACES_CHECKOUT_PATH) if info_is_ci() and info_is_platform_linux() else "{}/default.rust-toolchain.toml".format(SPACES_CHECKOUT_PATH)

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
    ],
)

rust_add(
    "rust_toolchain",
    version = "1.94",
    deps = [":spaces0", ":cargo_workspace_assets"],
)

if not info_is_ci():
    sccache_add(
        "sccache",
        version = "0.17",
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
        env_assign(
            "SPACES_PRINTER_SKIP_SDK_CHECKOUT",
            "TRUE",
            help = "Skip SDK checkout for printer",
        ),
        env_assign(
            "SPACES_ARCHIVER_SKIP_SDK_CHECKOUT",
            "TRUE",
            help = "Skip SDK checkout for archiver",
        ),
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

# This can be used for testing spaces sync
if workspace.load_value("CHECKOUT_INSTALL_SPACES") == "ON":
    checkout_add_repo(
        "install-spaces",
        url = "https://github.com/work-spaces/install-spaces",
        rev = "main",
    )

package_add("github.com", "cli", "cli", "v2.88.1")

# Required for dbus and nono (linux only)

if info_is_platform_linux() or workspace_load_value("SPACES_ENABLE_SANDBOX") == "ON":
    checkout_store_value("SPACES_DBUS_ENABLED", True)
    cmake_add("cmake4", "v4.3.1")
    package_add("github.com", "ninja-build", "ninja", "v1.13.2")
    package_add("github.com", "xpack-dev-tools", "pkg-config-xpack", "v0.29.2-3")

    if info_is_platform_linux():
        musl_gcc_add("musl_gcc")
        musl_gcc_add_toolchain_file(
            "musl_gcc_toolchain",
            "sysroot/share/cmake/musl-toolchain.cmake",
        )

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

    #if not info_is_ci():
    # sandbox = sandbox_new("workspace-sandbox")
    # sandbox_configure_for_os(sandbox)
    # checkout_set_sandbox(sandbox)
