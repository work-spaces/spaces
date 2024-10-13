use crate::executor::asset;
use crate::{executor, rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use starstd::{Function, Arg, get_rule_argument};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlatformArchive {
    pub macos_x86_64: Option<http_archive::Archive>,
    pub macos_aarch64: Option<http_archive::Archive>,
    pub windows_aarch64: Option<http_archive::Archive>,
    pub windows_x86_64: Option<http_archive::Archive>,
    pub linux_x86_64: Option<http_archive::Archive>,
    pub linux_aarch64: Option<http_archive::Archive>,
}

const fn get_archive_dict() -> &'static [(&'static str, &'static str)] {
    &[
        ("url", "url to zip|tar.xz|tar.gz|tar.bz2 file (can also be an uncompressed file with no suffix)"),
        ("sha256", "hash of the file"),
        ("link", "None|Hard: create hardlinks of the archive from the spaces store to the workspace"),
        ("includes", "options list of globs to include"),
        ("excludes", "optional list of globs to exclude"),
        ("strip_prefix", "optional prefix to strip from the archive path"),
        ("add_prefix", "optional prefix to add in the workspace (e.g. sysroot/share)"),
    ]
}


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
                    ("clone", "Default|Worktree"),
                ]
            }
        ],
        example: Some(r#"checkout.add_repo(
    # the rule name is also the path in the workspace where the clone will be
    rule = { "name": "spaces" },
    repo = {
        "url": "https://github.com/work-spaces/spaces",
        "rev": "main",
        "checkout": "Revision",
        "clone": "Default",
    }
)"#)
            
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
        example: Some(r#"checkout.add_archive(
    # the rule name is the path in the workspace where the archive will be extracted
    rule = {"name": "llvm-project"},
    archive = {
        "url": "https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-{}.zip".format(version),
        "sha256": "27b5c7c745ead7e9147c78471b9053d4f6fc3bed94baf45f4e8295439f564bb8",
        "link": "Hard",
        "strip_prefix": "llvm-project-llvmorg-{}".format(version),
        "add_prefix": "llvm-project",
    },
)"#),
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
                        "macos_aarch64",
                        "dict with same entries as archive in add_archive()",
                    ),
                    ("macos_x86_64", "dict"),
                    ("windows_aarch64", "dict"),
                    ("windows_x86_64", "dict"),
                    ("linux_aarch64", "dict"),
                    ("linux_x86_64", "dict"),
                ],
            },
        ],
        example: Some(r#"base = {
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
        "macos_x86_64": macos_x86_64,
        "macos_aarch64": macos_aarch64,
        "windows_x86_64": windows_x86_64,
        "windows_aarch64": windows_aarch64,
        "linux_x86_64": linux_x86_64,
    },
)"#)
    },
    Function {
        name: "add_asset",
        description: r#"Adds a file to the workspace. This is useful for providing
a top-level build file that orchestrates the entire workspace. It can also
be used to create a top-level README how the workflow works."#,
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
        example: Some(r#"content = """
# README

This is how to use this workspace.

"""

checkout.add_asset(
    rule = {"name": "README.md"},
    asset = {
        "destination": "README.md",
        "content": content,
    },
)"#)},
    Function {
        name: "add_which_asset",
        description: r#"Adds a hardlink to an executable file available on the `PATH` 
when checking out the workspace. This is useful for building tools that have complex dependencies.
Avoid using this when creating a workspace for your project. It creates system dependencies
that break workspace hermicity."#,
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
        example: Some(r#"checkout.add_which_asset(
    rule = { "name": "which_pkg_config" },
    asset = {
        "which": "pkg-config",
        "destination": "sysroot/bin/pkg-config"
    }
)"#)
        },
    Function {
        name: "update_asset",
        description: r#"Creates or updates an existing file containing structured data
in the workspace. This rules supports json|toml|yaml files. Different rules
can update the same file and the content will be preserved (as long as the keys are unique)."#,
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
        example: Some(r#"cargo_vscode_task = {
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
)"#)},
    Function {
        name: "update_env",
        description: r#"Creates or updates the environment file in the workspace.

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

"#,
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
        example: Some(r#"checkout.update_env(
    rule = {"name": "update_env"},
    env = {
        "paths": ["/usr/bin", "/bin"],
        "vars": {
            "PS1": '"(spaces) $PS1"',
        },
    },
)"#)}
];

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn add_repo(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] repo: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let repo: git::Repo = serde_json::from_value(repo.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let worktree_path = workspace::absolute_path();

        let state = rules::get_state().read().unwrap();
        let checkout = repo.get_checkout();
        let spaces_key = rule.name.clone();
        let rule_name = rule.name.clone();
        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Checkout,
                executor::Task::Git(executor::git::Git {
                    url: repo.url,
                    spaces_key,
                    worktree_path,
                    checkout,
                    clone: repo.clone.unwrap_or(git::Clone::Default),
                }),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_platform_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] platforms: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;
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

        add_http_archive(rule, platform_archive)
            .context(format_context!("Failed to add archive"))?;

        Ok(NoneType)
    }

    fn add_which_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for which asset rule"))?;

        let asset: asset::AddWhichAsset = serde_json::from_value(asset.to_json_value()?)
            .context(format_context!("Failed to parse which asset arguments"))?;

        let state = rules::get_state().read().unwrap();
        let rule_name = rule.name.clone();
        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Checkout,
                executor::Task::AddWhichAsset(asset),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    fn add_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] archive: starlark::values::Value,
        // includes, excludes, strip_prefix
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let archive: http_archive::Archive = serde_json::from_value(archive.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        add_http_archive(rule, Some(archive)).context(format_context!("Failed to add archive"))?;

        Ok(NoneType)
    }

    fn add_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let add_asset: executor::asset::AddAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse asset arguments"))?;

        let state = rules::get_state().read().unwrap();
        let rule_name = rule.name.clone();

        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Checkout,
                executor::Task::AddAsset(add_asset),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn update_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;
        // support JSON, yaml, and toml
        let update_asset: executor::asset::UpdateAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse archive arguments"))?;

        let state = rules::get_state().read().unwrap();
        let rule_name = rule.name.clone();

        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::PostCheckout,
                executor::Task::UpdateAsset(update_asset),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    fn update_env(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] env: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        // support JSON, yaml, and toml
        let update_env: executor::env::UpdateEnv = serde_json::from_value(env.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        let state = rules::get_state().read().unwrap();
        let rule_name = rule.name.clone();

        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::PostCheckout,
                executor::Task::UpdateEnv(update_env),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }
}

fn add_http_archive(
    rule: rules::Rule,
    archive_option: Option<http_archive::Archive>,
) -> anyhow::Result<()> {
    if let Some(archive) = archive_option {
        let state = rules::get_state().read().unwrap();

        //create a target that waits for all downloads
        //then create links based on all downloads being complete

        let http_archive = http_archive::HttpArchive::new(
            &workspace::get_store_path(),
            rule.name.as_str(),
            &archive,
        )
        .context(format_context!(
            "Failed to create http_archive {}",
            rule.name
        ))?;

        let rule_name = rule.name.clone();
        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Checkout,
                executor::Task::HttpArchive(executor::http_archive::HttpArchive {
                    http_archive,
                }),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
    }
    Ok(())
}
