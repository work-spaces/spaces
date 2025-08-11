use crate::executor::asset;
use crate::{executor, rules, singleton, task};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use starstd::{get_rule_argument, Arg, Function};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PlatformArchive {
    #[serde(rename = "macos-x86_64")]
    pub macos_x86_64: Option<http_archive::Archive>,
    #[serde(rename = "macos-aarch64")]
    pub macos_aarch64: Option<http_archive::Archive>,
    #[serde(rename = "windows-x86_64")]
    pub windows_aarch64: Option<http_archive::Archive>,
    #[serde(rename = "windows-aarch64")]
    pub windows_x86_64: Option<http_archive::Archive>,
    #[serde(rename = "linux-x86_64")]
    pub linux_x86_64: Option<http_archive::Archive>,
    #[serde(rename = "linux-aarch64")]
    pub linux_aarch64: Option<http_archive::Archive>,
}

const fn get_archive_dict() -> &'static [(&'static str, &'static str)] {
    &[
        ("url", "url to zip|tar.xz|tar.gz|tar.bz2 file (can also be an uncompressed file with no suffix)"),
        ("sha256", "hash of the file"),
        ("link", "None|Hard: create hardlinks of the archive from the spaces store to the workspace"),
        ("globs", "optional list of globs prefix with `+` to include and `-` to exclude"),
        ("strip_prefix", "optional prefix to strip from the archive path"),
        ("add_prefix", "optional prefix to add in the workspace (e.g. sysroot/share)"),
    ]
}

const ADD_REPO_EXAMPLE: &str = r#"checkout.add_repo(
    # the rule name is also the path in the workspace where the clone will be
    rule = { "name": "spaces" },
    repo = {
        "url": "https://github.com/work-spaces/spaces",
        "rev": "main",
        "checkout": "Revision",
        "clone": "Default",
        "is_evaluate_spaces_modules": True
    }
)"#;

const ADD_ARCHIVE_EXAMPLE: &str = r#"checkout.add_archive(
    # the rule name is the path in the workspace where the archive will be extracted
    rule = {"name": "llvm-project"},
    archive = {
        "url": "https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-{}.zip".format(version),
        "sha256": "27b5c7c745ead7e9147c78471b9053d4f6fc3bed94baf45f4e8295439f564bb8",
        "link": "Hard",
        "strip_prefix": "llvm-project-llvmorg-{}".format(version),
        "add_prefix": "llvm-project",
    },
)"#;

const ADD_PLATFORM_ARCHIVE_EXAMPLE: &str = r#"base = {
    "add_prefix": "sysroot/bin",
    "strip_prefix": "target/release",
    "link": "Hard",
}

macos_x86_64 = base | {
    "url": "https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-macos-latest-x86_64-v0.6.0-beta.13.zip",
    "sha256": "47d325145e6f7f870426f1b123c781f89394b0458bb43f5abe2d36ac3543f7ef",
}

macos_aarch64 = base | {
    "url": "https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-macos-latest-aarch64-v0.6.0-beta.13.zip",
    "sha256": "6dd972454942faa609670679c53b6876ab8e66bcfd0b583ee5a8d13c93b2e879",
}

windows_x86_64 = base | {
    "url": "https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-windows-latest-x86_64-v0.6.0-beta.13.exe",
    "sha256": "b93dc96b2c66fcfc4aef851db2064f6e6ecb54b29968ca5174f6b892b99651c8",
}

windows_aarch64 = base | {
    "url": "https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-windows-latest-aarch64-v0.6.0-beta.13.exe",
    "sha256": "c67c7b23897e0949843e248465d5444428fb287f89dcd45cec76dde4b2cdc6a9",
}

linux_x86_64 = base | {
    "url": "https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-linux-gnu-x86_64-v0.6.0-beta.13.zip",
    "sha256": "39030124f18b338eceee09061fb305b522ada76f6a0562f9926ea0747b3ad440",
}

checkout.add_platform_archive(
    # rule name is only the path in the workspace if add_prefix is not set
    rule = {"name": "spaces"},
    platforms = {
        "macos-x86_64": macos_x86_64,
        "macos-aarch64": macos_aarch64,
        "windows-x86_64": windows_x86_64,
        "windows-aarch64": windows_aarch64,
        "linux-x86_64": linux_x86_64,
    },
)"#;

