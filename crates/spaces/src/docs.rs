use clap::ValueEnum;
use starstd::{Arg, Function};
use crate::info;

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum DocItem {
    Checkout,
    Run,
    Info,
    StarStd,
    Completions,
    CheckoutAddAsset,
    CheckoutAddWhichAsset,
    CheckoutAddRepo,
    CheckoutAddPlatformArchive,
    CheckoutUpdateAsset,
    CheckoutUpdateEnv,
    RunAddExec,
    RunAddExecIf,
    RunAddTarget,
}


fn show_function(
    function: &Function,
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    markdown.heading(level, format!("{}()", function.name).as_str())?;

    markdown.code_block(
        "python",
        format!(
            "def {}({}) -> {}",
            function.name,
            function.args
                .iter()
                .map(|arg| arg.name)
                .collect::<Vec<&str>>()
                .join(", "),
            function.return_type
        )
        .as_str(),
    )?;

    markdown.printer.newline()?;

    markdown.paragraph(function.description)?;

    for arg in function.args {
        markdown.list_item(1, format!("`{}`: {}", arg.name, arg.description).as_str())?;
        for (key, value) in arg.dict {
            markdown.list_item(2, format!("`{}`: {}", key, value).as_str())?;
        }
    }

    markdown.printer.newline()?;

    if let Some(example) = function.example {
        markdown.printer.newline()?;
        markdown.bold("Example")?;
        markdown.printer.newline()?;
        markdown.code_block("python", example)?;
    }

    Ok(())
}


