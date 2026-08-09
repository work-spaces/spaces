"""
Spaces starlark checkout/run script to make changes to spaces, printer, and archiver.
With VSCode/Zed integration
"""

load("//@star/packages/star/musl-gcc.star", "musl_gcc_get_env")
load("//@star/prelude/info.star", "info_is_platform_aarch64", "info_is_platform_linux")
load("//@star/prelude/rules/deps.star", "deps")
load("//@star/prelude/rules/glob.star", "glob")
load(
    "//@star/prelude/rules/run.star",
    "run_add",
    "run_add_archive",
    "run_add_exec",
    "run_add_exec_test",
)
load(
    "//@star/sdk/star/visibility.star",
    "visibility_private",
    "visibility_rules",
)
load(
    "//@star/sdk/star/ws.star",
    "workspace_get_env_var",
    "workspace_get_env_var_or",
    "workspace_is_env_var_set",
    "workspace_load_value",
)

GLOB_DEPS = glob(includes = [
    "//Cargo.toml",
    "//spaces/Cargo.toml",
    "//spaces/Cargo.workspace.toml",
    "//spaces/**/*.rs",
    "//spaces/crates/spaces/src/assets/**/*.star",
    "//rust-toolchain.toml",
], excludes = [
    "//spaces/target/**",
])

rustup_files = ["//rust-toolchain.toml"]

run_add_exec(
    "rustup_update",
    command = "rustup",
    args = ["update"],
    deps = deps(files = rustup_files),
    help = "Update the Rust toolchain via rustup",
    visibility = visibility_private(),
)

run_add_exec(
    "cargo_tree",
    command = "cargo",
    args = ["tree"],
    deps = deps(rules = [":rustup_update"], files = rustup_files),
    help = "Run cargo tree. This is used to clean up the result of rustup update without any race conditions.",
    visibility = visibility_private(),
)

run_add_exec(
    "check",
    command = "cargo",
    args = ["check"],
    help = "Run cargo check on workspace",
    deps = deps(rules = [":cargo_tree"], globs = [GLOB_DEPS]),
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
    help = """**Run cargo build on workspace**:
            This rule runs the `cargo build` command on the workspace.

            It uses **rule caching** to skip the build if no dependencies have changed. This makes builds faster by reusing cached results.
            See [cargo build](https://doc.rust-lang.org/cargo/commands/cargo-build.html).

            This creates several advantages:
            - Faster builds by reusing cached results
            - Automatic dependency tracking
            - Easy building

            The next thing to say is *this* <- is in italics:

            See this blockquote.
            > This is a blockquote with **bold**.
            > And on the next line.

    """,
    env = {
        "SCCACHE_DIR": workspace_get_env_var("SCCACHE_DIR"),
        "RUSTUP_HOME": workspace_get_env_var("RUSTUP_HOME"),
        "CARGO_HOME": workspace_get_env_var("CARGO_HOME"),
    },
)

run_add_exec(
    "run",
    command = "cargo",
    args = ["run", "--target-dir=build/target"],
    help = "Run spaces from the build/debug target",
    deps = deps(rules = [":cargo_tree"], globs = [GLOB_DEPS]),
    visibility = visibility_private(),
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
    "fail",
    command = "bash",
    args = ["-c", "echo 'Writing to stderr' >&2 && false"],
    help = "Fail the build",
    visibility = visibility_private(),
)

run_add_exec(
    "clippy",
    command = "cargo",
    args = ["clippy"],
    log_level = "Passthrough",
    deps = deps(rules = [":cargo_tree"], globs = [GLOB_DEPS]),
    help = "Run cargo clippy on workspace",
    visibility = visibility_private(),
)

run_add_exec(
    "format",
    command = "cargo",
    args = ["fmt"],
    log_level = "Passthrough",
    deps = deps(rules = [":cargo_tree"], globs = [GLOB_DEPS]),
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
    deps = deps(rules = [":cargo_tree"], globs = [GLOB_DEPS]),
    visibility = visibility_rules(["//:test", "//spaces"]),
)

SPACES_INSTALL_ROOT = "SPACES_INSTALL_ROOT"

if workspace_is_env_var_set(SPACES_INSTALL_ROOT):
    root = workspace_get_env_var(SPACES_INSTALL_ROOT)
else:
    root = "{}/.local".format(workspace_get_env_var("HOME"))

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
    args = [
        "install",
        "--force",
        "--path=spaces/crates/spaces",
        "--profile=dev",
        "--root={}".format(root),
    ],
    deps = deps(rules = [":cargo_tree"], globs = [GLOB_DEPS]),
    visibility = visibility_private(),
    help = "Install dev build on local system",
)

linux_env = musl_gcc_get_env() if info_is_platform_linux() else {}
linux_musl_target = "--target={}-unknown-linux-musl".format("aarch64" if info_is_platform_aarch64() else "x86_64")
linux_args = [linux_musl_target] if info_is_platform_linux() else []

run_add_exec(
    "install_release",
    command = "cargo",
    args = [
        "install",
        "--target-dir=build/target",
        "--force",
        "--path=spaces/crates/spaces",
        "--profile=release",
        "--root={}".format(root),
    ] + linux_args,
    deps = deps(rules = [":cargo_tree"], globs = [GLOB_DEPS]),
)

run_add_exec(
    "install_dev_lsp",
    command = "cargo",
    args = [
        "install",
        "--target-dir=build/target",
        "--features=lsp-debug",
        "--force",
        "--path=spaces/crates/spaces",
        "--profile=dev",
        "--root={}".format(root),
    ],
    deps = deps(rules = [":cargo_tree"], globs = [GLOB_DEPS]),
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
    deps = [":cargo_tree"],
    visibility = visibility_private(),
)

