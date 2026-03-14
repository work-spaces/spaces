"""
Spaces starlark checkout/run script to make changes to spaces, printer, and archiver.
With VSCode/Zed integration
"""

load("//@star/packages/star/rust.star", "rust_add")
load("//@star/packages/star/sccache.star", "sccache_add")
load("//@star/packages/star/spaces-cli.star", "spaces_add_devutils", "spaces_add_star_formatter")
load("//@star/packages/star/starship.star", "starship_add_bash")
load(
    "//@star/sdk/star/checkout.star",
    "checkout_add_env_vars",
    "checkout_add_hard_link_asset",
    "checkout_add_repo",
    "checkout_update_asset",
)
load("//@star/sdk/star/env.star", "env_assign")
load(
    "//@star/sdk/star/info.star",
    "info_get_path_to_store",
    "info_is_ci",
)
load(
    "//@star/sdk/star/ws.star",
    "workspace_get_absolute_path",
    "workspace_get_path_to_checkout",
)

# Configure the top level workspace

SPACES_CHECKOUT_PATH = workspace_get_path_to_checkout()

spaces_add_devutils(
    "spaces0",
    "v0.15.28",
    devutils_version = "devutils-v0.1.5",
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

# Add spaces, printer, and archiver source repositories to the workspace
printer_url = "https://github.com/work-spaces/spaces-printer"
archiver_url = "https://github.com/work-spaces/spaces-archiver"

# This is needed for spaces-archiver to pickup the local version of printer
checkout_update_asset(
    "cargo_config",
    destination = ".cargo/config.toml",
    value = {
        "patch": {
            "crates-io": {
                "printer": {
                    "package": "spaces-printer",
                    "path": "./printer",
                },
                "archiver": {
                    "package": "spaces-archiver",
                    "path": "./archiver",
                },
            },
        },
    },
)

checkout_add_hard_link_asset(
    "rust_toolchain_toml",
    source = "{}/rust-toolchain.toml".format(SPACES_CHECKOUT_PATH),
    destination = "rust-toolchain.toml",
)

checkout_add_hard_link_asset(
    "cargo_workspace_toml",
    source = "{}/Cargo.workspace.toml".format(SPACES_CHECKOUT_PATH),
    destination = "Cargo.toml",
)

checkout_add_repo(
    "printer",
    url = printer_url,
    rev = "main",
)

checkout_add_repo(
    "archiver",
    url = archiver_url,
    rev = "main",
)

sccache_add(
    "sccache",
    version = "0.8",
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

checkout_update_asset(
    "vscode_tasks",
    destination = ".vscode/tasks.json",
    value = {
        "options": task_options,
        "tasks": [
            cargo_vscode_task | {
                "command": "build",
                "args": ["--manifest-path=spaces/Cargo.toml"],
                "label": "build:spaces",
            },
            cargo_vscode_task | {
                "command": "install",
                "args": ["--path=spaces/crates/spaces", "--root=${userHome}/.local", "--profile=dev"],
                "label": "install_dev:spaces",
            },
            cargo_vscode_task | {
                "command": "install",
                "args": ["--path=spaces/crates/spaces", "--root=${userHome}/.local", "--profile=release"],
                "label": "install:spaces",
            },
        ],
    },
)

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
    ],
)
