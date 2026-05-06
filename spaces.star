"""
Spaces starlark checkout/run script to make changes to spaces, printer, and archiver.
With VSCode/Zed integration
"""

load("//@star/packages/star/musl-gcc.star", "musl_gcc_get_env")
load("//@star/sdk/star/deps.star", "deps")
load("//@star/sdk/star/glob.star", "glob")
load(
    "//@star/sdk/star/info.star",
    "info_is_platform_aarch64",
    "info_is_platform_linux",
    "info_is_platform_x86_64",
)
load(
    "//@star/sdk/star/run.star",
    "run_add",
    "run_add_exec",
    "run_add_exec_test",
    "run_log_level_passthrough",
)
load(
    "//@star/sdk/star/visibility.star",
    "visibility_private",
    "visibility_rules",
)
load(
    "//@star/sdk/star/ws.star",
    "workspace_get_env_var",
    "workspace_is_env_var_set",
    "workspace_load_value",
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

DBUS_DEPS = ["//spaces/deps:dbus"] if workspace_load_value("SPACES_DBUS_ENABLED") else []

run_add(
    "base_deps",
    deps = [
        ":rustup_update",
    ] + DBUS_DEPS,
)

run_add_exec(
    "check",
    command = "cargo",
    args = ["check"],
    help = "Run cargo check on workspace",
    deps = deps(rules = [":base_deps"], globs = [GLOB_DEPS]),
    visibility = visibility_private(),
)

run_add_exec(
    "build",
    command = "cargo",
    args = ["build", "--target-dir=build/target"],
    deps = deps(
        rules = [":check"],
        globs = [GLOB_DEPS],
        files = [
            "{}/bin/cargo".format(workspace_get_env_var("CARGO_HOME")),
            "{}/bin/rustc".format(workspace_get_env_var("CARGO_HOME")),
        ],
    ),
    target_files = ["//build/target/debug/spaces"],
    visibility = visibility_private(),
    help = "Run cargo build on workspace",
    env = {
        "SCCACHE_DIR": workspace_get_env_var("SCCACHE_DIR"),
        "RUSTUP_HOME": workspace_get_env_var("RUSTUP_HOME"),
        "CARGO_HOME": workspace_get_env_var("CARGO_HOME"),
    },
)

run_add_exec(
    "post_build",
    command = "bash",
    args = ["-c", "echo $(build/target/debug/spaces --version) > build/changed.txt"],
    deps = deps(rules = [":build"]),
    target_files = ["//build/changed.txt"],
    help = "Run a quick post build for tests",
    visibility = visibility_private(),
)

run_add_exec(
    "clippy",
    command = "cargo",
    args = ["clippy"],
    log_level = "Passthrough",
    deps = deps(rules = [":base_deps"], globs = [GLOB_DEPS]),
    help = "Run cargo clippy on workspace",
    visibility = visibility_private(),
)

run_add_exec(
    "format",
    command = "cargo",
    args = ["fmt"],
    log_level = "Passthrough",
    deps = deps(rules = [":base_deps"], globs = [GLOB_DEPS]),
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
    deps = deps(rules = [":base_deps"], globs = [GLOB_DEPS]),
    visibility = visibility_rules(["//:test", "//spaces"]),
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
    deps = deps(files = ["//rust-toolchain.toml"]),
    help = "Update the Rust toolchain via rustup",
    visibility = visibility_private(),
)

run_add_exec(
    "wait",
    command = "sleep",
    args = ["200"],
    help = "Wait for 200 seconds",
    visibility = visibility_private(),
)

run_add_exec(
    "install_dev",
    command = "cargo",
    args = ["install", "--force", "--path=spaces/crates/spaces", "--profile=dev", "--root={}".format(root)],
    deps = [":base_deps"],
    visibility = visibility_private(),
    help = "Install dev build on local system",
)

_install_release_args = [
    "install",
    "--target-dir=build/target",
    "--force",
    "--path=spaces/crates/spaces",
    "--profile=release",
    "--root={}".format(root),
]
_install_release_env = {}

if info_is_platform_linux():
    if info_is_platform_x86_64():
        _MUSL_TARGET = "x86_64-unknown-linux-musl"
    elif info_is_platform_aarch64():
        _MUSL_TARGET = "aarch64-unknown-linux-musl"
    else:
        _MUSL_TARGET = None
    _install_release_args.append("--target={}".format(_MUSL_TARGET))
    _install_release_env = musl_gcc_get_env()

run_add_exec(
    "install_release",
    command = "cargo",
    args = _install_release_args,
    deps = [":base_deps"],
    env = _install_release_env,
    visibility = visibility_private(),
)

if info_is_platform_linux():
    run_add_exec(
        "check_static_build",
        command = "ldd",
        args = ["build/target/{}/release/spaces".format(_MUSL_TARGET)],
        deps = [":install_release"],
        log_level = run_log_level_passthrough(),
    )

run_add_exec(
    "install_dev_lsp",
    command = "cargo",
    args = ["install", "--target-dir=build/target", "--features=lsp-debug", "--force", "--path=spaces/crates/spaces", "--profile=dev", "--root={}".format(root)],
    deps = [":base_deps"],
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

DEBUG_BINARY = "build/target/debug/spaces"

run_add_exec(
    "script_tests",
    command = DEBUG_BINARY,
    args = [
        "./spaces/scripts/run-all.exec.star",
        "--spaces={}".format(DEBUG_BINARY),
    ],
    visibility = visibility_private(),
    deps = deps(
        rules = [":build"],
        files = [
            "scripts/test/**/*.exec.star",
            "scripts/run-all.exec.star",
            "//@star/sdk/star/std/**/*.star",
        ],
    ),
)

run_add_exec(
    "check_rust_clippy",
    command = "cargo",
    args = ["clippy"],
    visibility = visibility_private(),
    deps = [
<<<<<<< HEAD
        ":check_rust_fmt",
        ":check_starlark",
        ":rustup_update",
        ":script_tests",
=======
        ":base_deps",
        ":check_rust_fmt",
        ":check_starlark",
>>>>>>> a79a6ec (#705. Add support for sandboxing with nono)
    ],
)
