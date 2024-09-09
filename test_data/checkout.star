"""

"""

cargo_toml_contents = """
[workspace]
resolver = "2"
members = ["spaces"]

[profile.dev]
opt-level = 3
lto = false
debug = true
strip = false
codegen-units = 16

[profile.release]
opt-level = "z"
lto = true
debug = false
panic = "abort"
strip = true
codegen-units = 1
"""

checkout.add_asset(
    rule = { "name": "workspace.Cargo.toml" },
    asset = {
        "destination": "Cargo.toml",
        "content": cargo_toml_contents,
    },
)

checkout.add_platform_archive(
    rule = { "name": "rustup-init" },
    platforms = {
        "macos_aarch64": {
            "url": "https://static.rust-lang.org/rustup/dist/aarch64-apple-darwin/rustup-init",
            "sha256": "f547d77c32d50d82b8228899b936bf2b3c72ce0a70fb3b364e7fba8891eba781",
            "add_prefix": "sysroot/bin",
            "link": "Hard",
        },
        "macos_x86_64": {
            "url": "https://static.rust-lang.org/rustup/dist/x86_64-apple-darwin/rustup-init",
            "sha256": "f547d77c32d50d82b8228899b936bf2b3c72ce0a70fb3b364e7fba8891eba781",
            "add_prefix": "sysroot/bin",
            "link": "Hard",
        },
    },
)

checkout.add_repo(
    rule = { "name": "printer" },
    repo = {
        "url": "https://github.com/work-spaces/printer-rs",
        "rev": "main",
        "checkout": "Revision",
    },
)

checkout.add_repo(
    rule = { "name": "easy-archiver" },
    repo = {
        "url": "https://github.com/work-spaces/easy-archiver",
        "rev": "main",
        "checkout": "Revision",
    },
)

checkout.add_repo(
    rule = { "name": "tools/sysroot-rust" },
    repo = {
        "url": "https://github.com/work-spaces/sysroot-rust",
        "rev": "main",
        "checkout": "Revision",
    },
)

checkout.add_repo(
    rule = { "name": "tools/sysroot-sccache" },
    repo = {
        "url": "https://github.com/work-spaces/sysroot-sccache",
        "rev": "v0",
        "checkout": "Revision",
    },
)

cargo_vscode_task = {
    "type": "cargo",
    "problemMatcher": ["$rustc"],
    "group": "build",
}

checkout.update_asset(
    rule = { "name": "vscode_tasks" },
    asset = {
        "destination": ".vscode/tasks.json",
        "format": "json",
        "value": {
            "tasks": [
                cargo_vscode_task | {
                    "command": "build",
                    "args": ["--manifest-path=spaces/Cargo.toml"],
                    "label": "build:spaces",
                },
                cargo_vscode_task | {
                    "command": "install",
                    "args": ["--path=spaces", "--root=${userHome}/.local", "--profile=dev"],
                    "label": "install_dev:spaces",
                },
                cargo_vscode_task | {
                    "command": "install",
                    "args": ["--path=spaces", "--root=${userHome}/.local", "--profile=release"],
                    "label": "install:spaces",
                },
                cargo_vscode_task | {
                    "command": "build",
                    "args": ["--manifest-path=printer/Cargo.toml"],
                    "label": "build:printer",
                },
            ],
        },
    },
)

checkout.update_asset(
    rule = { "name": "cargo_config" },
    asset = {
        "destination": ".cargo/config.toml",
        "format": "toml",
        "value": {
            "patch": {
                "https://github.com/work-spaces/printer-rs": {"printer": {"path": "./printer"}},
                "https://github.com/work-spaces/easy-archiver": {"easy-archiver": {"path": "./easy-archiver"}},
            },
            "build": {"rustc-wrapper": "sccache"},
        },
    },
)

checkout.update_env(
    rule = { "name": "rust_env" },
    env = {
        "vars": {"RUST_TOOLCHAIN": "1.80", "PS1": '"(spaces) $PS1"'},
        "paths": ["/usr/bin", "/bin"],
    }
)