const ADD_CARGO_BIN_EXAMPLE: &str = r#"checkout.add_cargo_bin(
    rule = {"name": "probe-rs-tools"},
    cargo_bin = {
        "crate": "probe-rs-tools",
        "version": "0.24.0",
        "bins": ["probe-rs", "cargo-embed", "cargo-flash"]
    },
)"#;

const ADD_ASSET_DESCRIPTION: &str = r#"Adds a file to the workspace. This is useful for providing
a top-level build file that orchestrates the entire workspace. It can also
be used to create a top-level README how the workflow works."#;

const ADD_ASSET_EXAMPLE: &str = r#"content = """
# README

This is how to use this workspace.

"""

checkout.add_asset(
    rule = {"name": "README.md"},
    asset = {
        "destination": "README.md",
        "content": content,
    },
)"#;

const ADD_WHICH_ASSET_DESCRIPTION: &str = r#"Adds a hardlink to an executable file available on the `PATH`
when checking out the workspace. This is useful for building tools that have complex dependencies.
Avoid using this when creating a workspace for your project. It creates system dependencies
that break workspace hermicity."#;

const ADD_WHICH_ASSET_EXAMPLE: &str = r#"checkout.add_which_asset(
    rule = { "name": "which_pkg_config" },
    asset = {
        "which": "pkg-config",
        "destination": "sysroot/bin/pkg-config"
    }
)"#;

const ADD_HARD_LINK_ASSET_EXAMPLE: &str = r#"checkout.add_hard_link_asset(
    rule = { "name": "which_pkg_config" },
    asset = {
        "source": "<path to asset>",
        "destination": "sysroot/asset/my_asset"
    }
)"#;

const UPDATE_ASSET_DESCRIPTION: &str = r#"Creates or updates an existing file containing structured data
in the workspace. This rules supports json|toml|yaml files. Different rules
can update the same file and the content will be preserved (as long as the keys are unique)."#;

const UPDATE_ASSET_EXAMPLE: &str = r#"cargo_vscode_task = {
    "type": "cargo",
    "problemMatcher": ["$rustc"],
    "group": "build",
}

# Add some VS code tasks
checkout.update_asset(
    rule = {"name": "vscode_tasks"},
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
                }
            ],
        },
    }
)

# tell cargo to use sccache
checkout.update_asset(
    rule = {"name": "cargo_config"},
    asset = {
        "destination": ".cargo/config.toml",
        "format": "toml",
        "value": {
            "build": {"rustc-wrapper": "sccache"},
        },
    },
)"#;

const UPDATE_ENV_DESCRIPTION: &str = r#"Creates or updates the environment file in the workspace.

Spaces creates two mechanisms for managing the workspace environment.

1. It generates an `env` file that can be sourced from the command line.
2. When running `spaces run` it executes rules using the same environment values.

The rules allows you to add variables and paths to the environment.

At a minimum, `your-workspace/sysroot/bin` should be added to the path.

In the workspace, you can start a workspace bash shell using:

```sh
bash # or the shell of your preference
source env
```

"#;

const UPDATE_ENV_EXAMPLE: &str = r#"checkout.update_env(
    rule = {"name": "update_env"},
    env = {
        "paths": [],
        "system_paths": ["/usr/bin", "/bin"],
        "vars": {
            "PS1": '"(spaces) $PS1"',
        },
        "inherited_vars": ["HOME", "SHELL", "USER"],
    },
)"#;

const ADD_TARGET_EXAMPLE: &str = r#"checkout.add_target(
    rule = {"name": "my_rule", "deps": ["my_other_rule"]},
)"#;

pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "add_repo",
        description: "returns the name of the current platform",
        return_type: "str",
        args: &[
            get_rule_argument(),
            Arg{
                name : "repo",
                description: "dict with",
                dict: &[
                    ("url", "ssh or https path to repository"),
                    ("rev", "repository revision as a branch, tag or commit"),
                    ("checkout", "Revision: checkout detached at commit or branch|NewBranch: create a new branch based at rev"),
                    ("clone", "Default|Worktree|Shallow"),
                    ("is_evaluate_spaces_modules", "True|False to check the repo for spaces.star files to evaluate"),
                ]
            }
        ],
        example: Some(ADD_REPO_EXAMPLE)
    },
    Function {
        name: "add_archive",
        description: "Adds an archive to the workspace.",
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "archive",
                description: "dict value",
                dict: get_archive_dict(),
            },
        ],
        example: Some(ADD_ARCHIVE_EXAMPLE),
    },
    Function {
        name: "add_platform_archive",
        description: "Adds an archive to the workspace based on the platform.",
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "platforms",
                description: "dict with platform keys",
                dict: &[
                    (
                        "macos-aarch64",
                        "dict with same entries as archive in add_archive()",
                    ),
                    ("macos-x86_64", "same as macos-aarch64"),
                    ("windows-aarch64", "same as macos-aarch64"),
                    ("windows-x86_64", "same as macos-aarch64"),
                    ("linux-aarch64", "same as macos-aarch64"),
                    ("linux-x86_64", "same as macos-aarch64"),
                ],
            },
        ],
        example: Some(ADD_PLATFORM_ARCHIVE_EXAMPLE),
    },
    Function {
        name: "add_cargo_bin",
        description: "Adds a binary crate using cargo-binstall. The binaries are installed in the spaces store and hardlinked to the workspace.",
        return_type: "str",
        args: &[
            get_rule_argument(),
            Arg{
                name : "cargo_bin",
                description: "dict with",
                dict: &[
                    ("crate", "The name of the binary crate"),
                    ("version", "The crate version to install"),
                    ("bins", "List of binaries to install"),
                ]
            }
        ],
        example: Some(ADD_CARGO_BIN_EXAMPLE)
    },
    Function {
        name: "add_asset",
        description: ADD_ASSET_DESCRIPTION,
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "asset",
                description: "dict with",
                dict: &[
                    ("content", "file contents as a string"),
                    ("destination", "relative path where asset will live in the workspace"),
                ],
            },
        ],
        example: Some(ADD_ASSET_EXAMPLE)},
    Function {
        name: "add_which_asset",
        description: ADD_WHICH_ASSET_DESCRIPTION,
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "asset",
                description: "dict with",
                dict: &[
                    ("which", "name of system executable to search for"),
                    ("destination", "relative path where asset will live in the workspace"),
                ],
            },
        ],
        example: Some(ADD_WHICH_ASSET_EXAMPLE)
    },
    Function {
            name: "add_hard_link_asset",
            description: r#"Adds a hardlink from anywhere on the system to the workspace"#,
            return_type: "None",
            args: &[
                get_rule_argument(),
                Arg {
                    name: "asset",
                    description: "dict with",
                    dict: &[
                        ("source", "the source of the hard link"),
                        ("destination", "relative path where asset will live in the workspace"),
                    ],
                },
            ],
            example: Some(ADD_HARD_LINK_ASSET_EXAMPLE)
    },
    Function {
        name: "add_soft_link_asset",
        description: r#"Adds a softlink from anywhere on the system to the workspace"#,
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "asset",
                description: "dict with",
                dict: &[
                    ("source", "the source of the software link"),
                    ("destination", "relative path where asset will live in the workspace"),
                ],
            },
        ],
        example: Some(ADD_HARD_LINK_ASSET_EXAMPLE)
    },
    Function {
        name: "update_asset",
        description: UPDATE_ASSET_DESCRIPTION,
        return_type: "None",
        args: &[
            starstd::get_rule_argument(),
            Arg {
                name: "asset",
                description: "dict with",
                dict: &[
                    ("destination", "path to the asset in the workspace"),
                    ("format", "json|toml|yaml"),
                    ("value", "dict containing the structured data to be added to the asset"),
                ],
            },
        ],
        example: Some(UPDATE_ASSET_EXAMPLE)},
    Function {
        name: "update_env",
        description: UPDATE_ENV_DESCRIPTION,
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "env",
                description: "dict with",
                dict: &[
                    ("vars", "dict of variables to add to the environment"),
                    ("paths", "list of paths required"),
                ],
            },
        ],
        example: Some(UPDATE_ENV_EXAMPLE)},
    Function {
        name: "abort",
        description: "Abort script evaluation with a message.",
        return_type: "None",
        args: &[
            Arg {
                name: "message",
                description: "Abort message to show the user.",
                dict: &[],
            },
        ],
        example: Some(r#"checkout.abort("Failed to do something")"#)},
    Function {
        name: "add_target",
        description: "Adds a target. There is no specific action for the target, but this rule can be useful for organizing dependencies.",
        return_type: "None",
        args: &[
            get_rule_argument(),
        ],
        example: Some(ADD_TARGET_EXAMPLE)},
];

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct CargoBin {
    #[serde(rename = "crate")]
    crate_: String,
    bins: Vec<String>,
    version: String,
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn abort(message: &str) -> anyhow::Result<NoneType> {
        Err(format_error!("Checkout Aborting: {}", message))
    }

    fn add_target(
        #[starlark(require = named)] rule: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add target rule"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::Target,
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_repo(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] repo: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo rule"))?;

        let repo: git::Repo = serde_json::from_value(repo.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();

        let worktree_path = if let Some(directory) = repo.working_directory.as_ref() {
            directory.clone()
        } else {
            workspace.get_absolute_path()
        };

        if let Some(clone_type) = repo.clone.as_ref() {
            if *clone_type == git::Clone::Worktree && repo.sparse_checkout.is_some() {
                return Err(format_error!(
                    "Sparse checkout is not supported with Worktree clone"
                ));
            }
        }

        let checkout = repo.get_checkout();
        let spaces_key = rule.name.clone();
        let rule_name = rule.name.clone();
        let url = repo.url.trim_end_matches('/');
        let url: Arc<str> = url.strip_suffix(".git").unwrap_or(url).into();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::Git(executor::git::Git {
                url,
                spaces_key,
                worktree_path,
                checkout,
                clone: repo.clone.unwrap_or(git::Clone::Default),
                is_evaluate_spaces_modules: repo.is_evaluate_spaces_modules.unwrap_or(true),
                sparse_checkout: repo.sparse_checkout,
                working_directory: repo.working_directory,
            }),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_cargo_bin(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] cargo_bin: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let cargo_bin: CargoBin = serde_json::from_value(cargo_bin.to_json_value()?)
            .context(format_context!("bad options for cargo_bin"))?;

        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for cargo_bin rule"))?;

        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();

        let cargo_binstall_dir = workspace.get_cargo_binstall_root();

        let output_directory = format!("{}/{}", cargo_binstall_dir, cargo_bin.version);

        std::fs::create_dir_all(output_directory.as_str()).context(format_context!(
            "Failed to create directory {output_directory}"
        ))?;

        let hard_link_rule = rule.clone();

        let cargo_binstall_path = format!(
            "{}/sysroot/bin/cargo-binstall",
            workspace.get_spaces_tools_path()
        );

        let exec = executor::exec::Exec {
            command: cargo_binstall_path.into(),
            args: Some(vec![
                format!("--version={}", cargo_bin.version).into(),
                format!("--root={output_directory}").into(),
                "--no-confirm".into(),
                cargo_bin.crate_.into(),
            ]),
            env: None,
            working_directory: None,
            redirect_stdout: None,
            expect: None,
            log_level: None,
            timeout: None,
        };

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::Exec(exec),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        for bin in cargo_bin.bins {
            let mut bin_rule = hard_link_rule.clone();
            bin_rule.name = format!("{}/{}", hard_link_rule.name, bin).into();

            // cargo install uses the root/bin install directory
            let output_file = format!("{output_directory}/bin/{bin}");

            let rule_name = hard_link_rule.name.clone();
            rules::insert_task(task::Task::new(
                bin_rule,
                task::Phase::PostCheckout,
                executor::Task::AddHardLink(asset::AddHardLink {
                    source: output_file,
                    destination: format!("sysroot/bin/{bin}"),
                }),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        }

        Ok(NoneType)
    }

    fn add_platform_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] platforms: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add platform archive rule"))?;
        //convert platforms to starlark value

        let platforms: PlatformArchive = serde_json::from_value(platforms.to_json_value()?)?;

        let platform_archive = match platform::Platform::get_platform() {
            Some(platform::Platform::MacosX86_64) => platforms.macos_x86_64,
            Some(platform::Platform::MacosAarch64) => platforms.macos_aarch64,
            Some(platform::Platform::WindowsX86_64) => platforms.windows_x86_64,
            Some(platform::Platform::WindowsAarch64) => platforms.windows_aarch64,
            Some(platform::Platform::LinuxX86_64) => platforms.linux_x86_64,
            Some(platform::Platform::LinuxAarch64) => platforms.linux_aarch64,
            _ => None,
        };

        if platform_archive.is_none() {
            return Err(format_error!(
                "Platform {} not supported by {}",
                platform::Platform::get_platform().unwrap(),
                rule.name
            ));
        }
        add_http_archive(rule, platform_archive)
            .context(format_context!("Failed to add archive"))?;

        Ok(NoneType)
    }

    fn add_which_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for which asset rule"))?;

        let asset: asset::AddWhichAsset = serde_json::from_value(asset.to_json_value()?)
            .context(format_context!("Failed to parse which asset arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::AddWhichAsset(asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    fn add_hard_link_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for which asset rule"))?;

        let asset: asset::AddHardLink = serde_json::from_value(asset.to_json_value()?)
            .context(format_context!("Failed to parse which asset arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::AddHardLink(asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    fn add_soft_link_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for which asset rule"))?;

        let asset: asset::AddSoftLink = serde_json::from_value(asset.to_json_value()?)
            .context(format_context!("Failed to parse which asset arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::AddSoftLink(asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    fn add_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] archive: starlark::values::Value,
        // includes, excludes, strip_prefix
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add archive rule"))?;

        let archive: http_archive::Archive = serde_json::from_value(archive.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        add_http_archive(rule, Some(archive)).context(format_context!("Failed to add archive"))?;
        Ok(NoneType)
    }

    fn add_oras_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] oras_archive: starlark::values::Value,
        // includes, excludes, strip_prefix
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for oras rule"))?;

        let oras_archive: executor::oras::OrasArchive =
            serde_json::from_value(oras_archive.to_json_value()?)
                .context(format_context!("Failed to parse oras archive arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::OrasArchive(oras_archive),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    fn add_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add asset rule"))?;

        let add_asset: executor::asset::AddAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse asset arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::AddAsset(add_asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn update_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for update asset rule"))?;
        // support JSON, yaml, and toml
        let update_asset: executor::asset::UpdateAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse archive arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::PostCheckout,
            executor::Task::UpdateAsset(update_asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    fn update_env(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] env: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for update env rule"))?;

        // support JSON, yaml, and toml
        let environment: environment::Environment = serde_json::from_value(env.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        let update_env = executor::env::UpdateEnv { environment };

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::UpdateEnv(update_env),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }
}

fn add_http_archive(
    rule: rule::Rule,
    archive_option: Option<http_archive::Archive>,
) -> anyhow::Result<()> {
    if let Some(mut archive) = archive_option {
        //create a target that waits for all downloads
        //then create links based on all downloads being complete

        archive.sha256 = if archive.sha256.starts_with("http") {
            // download the sha256 file
            http_archive::download_string(&archive.sha256).context(format_context!(
                "Failed to download sha256 file {}",
                archive.sha256
            ))?
        } else {
            archive.sha256
        };

        let mut globs = archive.globs.unwrap_or_default();
        if let Some(includes) = archive.includes.as_ref() {
            for include in includes {
                globs.insert(format!("+{include}").into());
            }
        }

        if let Some(excludes) = archive.excludes.as_ref() {
            if globs.is_empty() {
                globs.insert("+**".into());
            }
            for exclude in excludes {
                globs.insert(format!("-{exclude}").into());
            }
        }

        if !globs.is_empty() {
            changes::glob::validate(&globs).context(format_context!("Failed to validate globs"))?;
            archive.globs = Some(globs);
        } else {
            archive.globs = None;
        }

        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();

        let http_archive = http_archive::HttpArchive::new(
            &workspace.get_store_path(),
            rule.name.as_ref(),
            &archive,
            format!("{}/sysroot/bin", workspace.get_spaces_tools_path()).as_str(),
        )
        .context(format_context!(
            "Failed to create http_archive {}",
            rule.name
        ))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::HttpArchive(executor::http_archive::HttpArchive { http_archive }),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
    }
    Ok(())
}
