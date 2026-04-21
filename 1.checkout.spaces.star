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
load("//@star/sdk/star/asset.star", "asset_hard_link")
load(
    "//@star/sdk/star/checkout.star",
    "checkout_add_any_assets",
    "checkout_add_env_vars",
    "checkout_add_home_assets",
    "checkout_add_home_store_env",
    "checkout_add_repo",
    "checkout_clone_default",
    "checkout_set_sandbox",
    "checkout_store_value",
)
load("//@star/sdk/star/env.star", "env_assign")
load(
    "//@star/sdk/star/info.star",
    "info_get_path_to_store",
    "info_is_ci",
    "info_is_platform_linux",
)
load(
    "//@star/sdk/star/sandbox.star",
    "sandbox_configure_for_os",
    "sandbox_new",
)
load(
    "//@star/sdk/star/ws.star",
    "workspace_get_absolute_path",
    "workspace_get_path_to_checkout",
    "workspace_load_value",
)

# Configure the top level workspace

SPACES_CHECKOUT_PATH = workspace_get_path_to_checkout()

spaces_add_devutils(
    "spaces0",
    "v0.15.34",
    devutils_version = "devutils-v0.1.12",
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

rust_add(
    "rust_toolchain",
    version = "1.93",
    deps = [":spaces0"],
    rust_toolchain_toml_dir = "//spaces",
)

RUST_TOOLCHAIN_SOURCE = "{}/musl.rust-toolchain.toml".format(SPACES_CHECKOUT_PATH) if info_is_ci() and info_is_platform_linux() else "{}/default.rust-toolchain.toml".format(SPACES_CHECKOUT_PATH)

checkout_add_any_assets(
    "cargo_workspace_assets",
    assets = [
        asset_hard_link(
            source = RUST_TOOLCHAIN_SOURCE,
            destination = "rust-toolchain.toml",
        ),
        asset_hard_link(
            source = "{}/Cargo.workspace.toml".format(SPACES_CHECKOUT_PATH),
            destination = "Cargo.toml",
        ),
    ],
)

if not info_is_ci():
    sccache_add(
        "sccache",
        version = "0.14",
    )
else:
    checkout_add_env_vars(
        "sscache_env",
        vars = [
            env_assign(
                "SCCACHE_DIR",
                value = "",
                help = "populate SCCACHE_DIR to avoid errors in CI",
            ),
        ],
    )

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
        env_assign(
            "GIT_CONFIG_GLOBAL",
            workspace_get_absolute_path() + "/.spaces/.gitconfig",
            help = "Assign git config to workspace home fholder",
        ),
    ],
)

GH_RULE = package_add("github.com", "cli", "cli", "v2.88.1")

# Required for dbus and nono (linux only)

if info_is_platform_linux() or workspace_load_value("SPACES_ENABLE_DBUS") == "ON":
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
        rev = "2d4506d18527b430c77116bd5832a284c378f584",
        clone = checkout_clone_default(),
    )

if workspace_load_value("SPACES_ENABLE_SANDBOX") == True:
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

    sandbox = sandbox_new("workspace-sandbox")
    sandbox_configure_for_os(sandbox)
    checkout_set_sandbox(sandbox)
