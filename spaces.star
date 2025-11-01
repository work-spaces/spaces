"""
Spaces starlark checkout script to make changes to spaces, printer, and easy-archiver.
With VSCode/Zed integration
"""

load("//@star/packages/star/buildifier.star", "buildifier_add")
load("//@star/packages/star/rust.star", "rust_add")
load("//@star/packages/star/sccache.star", "sccache_add")
load("//@star/packages/star/starship.star", "starship_add_bash")
load(
    "//@star/sdk/star/checkout.star",
    "checkout_add_asset",
    "checkout_add_hard_link_asset",
    "checkout_add_repo",
    "checkout_update_asset",
)
load(
    "//@star/sdk/star/run.star",
    "run_add_exec",
    "run_add_exec_test",
)
load("//@star/sdk/star/shell.star", "shell")
load("//@star/sdk/star/spaces-env.star", "spaces_working_env")
load(
    "//@star/sdk/star/ws.star",
    "workspace_get_absolute_path",
    "workspace_get_env_var",
    "workspace_get_path_to_checkout",
    "workspace_is_env_var_set",
)

# Configure the top level workspace

SPACES_CHECKOUT_PATH = workspace_get_path_to_checkout()

SHORTCUTS = {
    "inspect": "spaces inspect",
    "install_dev": "spaces run //spaces:install_dev",
    "install_dev_lsp": "spaces run //spaces:install_dev_lsp",
    "install_release": "spaces run //spaces:install_release",
    "clippy": "spaces run //spaces:clippy",
    "format": "spaces run //spaces:format",
}


starship_add_bash("starship0", shortcuts = SHORTCUTS)

# This is needed for easy-archiver to pickup the local version of printer
checkout_update_asset(
    "cargo_config",
    destination = ".cargo/config.toml",
    value = {
        "patch": {
            "https://github.com/work-spaces/printer-rs": {
                "printer": {
                    "path": "./printer",
                },
            },
            "https://github.com/work-spaces/easy-archiver": {
                "easy-archiver": {
                    "path": "./easy-archiver",
                },
            },
        },
    },
)

# Add spaces, printer, and easy-archiver source repositories to the workspace

printer_url = "https://github.com/work-spaces/printer-rs"
easy_archiver_url = "https://github.com/work-spaces/easy-archiver"

checkout_add_hard_link_asset(
    "rust_toolchain",
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
    "easy-archiver",
    url = easy_archiver_url,
    rev = "main",
)

rust_add(
    "rust_toolchain",
    version = "1.80",
)

buildifier_add(
    "buildifier",
    version = "v8.2.1",
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

spaces_store = info.get_path_to_store()

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

checkout_update_asset(
    "zed_settings",
    destination = ".zed/settings.json",
    value = {
        "lsp": {
            "rust-analyzer": {
                "initialization_options": {
                    "cargo": {
                        "features": [],
                    },
                },
            },
        },
        "languages": {
            "Starlark": {
                "language_servers": ["!spaces-lsp", "!buck2-lsp", "!starpls", "!tilt"],
                "tab_size": 4,
            },
        },
    },
)

run_add_exec(
    "check",
    command = "cargo",
    args = ["check"],
    help = "Run cargo check on workspace",
)

run_add_exec(
    "build",
    command = "cargo",
    args = ["build"],
    help = "Run cargo build on workspace",
)

run_add_exec(
    "clippy",
    command = "cargo",
    args = ["clippy"],
    log_level = "Passthrough",
    help = "Run cargo clippy on workspace",
)

run_add_exec(
    "format",
    command = "cargo",
    args = ["fmt"],
    log_level = "Passthrough",
    help = "Run cargo fmt on workspace",
)

run_add_exec_test(
    "capsule_test",
    command = "cargo",
    args = [
        "test",
        "--package=capsule",
        "--",
        "--test-threads=1",  # Tests share state (heap) and can't be multithreaded
    ],
    env = {
        "RUST_BACKTRACE": "1",
        "RUST_LOG": "trace",
    },
)

SPACES_INSTALL_ROOT = "SPACES_INSTALL_ROOT"

if workspace_is_env_var_set(SPACES_INSTALL_ROOT):
    root = workspace_get_env_var(SPACES_INSTALL_ROOT)
else:
    root = "~/.local"

shell(
    "install_dev",
    script = "cargo install --force --path=spaces/crates/spaces --profile=dev --root={}".format(root),
)

shell(
    "install_release",
    script = "cargo install --force --path=spaces/crates/spaces --profile=release --root={}".format(root),
)

shell(
    "install_dev_lsp",
    script = "cargo install --features=lsp --force --path=spaces/crates/spaces --profile=dev --root={}".format(root),
)

spaces_working_env(add_spaces_to_sysroot = True, inherit_terminal = True)
