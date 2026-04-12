"""
Spaces starlark checkout/run script to make changes to spaces, printer, and archiver.
With VSCode/Zed integration
"""

load("//@star/sdk/star/deps.star", "deps")
load("//@star/sdk/star/glob.star", "glob")
load(
    "//@star/sdk/star/run.star",
    "run_add_exec",
    "run_add_exec_test",
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

GLOB_DEPS = glob(includes = [
    "//Cargo.toml",
    "//spaces/Cargo.toml",
    "//spaces/Cargo.workspace.toml",
    "//spaces/**/*.rs",
    "//spaces/rust-toolchain.toml",
], excludes = [
    "//spaces/target/**",
])

run_add_exec(
    "check",
    command = "cargo",
    args = ["check"],
    help = "Run cargo check on workspace",
    deps = deps(rules = [":rustup_update"], globs = [GLOB_DEPS]),
    visibility = visibility_private(),
)

run_add_exec(
    "build",
    command = "cargo",
    args = ["build", "--target-dir=build/target"],
    deps = deps(rules = [":rustup_update", ":check"], globs = [GLOB_DEPS]),
    target_files = ["//build/target/debug/spaces"],
    visibility = visibility_private(),
    help = "Run cargo build on workspace",
)

run_add_exec(
    "post_build",
    command = "bash",
    args = ["-c", "echo build changed > build/changed.txt"],
    deps = deps(rules = [":build"]),
    visibility = visibility_private(),
)

run_add_exec(
    "clippy",
    command = "cargo",
    args = ["clippy"],
    log_level = "Passthrough",
    deps = deps(rules = [":rustup_update"], globs = [GLOB_DEPS]),
    help = "Run cargo clippy on workspace",
    visibility = visibility_private(),
)

run_add_exec(
    "format",
    command = "cargo",
    args = ["fmt"],
    log_level = "Passthrough",
    deps = deps(rules = [":rustup_update"], globs = [GLOB_DEPS]),
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
    deps = deps(rules = [":rustup_update"], globs = [GLOB_DEPS]),
    visibility = visibility_rules(["//:test"]),
)

SPACES_INSTALL_ROOT = "SPACES_INSTALL_ROOT"

if workspace_is_env_var_set(SPACES_INSTALL_ROOT):
    root = workspace_get_env_var(SPACES_INSTALL_ROOT)
else:
    root = "{}/.local".format(workspace_get_env_var("HOME"))

run_add_exec(
    "rustup_update",
    command = "rustup",
    args = ["update"],
    deps = deps(files = ["//spaces/rust-toolchain.toml"]),
    help = "Update the Rust toolchain via rustup",
    visibility = visibility_private(),
)

run_add_exec(
    "install_dev",
    command = "bash",
    args = ["-c", "cargo install --force --path=spaces/crates/spaces --profile=dev --root={}".format(root)],
    deps = [":rustup_update"],
    visibility = visibility_private(),
    help = "Install dev build on local system",
)

shell(
    "install_release",
    script = "cargo install --target-dir=build/target --force --path=spaces/crates/spaces --profile=release --root={}".format(root),
    deps = [":rustup_update"],
    visibility = visibility_private(),
)

shell(
    "install_dev_lsp",
    script = "cargo install --target-dir=build/target --features=lsp-debug --force --path=spaces/crates/spaces --profile=dev --root={}".format(root),
    deps = [":rustup_update"],
    visibility = visibility_private(),
)

STARLARK_FILES = [
    "0.checkout.spaces.star",
    "1.checkout.spaces.star",
    "spaces.star",
]

run_add_exec(
    "check_starlark",
    command = "buildifier",
    args = [
        "-lint=warn",
        "-mode=check",
    ] + STARLARK_FILES,
    deps = deps(files = STARLARK_FILES),
    visibility = visibility_private(),
    working_directory = ".",
)

run_add_exec(
    "check_rust_fmt",
    command = "cargo",
    args = ["fmt", "--check"],
    deps = [":rustup_update"],
    visibility = visibility_private(),
)

run_add_exec(
    "check_rust_clippy",
    command = "cargo",
    args = ["clippy"],
    visibility = visibility_private(),
    deps = [":check_rust_fmt", ":check_starlark", ":rustup_update"],
)
