# Spaces Documentation


## Info Functions

The `info` functions provide information about the workspace
during checkout and run. Info functions are executed immediately. They are not rule definitions.

```star
is_windows = info.is_platform_windows()
```


- `fn get_platform_name() -> bool`
  - returns the name of the current platform
- `fn is_platform_windows() -> bool`
  - returns true if platform is Windows
- `fn is_platform_macos() -> bool`
  - returns true if platform is macos
- `fn is_platform_linux() -> bool`
  - returns true if platform is linux
- `fn is_platform_x86_64() -> bool`
  - returns true if platform is x86_64
- `fn is_platform_aarch64() -> bool`
  - returns true if platform is aarch64
- `fn get_path_to_store() -> String`
  - returns the path to the spaces store (typically $HOME/.spaces/store)
- `fn get_absolute_path_to_workspace() -> String`
  - returns the absolute path to the workspace
- `fn get_path_to_checkout() -> String`
  - returns the path where the current script is located in the workspace
- `fn get_path_to_build_checkout() -> String`
  - returns the path to the workspace build folder for the current script
- `fn get_path_to_build_archive() -> String`
  - returns the path to where run.create_archive() creates the output archive

## Checkout Rules

You use checkout rules to build a workspace.
You can fetch git repositories and archives. You can also add assets (local files)
to the workspace root folder (not under version control).

### add_repo()

#### Description

Adds a repository to the workspace.


#### Arguments

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
- `repo`: dict
  - `url`: ssh or https path to repository
  - `rev`: repository revision as a branch, tag or commit
  - `checkout`:
    - `Revision`: checkout repository as a detached revision
    - `NewBranch`: create a new branch based at `Revision`
  - `clone`: optional value defaults to `Default`
    - `Default`: standard clone of the repository
  - `Worktree`: clone as a worktree using a bare repository in the spaces store

#### Example

```star
checkout.add_repo(
    # the rule name is also the path in the workspace where the clone will be
    rule = { "name": "spaces" },
    repo = {
        "url": "https://github.com/work-spaces/spaces",
        "rev": "main",
        "checkout": "Revision",
        "clone": "Default",
    }
)
```
### add_archive()

#### Description

Adds an archive to the workspace.


#### Arguments

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
- `archive`: dict
  - `url`: url to zip|tar.xz|tar.gz|tar.bz2 file (can also be an uncompressed file with no suffix)
  - `sha256`: hash of the file
  - `link`:
    - `None`: use `Hard`
    - `Hard`: create hardlinks of the archive from the spaces store to the workspace
  - `includes`: options list of globs to include
  - `excludes`: optional list of globs to exclude
  - `strip_prefix`: optional prefix to strip from the archive path
  - `add_prefix`: optional prefix to add in the workspace (e.g. sysroot/share)

#### Example

```star
checkout.add_archive(
    # the rule name is the path in the workspace where the archive will be extracted
    rule = {"name": "llvm-project"},
    archive = {
        "url": "https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-{}.zip".format(version),
        "sha256": "27b5c7c745ead7e9147c78471b9053d4f6fc3bed94baf45f4e8295439f564bb8",
        "link": "Hard",
        "strip_prefix": "llvm-project-llvmorg-{}".format(version),
        "add_prefix": "llvm-project",
    },
)
```
### add_platform_archive()

#### Description

Adds an archive to the workspace based on the platform. 
This rule is used for adding tools (binaries to the sysroot folder).

#### Arguments

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
- `platforms`: dict
  - `macos_aarch64`: dict with same entries as archive in add_archive()
  - `macos_x86_64`: dict
  - `windows_aarch64`: dict
  - `windows_x86_64`: dict
  - `linux_aarch64`: dict
  - `linux_x86_64`: dict

#### Example

```star
base = {
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
)
```
### add_which_asset()

#### Description

Adds a hardlink to an executable file available on the `PATH` 
when checking out the workspace. This is useful for building tools that have complex dependencies.
Avoid using this when creating a workspace for your project. It creates system dependencies
that break workspace hermicity.


#### Arguments

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
- `asset`: dict
  - `which`: name of system executable to search for
  - `destination`: relative path where asset will live in the workspace

#### Example

```star
checkout.add_which_asset(
    rule = { "name": "which_pkg_config" },
    asset = {
        "which": "pkg-config",
        "destination": "sysroot/bin/pkg-config"
    }
)
```
### add_asset()

#### Description

Adds a file to the workspace. This is useful for providing
a top-level build file that orchestrates the entire workspace. It can also
be used to create a top-level README how the workflow works.


#### Arguments

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
- `asset`: dict
  - `content`: file contents as a string
  - `destination`: relative path where asset will live in the workspace

#### Example

```star
content = """
# README

This is how to use this workspace.

"""

checkout.add_asset(
    rule = {"name": "README.md"},
    asset = {
        "destination": "README.md",
        "content": content,
    },
)
```
## Run Rules

You use run rules to execute tasks in the workspace.

### add_exec()

#### Description

Adds a rule that will execute a process.
The output of the process will be captures in the `spaces_logs` folder.


#### Arguments

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
    - `Setup`: always run before all non-setup rules
    - `Run`: run as part of `spaces run`
    - `Optional`: only run if required
- `exec`: dict
  - `command`: name of the command to execute
  - `args`: optional list of arguments
  - `env`: optional dict of environment variables
  - `working_directory`: optional working directory (default is the workspace)
  - `expect`: optional expected outcome (default is `Success`
    - `Failure`: the process is expected to fail (exit status non-zero)
    - `Success`: the process is expected to succeed (exit status 0)

#### Example

```star
run.add_exec(
    rule = {"name": name, "type": "Setup", "deps": ["sysroot-python:venv"]},
    exec = {
        "command": "pip3",
        "args": ["install"] + packages,
    },
)
```
### add_exec()

#### Description

Adds a rule to execute. Depending on the
result of the rule (Success or Failure), different dependencies will
be enabled. The `then` and `else` targets must be set to `type: Optional`.


#### Arguments

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
    - `Setup`: always run before all non-setup rules
    - `Run`: run as part of `spaces run`
    - `Optional`: only run if required
- `exec_if`: dict
  - `if`: this is an `exec` object used with add_exec()
  - `then`: list of optional targets to enable if the command has the expected result
  - `else`: optional list of optional targets to enable if the command has the unexpected result

#### Example

```star
run.add_exec(
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
)
```
### add_exec()

#### Description

Adds a target. There
is no specific action for the target, but this rule
can be useful for organizing depedencies.


#### Arguments

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
    - `Setup`: always run before all non-setup rules
    - `Run`: run as part of `spaces run`
    - `Optional`: only run if required

#### Example

```star
run.add_target(
    rule = {"name": "my_rule", "deps": ["my_other_rule"]},
)
```

