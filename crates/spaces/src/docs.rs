use clap::ValueEnum;

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

struct Arg {
    name: &'static str,
    description: &'static str,
}

struct Function {
    name: &'static str,
    description: &'static str,
    return_type: &'static str,
    args: &'static [Arg],
}

impl Function {
    fn show(
        self: &Self,
        level: u8,
        markdown: &mut printer::markdown::Markdown,
    ) -> anyhow::Result<()> {
        markdown.heading(level, format!("{}()", self.name).as_str())?;
        markdown.paragraph(
            format!(
                "`def {}({}) -> {}`",
                self.name,
                self.args
                    .iter()
                    .map(|arg| arg.name)
                    .collect::<Vec<&str>>()
                    .join(", "),
                self.return_type
            )
            .as_str(),
        )?;

        for arg in self.args {
            markdown.list_item(1, format!("`{}`: {}", arg.name, arg.description).as_str())?;
        }

        markdown.printer.newline()?;
        markdown.paragraph(self.description)?;

        Ok(())
    }
}

fn show_rule(markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.list_item(1, "`rule`: dict")?;
    markdown.list_item(2, "`name`: rule name as string")?;
    markdown.list_item(2, "`deps`: list of dependencies")?;
    markdown.list_item(2, "`type`: Setup|Run (default)|Optional")?;
    markdown.list_item(3, "`Setup`: always run before all non-setup rules")?;
    markdown.list_item(3, "`Run`: run as part of `spaces run`")?;
    markdown.list_item(3, "`Optional`: only run if required")?;
    Ok(())
}

fn show_completions(markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(1, "Completions")?;

    Ok(())
}

fn show_run_add_exec(level: u8, markdown: &mut printer::markdown::Markdown) -> anyhow::Result<()> {
    markdown.heading(level, "add_exec()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph(
        "Adds a rule that will execute a process.
The output of the process will be captures in the `spaces_logs` folder.",
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Arguments")?;
    show_rule(markdown)?;
    markdown.list_item(1, "`exec`: dict")?;
    markdown.list_item(2, "`command`: name of the command to execute")?;
    markdown.list_item(2, "`args`: optional list of arguments")?;
    markdown.list_item(2, "`env`: optional dict of environment variables")?;
    markdown.list_item(
        2,
        "`working_directory`: optional working directory (default is the workspace)",
    )?;
    markdown.list_item(
        2,
        "`expect`: optional expected outcome (default is `Success`",
    )?;
    markdown.list_item(
        3,
        "`Failure`: the process is expected to fail (exit status non-zero)",
    )?;
    markdown.list_item(
        3,
        "`Success`: the process is expected to succeed (exit status 0)",
    )?;

    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;

    markdown.code_block(
        "star",
        r#"run.add_exec(
    rule = {"name": name, "type": "Setup", "deps": ["sysroot-python:venv"]},
    exec = {
        "command": "pip3",
        "args": ["install"] + packages,
    },
)"#,
    )?;
    Ok(())
}

fn show_run_add_exec_if(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    markdown.heading(level, "add_exec()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph(
        "Adds a rule to execute. Depending on the
result of the rule (Success or Failure), different dependencies will
be enabled. The `then` and `else` targets must be set to `type: Optional`.",
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Arguments")?;
    show_rule(markdown)?;
    markdown.list_item(1, "`exec_if`: dict")?;
    markdown.list_item(2, "`if`: this is an `exec` object used with add_exec()")?;
    markdown.list_item(
        2,
        "`then`: list of optional targets to enable if the command has the expected result",
    )?;
    markdown.list_item(2, "`else`: optional list of optional targets to enable if the command has the unexpected result")?;

    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;

    markdown.code_block(
        "star",
        r#"run.add_exec(
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
)"#,
    )?;
    Ok(())
}