DEBUG_BINARY = "build/target/debug/spaces"
CO_SPACES_TOML_DOCS_DIR = "spaces/docs/co-spaces-toml"

run_add_exec(
    "co_query_list_docs",
    command = DEBUG_BINARY,
    args = [
        "query-co",
        "list",
    ],
    env = {
        "CO_SPACES_TOML": CO_SPACES_TOML_DOCS_DIR,
    },
    deps = deps(
        rules = [":build"],
        files = [
            "docs/co-spaces-toml/*.co.spaces.toml",
        ],
    ),
    visibility = visibility_private(),
    help = "Run `spaces query-co list` using the docs/co-spaces-toml directory",
)

run_add_exec(
    "co_query_list_docs_json",
    command = DEBUG_BINARY,
    args = [
        "query-co",
        "list",
        "--format=json",
    ],
    env = {
        "CO_SPACES_TOML": CO_SPACES_TOML_DOCS_DIR,
    },
    deps = deps(
        rules = [":build"],
        files = [
            "docs/co-spaces-toml/*.co.spaces.toml",
        ],
    ),
    visibility = visibility_private(),
    help = "Run `spaces query-co list --format=json` using the docs/co-spaces-toml directory",
)

run_add_exec(
    "script_tests",
    command = DEBUG_BINARY,
    args = [
        "./spaces/scripts/run-all.exec.star",
        "--spaces={}".format(DEBUG_BINARY),
    ],
    visibility = visibility_private(),
    env = {
        # ensure //@star/prelude is not loaded from workspace
        "SPACES_WORKSPACE": "/tmp",
    },
    deps = deps(
        rules = [":build"],
        files = [
            "scripts/test/**/*.exec.star",
            "scripts/run-all.exec.star",
            "//@star/prelude/exec/**/*.star",
            "//@star/prelude/*.star",
        ],
    ),
)

run_add_exec(
    "create_exec_user_error_doc",
    command = DEBUG_BINARY,
    args = [
        "./spaces/scripts/show-all-errors.exec.star",
    ],
    deps = deps(
        rules = [":build"],
        files = [
            "scripts/errors/**",
            "scripts/show-all-errors.exec.star",
        ],
    ),
    target_files = ["//all-errors-output.txt"],
    visibility = visibility_private(),
    help = "Run all error scripts and write the output //all-errors-output.txt",
)

run_add_exec(
    "run_error_script",
    command = DEBUG_BINARY,
    args = [
        "./spaces/scripts/errors/error-toml-null-value.exec.star",
    ],
    deps = deps(
        rules = [":build"],
        files = [
            "scripts/errors/**",
            "scripts/errors/error-toml-null-value.exec.star",
        ],
    ),
    visibility = visibility_private(),
    help = "Run a single error script within a rule to check for a nested banner",
)

run_add_exec(
    "check_rust_clippy",
    command = "cargo",
    args = ["clippy"],
    visibility = visibility_private(),
    deps = [
        ":check_rust_fmt",
        ":check_starlark",
        ":script_tests",
    ],
)

RELEASE_INSTALL_DIR = "build/install"
SPACES_RELEASE_TAG_ENV = "SPACES_RELEASE_TAG"
GITHUB_REPOSITORY_ENV = "GITHUB_REPOSITORY"

release_tag = workspace_load_value(SPACES_RELEASE_TAG_ENV) or "dev"
github_repo = workspace_get_env_var_or(GITHUB_REPOSITORY_ENV, "work-spaces/spaces")

run_add_exec(
    "build_release_install",
    command = "cargo",
    args = [
        "install",
        "--target-dir=build/target",
        "--force",
        "--path=spaces/crates/spaces",
        "--profile=release",
        "--root={}".format(RELEASE_INSTALL_DIR),
    ],
    deps = deps(rules = [":cargo_tree"], globs = [GLOB_DEPS]),
    target_dirs = ["//build/install/bin"],
    help = "Build and install the release binary to build/install",
    visibility = visibility_private(),
)

(RELEASE_ARCHIVE_PATH, _RELEASE_ARCHIVE_SHA256) = run_add_archive(
    "archive_release",
    archive_name = "spaces",
    deps = [":build_release_install"],
    version = release_tag.lstrip("v"),
    source_directory = "//build/install/bin",
    suffix = "zip",
    includes = ["spaces*"],
    visibility = visibility_private(),
)

run_add_exec(
    "check_release",
    command = "gh",
    args = [
        "release",
        "view",
        release_tag,
        "--repo={}".format(github_repo),
    ],
    workspace_vars = ["GH_TOKEN"],
    help = "Verify the release {} exists on GitHub before publishing".format(release_tag),
    visibility = visibility_private(),
)

if workspace_load_value("SPACES_PUBLISH_DRY_RUN") == "ON":
    run_add(
        "publish_release",
        deps = [":archive_release"],
        help = "Build and archive the release but do not publish the artifacts",
    )
else:
    run_add_exec(
        "publish_release",
        command = "gh",
        args = [
            "release",
            "upload",
            release_tag,
            RELEASE_ARCHIVE_PATH,
            "--repo={}".format(github_repo),
            "--clobber",
        ],
        deps = [":check_release", ":archive_release"],
        workspace_vars = ["GH_TOKEN"],
        help = "Upload the release archive to the GitHub release {}".format(release_tag),
        visibility = visibility_private(),
    )
