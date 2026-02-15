"""
Spaces starlark checkout/run script to make changes to spaces, printer, and archiver.
With VSCode/Zed integration
"""

load(
    "//@star/sdk/star/run.star",
    "run_add_exec",
    "run_add_exec_test",
    "run_add_target",
)
load("//@star/sdk/star/shell.star", "shell")
load(
    "//@star/sdk/star/visibility.star",
    "visibility_private",
    "visibility_rules",
)
load(
    "//@star/sdk/star/ws.star",
    "workspace_get_env_var",
    "workspace_is_env_var_set",
)

run_add_exec(
    "check",
    command = "cargo",
    args = ["check"],
    help = "Run cargo check on workspace",
    visibility = visibility_private(),
)

run_add_exec(
    "build",
    command = "cargo",
    inputs = [
        "+//spaces/Cargo.toml",
        "+//spaces/Cargo.workspace.toml",
        "+//spaces/**/*.rs",
        "-//spaces/target/**",
        "+//printer/**/*.rs",
        "+//archiver/**/*.rs",
    ],
    args = ["build"],
    visibility = visibility_private(),
    help = "Run cargo build on workspace",
)

run_add_target(
    "test_vis",
    deps = [":build"],
    visibility = visibility_private(),
)

run_add_exec(
    "clippy",
    command = "cargo",
    args = ["clippy"],
    log_level = "Passthrough",
    help = "Run cargo clippy on workspace",
    visibility = visibility_private(),
)

run_add_exec(
    "format",
    command = "cargo",
    args = ["fmt"],
    log_level = "Passthrough",
    help = "Run cargo fmt on workspace",
    visibility = visibility_private(),
)

run_add_exec_test(
    "cargo_test",
    command = "cargo",
    args = [
        "test",
        "--",
        "--test-threads=1",  # Tests share state (heap) and can't be multithreaded
    ],
    env = {
        "RUST_BACKTRACE": "1",
        "RUST_LOG": "trace",
    },
    visibility = visibility_rules(["//:test"]),
)

SPACES_INSTALL_ROOT = "SPACES_INSTALL_ROOT"

if workspace_is_env_var_set(SPACES_INSTALL_ROOT):
    root = workspace_get_env_var(SPACES_INSTALL_ROOT)
else:
    root = "~/.local"

shell(
    "install_dev",
    script = "cargo install --force --path=spaces/crates/spaces --profile=dev --root={}".format(root),
    visibility = visibility_private(),
)

shell(
    "install_release",
    script = "cargo install --force --path=spaces/crates/spaces --profile=release --root={}".format(root),
    visibility = visibility_private(),
)

shell(
    "install_dev_lsp",
    script = "cargo install --features=lsp-debug --force --path=spaces/crates/spaces --profile=dev --root={}".format(root),
    visibility = visibility_private(),
)