fn show_run_add_target(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    markdown.heading(level, "add_exec()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph(
        "Adds a target. There
is no specific action for the target, but this rule
can be useful for organizing depedencies.",
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Arguments")?;
    show_rule(markdown)?;

    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;

    markdown.code_block(
        "star",
        r#"run.add_target(
    rule = {"name": "my_rule", "deps": ["my_other_rule"]},
)"#,
    )?;
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
    markdown.heading(level, "add_repo()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph("Adds a repository to the workspace.")?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Arguments")?;
    markdown.list_item(1, "`rule`: dict")?;
    markdown.list_item(2, "`name`: rule name as string")?;
    markdown.list_item(2, "`deps`: list of dependencies")?;
    markdown.list_item(1, "`repo`: dict")?;
    markdown.list_item(2, "`url`: ssh or https path to repository")?;
    markdown.list_item(2, "`rev`: repository revision as a branch, tag or commit")?;
    markdown.list_item(2, "`checkout`:")?;
    markdown.list_item(3, "`Revision`: checkout repository as a detached revision")?;
    markdown.list_item(3, "`NewBranch`: create a new branch based at `Revision`")?;
    markdown.list_item(2, "`clone`: optional value defaults to `Default`")?;
    markdown.list_item(3, "`Default`: standard clone of the repository")?;
    markdown.list_item(
        2,
        "`Worktree`: clone as a worktree using a bare repository in the spaces store",
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;

    markdown.code_block(
        "star",
        r#"checkout.add_repo(
    # the rule name is also the path in the workspace where the clone will be
    rule = { "name": "spaces" },
    repo = {
        "url": "https://github.com/work-spaces/spaces",
        "rev": "main",
        "checkout": "Revision",
        "clone": "Default",
    }
)"#,
    )?;

    Ok(())
}

fn show_checkout_add_archive(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    markdown.heading(level, "add_archive()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph("Adds an archive to the workspace.")?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Arguments")?;
    markdown.list_item(1, "`rule`: dict")?;
    markdown.list_item(2, "`name`: rule name as string")?;
    markdown.list_item(2, "`deps`: list of dependencies")?;
    markdown.list_item(1, "`archive`: dict")?;
    markdown.list_item(2, "`url`: url to zip|tar.xz|tar.gz|tar.bz2 file (can also be an uncompressed file with no suffix)")?;
    markdown.list_item(2, "`sha256`: hash of the file")?;
    markdown.list_item(2, "`link`:")?;
    markdown.list_item(3, "`None`: use `Hard`")?;
    markdown.list_item(
        3,
        "`Hard`: create hardlinks of the archive from the spaces store to the workspace",
    )?;
    markdown.list_item(2, "`includes`: options list of globs to include")?;
    markdown.list_item(2, "`excludes`: optional list of globs to exclude")?;
    markdown.list_item(
        2,
        "`strip_prefix`: optional prefix to strip from the archive path",
    )?;
    markdown.list_item(
        2,
        "`add_prefix`: optional prefix to add in the workspace (e.g. sysroot/share)",
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;

    markdown.code_block(
        "star",
        r#"checkout.add_archive(
    # the rule name is the path in the workspace where the archive will be extracted
    rule = {"name": "llvm-project"},
    archive = {
        "url": "https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-{}.zip".format(version),
        "sha256": "27b5c7c745ead7e9147c78471b9053d4f6fc3bed94baf45f4e8295439f564bb8",
        "link": "Hard",
        "strip_prefix": "llvm-project-llvmorg-{}".format(version),
        "add_prefix": "llvm-project",
    },
)"#,
    )?;

    Ok(())
}

fn show_checkout_add_platform_archive(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    markdown.heading(level, "add_platform_archive()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph(
        r#"Adds an archive to the workspace based on the platform. 
This rule is used for adding tools (binaries to the sysroot folder)."#,
    )?;
    markdown.heading(level + 1, "Arguments")?;
    markdown.list_item(1, "`rule`: dict")?;
    markdown.list_item(2, "`name`: rule name as string")?;
    markdown.list_item(2, "`deps`: list of dependencies")?;
    markdown.list_item(1, "`platforms`: dict")?;
    markdown.list_item(
        2,
        "`macos_aarch64`: dict with same entries as archive in add_archive()",
    )?;
    markdown.list_item(2, "`macos_x86_64`: dict")?;
    markdown.list_item(2, "`windows_aarch64`: dict")?;
    markdown.list_item(2, "`windows_x86_64`: dict")?;
    markdown.list_item(2, "`linux_aarch64`: dict")?;
    markdown.list_item(2, "`linux_x86_64`: dict")?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;

    markdown.code_block(
        "star",
        r#"base = {
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
)"#)?;
    Ok(())
}