const fn get_rule_argument() -> Arg {
    Arg {
        name: "rule",
        description: "dict",
        dict: &[
            ("name", "rule name as string"),
            ("deps", "list of dependencies"),
            ("type", "Setup|Run (default)|Optional"),
        ],
    }
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

fn show_completions(markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(1, "Completions")?;

    Ok(())
}

fn show_run_add_exec(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {

    const FUNCTION: Function = Function {
        name: "add_exec",
        description: "Adds a rule that will execute a process.",
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "exec",
                description: "dict with",
                dict: &[
                    ("command", "name of the command to execute"),
                    ("args", "optional list of arguments"),
                    ("env", "optional dict of environment variables"),
                    ("working_directory", "optional working directory (default is the workspace)"),
                    ("expect", "Failure: expect non-zero return code|Success: expect zero return code"),
                ],
            },
        ],
        example: Some(r#"run.add_exec(
    rule = {"name": name, "type": "Setup", "deps": ["sysroot-python:venv"]},
    exec = {
        "command": "pip3",
        "args": ["install"] + packages,
    },
)"#)};

    show_function(&FUNCTION, level, markdown)?;

    Ok(())
}

fn show_run_add_exec_if(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {

    const FUNCTION: Function = Function {
        name: "add_exec_if",
        description: "Adds a rule to execute if a condition is met.",
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "exec_if",
                description: "dict with",
                dict: &[
                    ("if", "this is an `exec` object used with add_exec()"),
                    ("then", "list of optional targets to enable if the command has the expected result"),
                    ("else", "optional list of optional targets to enable if the command has the unexpected result"),
                ],
            },
        ],
        example: Some(r#"run.add_exec(
    rule = {"name": create_file, "type": "Optional" },
    exec = {
        "command": "touch",
        "args": ["some_file"],
    },
)

run.add_exec_if(
    rule = {"name": check_file, "deps": []},
    exec_if = {
        "if": {
            "command": "ls",
            "args": [
                "some_file",
            ],
            "expect": "Failure",
        },
        "then": ["create_file"],
    },
)"#)};


    show_function(&FUNCTION, level, markdown)?;

    Ok(())
}

fn show_run_add_target(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {

    const FUNCTION: Function = Function {
        name: "add_target",
        description: "Adds a target. There is no specific action for the target, but this rule can be useful for organizing depedencies.",
        return_type: "None",
        args: &[
            get_rule_argument(),
        ],
        example: Some(r#"run.add_target(
    rule = {"name": "my_rule", "deps": ["my_other_rule"]},
)"#)};

    show_function(&FUNCTION, level, markdown)?;

    Ok(())
}

fn show_run(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Run Rules")?;

    markdown.paragraph(r#"You use run rules to execute tasks in the workspace."#)?;

    show_run_add_exec(level + 1, markdown)?;
    markdown.printer.newline()?;
    show_run_add_exec_if(level + 1, markdown)?;
    markdown.printer.newline()?;
    show_run_add_target(level + 1, markdown)?;
    markdown.printer.newline()?;
    Ok(())
}

fn show_checkout_add_repo(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    const FUNCTION:Function = Function {
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
                
        };

    show_function(&FUNCTION, level, markdown)?;
   

    Ok(())
}

fn show_checkout_add_archive(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {

    const FUNCTION: Function = Function {
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
    };


    show_function(&FUNCTION, level, markdown)?;

    Ok(())
}

fn show_checkout_add_platform_archive(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {

    const FUNCTION: Function = Function {
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
    };

    show_function(&FUNCTION, level, markdown)?;

    Ok(())
}

fn show_checkout_add_which_asset(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {

    const FUNCTION: Function = Function {
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
        };

    show_function(&FUNCTION, level, markdown)?;

    Ok(())
}

fn show_checkout_add_asset(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {

    const FUNCTION: Function = Function {
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
)"#)};

    show_function(&FUNCTION, level, markdown)?;

    Ok(())
}

fn show_checkout_update_asset(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {

    const FUNCTION: Function = Function {
        name: "update_asset",
        description: r#"Creates or updates an existing file containing structured data
in the workspace. This rules supports json|toml|yaml files. Different rules
can update the same file and the content will be preserved (as long as the keys are unique)."#,
        return_type: "None",
        args: &[
            get_rule_argument(),
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
)"#)};

    show_function(&FUNCTION, level, markdown)?;

    Ok(())
}

fn show_checkout_update_env(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {

    const FUNCTION: Function = Function {
        name: "update_env",
        description: r#"Creates or updates the environment file in the workspace.


    markdown.heading(level, "update_asset()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph(
        r#"Spaces creates two mechanisms for managing the workspace environment.
1. It generates an `env` file that can be sourced from the command line.
2. When running `spaces run` it executes rules using the same environment values.

The rules allows you to add variables and paths to the environment.

At a minimum, `your-workspace/sysroot/bin` should be added to the path.

In the workspace, you can start a workspace bash shell using:

```sh
env -i bash --noprofile --norc
source env
```

When using `zsh`, use:

```sh
env -i zsh -f
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
)"#)};

    show_function(&FUNCTION, level, markdown)?;

    Ok(())
}

fn show_checkout(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Checkout Rules")?;

    markdown.paragraph(
        r#"You use checkout rules to build a workspace.
You can fetch git repositories and archives. You can also add assets (local files)
to the workspace root folder (not under version control)."#,
    )?;

    show_checkout_add_repo(level + 1, markdown)?;
    markdown.printer.newline()?;
    show_checkout_add_archive(level + 1, markdown)?;
    markdown.printer.newline()?;
    show_checkout_add_platform_archive(level + 1, markdown)?;
    markdown.printer.newline()?;
    show_checkout_add_which_asset(level + 1, markdown)?;
    markdown.printer.newline()?;
    show_checkout_add_asset(level + 1, markdown)?;
    markdown.printer.newline()?;
    Ok(())
}

fn show_info(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Info Functions")?;

    markdown.heading(level + 1, "Description")?;

    markdown.paragraph(
        r#"The `info` functions provide information about the workspace
during checkout and run. Info functions are executed immediately. They are not rule definitions.

```star
is_windows = info.is_platform_windows()
```
"#,
    )?;


    markdown.heading(level + 1, "Functions")?;

    for function in info::FUNCTIONS {
        show_function(function, level + 2, markdown)?;
    }

    Ok(())
}

fn show_star_std(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "Spaces Starlark Standard Functions")?;

    markdown.heading(level + 1, "Description")?;

    markdown.paragraph(
        r#"The spaces starlark standard library includes
functions for doing things like accessing the filesystem. The functions
in this library are executed immediately."#,
    )?;

    markdown.heading(level + 1, "`fs` Functions")?;

    for function in starstd::fs::FUNCTIONS {
        show_function(function, level + 2, markdown)?;
    }

    markdown.heading(level + 1, "`process` Functions")?;
    
    for function in starstd::process::FUNCTIONS {
        show_function(function, level + 2, markdown)?;
    }

    markdown.heading(level + 1, "`script` Functions")?;

    for function in starstd::script::FUNCTIONS {
        show_function(function, level + 2, markdown)?;
    }

    Ok(())
}

fn show_doc_item(
    markdown: &mut printer::markdown::Markdown,
    doc_item: DocItem,
) -> anyhow::Result<()> {
    match doc_item {
        DocItem::Checkout => show_checkout(1, markdown)?,
        DocItem::Run => show_run(1, markdown)?,
        DocItem::Completions => show_completions(markdown)?,
        DocItem::CheckoutAddRepo => show_checkout_add_repo(1, markdown)?,
        DocItem::CheckoutAddAsset => show_checkout_add_asset(1, markdown)?,
        DocItem::CheckoutAddWhichAsset => show_checkout_add_which_asset(1, markdown)?,
        DocItem::CheckoutAddPlatformArchive => show_checkout_add_platform_archive(1, markdown)?,
        DocItem::CheckoutUpdateAsset => show_checkout_update_asset(1, markdown)?,
        DocItem::CheckoutUpdateEnv => show_checkout_update_env(1, markdown)?,
        DocItem::RunAddExec => show_run_add_exec(1, markdown)?,
        DocItem::RunAddExecIf => show_run_add_exec_if(1, markdown)?,
        DocItem::RunAddTarget => show_run_add_target(1, markdown)?,
        DocItem::Info => show_info(1, markdown)?,
        DocItem::StarStd => show_star_std(1, markdown)?,
    }
    Ok(())
}

fn show_all(markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(1, "Spaces API Documentation")?;
    markdown.printer.newline()?;

    show_info(2, markdown)?;
    show_star_std(2, markdown)?;
    show_checkout(2, markdown)?;
    show_run(2, markdown)?;

    Ok(())
}

pub fn show(printer: &mut printer::Printer, doc_item: Option<DocItem>) -> anyhow::Result<()> {
    let mut markdown = printer::markdown::Markdown::new(printer);

    if let Some(doc_item) = doc_item {
        show_doc_item(&mut markdown, doc_item)?;
    } else {
        show_all(&mut markdown)?;
    }

    markdown.printer.newline()?;
    Ok(())
}