fn show_checkout_add_which_asset(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    markdown.heading(level, "add_which_asset()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph(
        r#"Adds a hardlink to an executable file available on the `PATH` 
when checking out the workspace. This is useful for building tools that have complex dependencies.
Avoid using this when creating a workspace for your project. It creates system dependencies
that break workspace hermicity."#,
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Arguments")?;
    markdown.list_item(1, "`rule`: dict")?;
    markdown.list_item(2, "`name`: rule name as string")?;
    markdown.list_item(2, "`deps`: list of dependencies")?;
    markdown.list_item(1, "`asset`: dict")?;
    markdown.list_item(2, "`which`: name of system executable to search for")?;
    markdown.list_item(
        2,
        "`destination`: relative path where asset will live in the workspace",
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;
    markdown.code_block(
        "star",
        r#"checkout.add_which_asset(
    rule = { "name": "which_pkg_config" },
    asset = {
        "which": "pkg-config",
        "destination": "sysroot/bin/pkg-config"
    }
)"#,
    )?;
    Ok(())
}

fn show_checkout_add_asset(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    markdown.heading(level, "add_asset()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph(
        r#"Adds a file to the workspace. This is useful for providing
a top-level build file that orchestrates the entire workspace. It can also
be used to create a top-level README how the workflow works."#,
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Arguments")?;
    markdown.list_item(1, "`rule`: dict")?;
    markdown.list_item(2, "`name`: rule name as string")?;
    markdown.list_item(2, "`deps`: list of dependencies")?;
    markdown.list_item(1, "`asset`: dict")?;
    markdown.list_item(2, "`content`: file contents as a string")?;
    markdown.list_item(
        2,
        "`destination`: relative path where asset will live in the workspace",
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;
    markdown.code_block(
        "star",
        r#"content = """
# README

This is how to use this workspace.

"""

checkout.add_asset(
    rule = {"name": "README.md"},
    asset = {
        "destination": "README.md",
        "content": content,
    },
)"#,
    )?;
    Ok(())
}

fn show_checkout_update_asset(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
    markdown.heading(level, "update_asset()")?;
    markdown.heading(level + 1, "Description")?;
    markdown.paragraph(
        r#"Creates or updates an existing file containing structured data
in the workspace. This rules supports json|toml|yaml files. Different rules
can update the same file and the content will be preserved (as long as the keys are unique)."#,
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Arguments")?;
    markdown.list_item(1, "`rule`: dict")?;
    markdown.list_item(2, "`name`: rule name as string")?;
    markdown.list_item(2, "`deps`: list of dependencies")?;
    markdown.list_item(1, "`asset`: dict")?;
    markdown.list_item(2, "`destination`: path to the asset in the workspace")?;
    markdown.list_item(2, "`format`: json|toml|yaml")?;
    markdown.list_item(
        2,
        "`value`: dict containing the structured data to be added to the asset",
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;
    markdown.code_block(
        "star",
        r#"cargo_vscode_task = {
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
)"#,
    )?;
    Ok(())
}

fn show_checkout_update_env(
    level: u8,
    markdown: &mut printer::markdown::Markdown,
) -> anyhow::Result<()> {
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
    )?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Arguments")?;
    markdown.list_item(1, "`rule`: dict")?;
    markdown.list_item(2, "`name`: rule name as string")?;
    markdown.list_item(2, "`deps`: list of dependencies")?;
    markdown.list_item(1, "`env`: dict")?;
    markdown.list_item(2, "`vars`: dict of variables to add to the environment")?;
    markdown.list_item(2, "`paths`: list of paths required")?;
    markdown.printer.newline()?;
    markdown.heading(level + 1, "Example")?;
    markdown.code_block(
        "star",
        r#"checkout.update_env(
    rule = {"name": "update_env"},
    env = {
        "paths": ["/usr/bin", "/bin"],
        "vars": {
            "PS1": '"(spaces) $PS1"',
        },
    },
)"#,
    )?;
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

    const FUNCTIONS: &[Function] = &[
        Function {
            name: "get_platform_name",
            description: "returns the name of the current platform",
            return_type: "str",
            args: &[],
        },
        Function {
            name: "is_platform_windows",
            description: "returns true if platform is Windows",
            return_type: "bool",
            args: &[],
        },
        Function {
            name: "is_platform_macos",
            description: "returns true if platform is macos",
            return_type: "bool",
            args: &[],
        },
        Function {
            name: "is_platform_linux",
            description: "returns true if platform is linux",
            return_type: "bool",
            args: &[],
        },
        Function {
            name: "get_path_to_store",
            description: "returns the path to the spaces store (typically $HOME/.spaces/store)",
            return_type: "str",
            args: &[],
        },
        Function {
            name: "get_absolute_path_to_workspace",
            description: "returns the absolute path to the workspace",
            return_type: "str",
            args: &[],
        },
        Function {
            name: "get_path_to_checkout",
            description: "returns the path where the current script is located in the workspace",
            return_type: "str",
            args: &[],
        },
        Function {
            name: "get_path_to_build_checkout",
            description: "returns the path to the workspace build folder for the current script",
            return_type: "str",
            args: &[],
        },
        Function {
            name: "get_path_to_build_archive",
            description:
                "returns the path to where run.create_archive() creates the output archive",
            return_type: "str",
            args: &[],
        },
    ];

    markdown.heading(level + 1, "Functions")?;

    for function in FUNCTIONS {
        function.show(level + 2, markdown)?;
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

    const FS_FUNCTIONS: &[Function] = &[
        Function {
            name: "write_string_to_file",
            description: "Writes a string to a file. Truncates the file if it exists. Creates it if it doesn't.",
            return_type: "None",
            args: &[Arg{
                name : "path",
                description: "path relative to the workspace root",
            },
            Arg{
                name : "content",
                description: "contents to write",
            }],
        },
        Function {
            name: "append_string_to_file",
            description: "Appends a string to a file. Creates the file if it doesn't exist.",
            return_type: "None",
            args: &[Arg{
                name : "path",
                description: "path relative to the workspace root",
            },
            Arg{
                name : "content",
                description: "contents to write",
            }],        },
        Function {
            name: "read_file_to_string",
            description: "Reads the contents of the file as a string",
            return_type: "str",
            args: &[Arg{
                name : "path",
                description: "path relative to the workspace root",
            }]
        },
        Function {
            name: "exists",
            description: "Checks if the file/directory exists",
            return_type: "bool",
            args: &[Arg{
                name : "path",
                description: "path relative to the workspace root",
            }]
        },
        Function {
            name: "read_toml_to_dict",
            description: "Reads and parses a toml file",
            return_type: "str",
            args: &[Arg{
                name : "path",
                description: "path relative to the workspace root",
            }]        },
        Function {
            name: "read_yaml_to_dict",
            description: "Reads and parses a yaml file",
            return_type: "dict with parsed yaml",
            args: &[Arg{
                name : "path",
                description: "path relative to the workspace root",
            }]        },
        Function {
            name: "read_json_to_dict",
            description: "Reads and parses a json file",
            return_type: "dict with parsed json",
            args: &[Arg{
                name : "path",
                description: "path relative to the workspace root",
            }]        },
    ];

    for function in FS_FUNCTIONS {
        function.show(level + 2, markdown)?;
    }

    markdown.heading(level + 1, "`process` Functions")?;
    const PROCESS_FUNCTIONS: &[Function] = &[
        Function {
            name: "exec",
            description: "Writes a string to a file. Truncates the file if it exists. Creates it if it doesn't.",
            return_type: "dict with `status`, `stdout`, and `stderr`",
            args: &[Arg{
                name : "exec",
                description: "dict defining the process to execute. See below for details.",
            },
            Arg{
                name : "content",
                description: "contents to write",
            }],
        },
    ];

    for function in PROCESS_FUNCTIONS {
        function.show(level + 2, markdown)?;
    }

    markdown.heading(level + 2, "`exec` arguments")?;
    markdown.paragraph("`exec` takes a single dict argument with the following keys:")?;
    markdown.list_item(1, "`command`: name of the command to execute")?;
    markdown.list_item(1, "`args`: optional list of arguments")?;
    markdown.list_item(1, "`env`: optional dict of environment variables")?;
    markdown.list_item(
        1,
        "`working_directory`: optional working directory (default is the workspace)",
    )?;
    markdown.list_item(
        1,
        "`stdin`: optional string to pipe to the process stdin",
    )?;

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
    markdown.heading(1, "Spaces Documentation")?;
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
