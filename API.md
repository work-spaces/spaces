# Spaces Built-in Functions API Documentation


- [Checkout Rules](#checkout-rules)
- [Run Rules](#run-rules)
- [Info Functions](#info-functions)
- [Spaces Starlark Standard Functions](#spaces-starlark-standard-functions)

## Checkout Rules

You use checkout rules to build a workspace.
You can fetch git repositories and archives. You can also add assets (local files)
to the workspace root folder (not under version control).

#### abort

```python
def abort(message) -> None
```
Abort script evaluation with a message.

- `message`: Abort message to show the user.


**Example**
```python
run.abort("Failed to do something")
```
#### add_archive

```python
def add_archive(rule, archive) -> None
```
Adds an archive to the workspace.

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `archive`: dict value
  - `url`: url to zip|tar.xz|tar.gz|tar.bz2 file (can also be an uncompressed file with no suffix)
  - `sha256`: hash of the file
  - `link`: None|Hard: create hardlinks of the archive from the spaces store to the workspace
  - `includes`: options list of globs to include
  - `excludes`: optional list of globs to exclude
  - `strip_prefix`: optional prefix to strip from the archive path
  - `add_prefix`: optional prefix to add in the workspace (e.g. sysroot/share)


**Example**
```python
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
#### add_asset

```python
def add_asset(rule, asset) -> None
```
Adds a file to the workspace. This is useful for providing
a top-level build file that orchestrates the entire workspace. It can also
be used to create a top-level README how the workflow works.

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `asset`: dict with
  - `content`: file contents as a string
  - `destination`: relative path where asset will live in the workspace


**Example**
```python
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
#### add_cargo_bin

```python
def add_cargo_bin(rule, cargo_bin) -> str
```
Adds a binary crate using cargo-binstall. The binaries are installed in the spaces store and hardlinked to the workspace.

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `cargo_bin`: dict with
  - `crate`: The name of the binary crate
  - `version`: The crate version to install
  - `bins`: List of binaries to install


**Example**
```python
checkout.add_cargo_bin(
    rule = {"name": "probe-rs-tools"},
    cargo_bin = {
        "crate": "probe-rs-tools", 
        "version": "0.24.0", 
        "bins": ["probe-rs", "cargo-embed", "cargo-flash"]
    },
)
```
#### add_hard_link_asset

```python
def add_hard_link_asset(rule, asset) -> None
```
Adds a hardlink from anywhere on the system to the workspace

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `asset`: dict with
  - `source`: the source of the hard link
  - `destination`: relative path where asset will live in the workspace


**Example**
```python
checkout.add_hard_link_asset(
    rule = { "name": "which_pkg_config" },
    asset = {
        "source": "<path to asset>",
        "destination": "sysroot/asset/my_asset"
    }
)
```
#### add_platform_archive

```python
def add_platform_archive(rule, platforms) -> None
```
Adds an archive to the workspace based on the platform.

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `platforms`: dict with platform keys
  - `macos-aarch64`: dict with same entries as archive in add_archive()
  - `macos-x86_64`: same as macos-aarch64
  - `windows-aarch64`: same as macos-aarch64
  - `windows-x86_64`: same as macos-aarch64
  - `linux-aarch64`: same as macos-aarch64
  - `linux-x86_64`: same as macos-aarch64


**Example**
```python
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
        "macos-x86_64": macos_x86_64,
        "macos-aarch64": macos_aarch64,
        "windows-x86_64": windows_x86_64,
        "windows-aarch64": windows_aarch64,
        "linux-x86_64": linux_x86_64,
    },
)
```
#### add_repo

```python
def add_repo(rule, repo) -> str
```
returns the name of the current platform

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `repo`: dict with
  - `url`: ssh or https path to repository
  - `rev`: repository revision as a branch, tag or commit
  - `checkout`: Revision: checkout detached at commit or branch|NewBranch: create a new branch based at rev
  - `clone`: Default|Worktree


**Example**
```python
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
#### add_target

```python
def add_target(rule) -> None
```
Adds a target. There is no specific action for the target, but this rule can be useful for organizing depedencies.

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`


**Example**
```python
run.add_target(
    rule = {"name": "my_rule", "deps": ["my_other_rule"]},
)
```
#### add_which_asset

```python
def add_which_asset(rule, asset) -> None
```
Adds a hardlink to an executable file available on the `PATH` 
when checking out the workspace. This is useful for building tools that have complex dependencies.
Avoid using this when creating a workspace for your project. It creates system dependencies
that break workspace hermicity.

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `asset`: dict with
  - `which`: name of system executable to search for
  - `destination`: relative path where asset will live in the workspace


**Example**
```python
checkout.add_which_asset(
    rule = { "name": "which_pkg_config" },
    asset = {
        "which": "pkg-config",
        "destination": "sysroot/bin/pkg-config"
    }
)
```
#### update_asset

```python
def update_asset(rule, asset) -> None
```
Creates or updates an existing file containing structured data
in the workspace. This rules supports json|toml|yaml files. Different rules
can update the same file and the content will be preserved (as long as the keys are unique).

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `asset`: dict with
  - `destination`: path to the asset in the workspace
  - `format`: json|toml|yaml
  - `value`: dict containing the structured data to be added to the asset


**Example**
```python
cargo_vscode_task = {
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
)
```
#### update_env

```python
def update_env(rule, env) -> None
```
Creates or updates the environment file in the workspace.

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



- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `env`: dict with
  - `vars`: dict of variables to add to the environment
  - `paths`: list of paths required


**Example**
```python
checkout.update_env(
    rule = {"name": "update_env"},
    env = {
        "paths": ["/usr/bin", "/bin"],
        "vars": {
            "PS1": '"(spaces) $PS1"',
        },
    },
)
```

## Run Rules

You use run rules to execute tasks in the workspace.

#### abort

```python
def abort(message) -> None
```
Abort script evaluation with a message.

- `message`: Abort message to show the user.


**Example**
```python
run.abort("Failed to do something")
```
#### add_exec

```python
def add_exec(rule, exec) -> None
```
Adds a rule that will execute a process.

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `exec`: dict with
  - `command`: name of the command to execute
  - `args`: optional list of arguments
  - `env`: optional dict of environment variables
  - `working_directory`: optional working directory (default is the workspace)
  - `expect`: Failure: expect non-zero return code|Success: expect zero return code


**Example**
```python
run.add_exec(
    rule = {"name": name, "type": "Setup", "deps": ["sysroot-python:venv"]},
    exec = {
        "command": "pip3",
        "args": ["install"] + packages,
    },
)
```
#### add_exec_if

```python
def add_exec_if(rule, exec_if) -> None
```
Adds a rule to execute if a condition is met.

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`
- `exec_if`: dict with
  - `if`: this is an `exec` object used with add_exec()
  - `then`: list of optional targets to enable if the command has the expected result
  - `else`: optional list of optional targets to enable if the command has the unexpected result


**Example**
```python
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
    }
)
```
#### add_target

```python
def add_target(rule) -> None
```
Adds a target. There is no specific action for the target, but this rule can be useful for organizing depedencies.

- `rule`: dict
  - `name`: rule name as string
  - `deps`: list of dependencies
  - `type`: Setup|Run (default)|Optional
  - `help`: Optional help text show with `spaces evaluate`


**Example**
```python
run.add_target(
    rule = {"name": "my_rule", "deps": ["my_other_rule"]},
)
```

## Info Functions

### Description

The `info` functions provide information about the workspace
during checkout and run. Info functions are executed immediately. They are not rule definitions.

### Functions

#### get_absolute_path_to_workspace

```python
def get_absolute_path_to_workspace() -> str
```
returns the absolute path to the workspace


#### get_cpu_count

```python
def get_cpu_count() -> int
```
returns the number of CPUs on the current machine


#### get_path_to_build_archive

```python
def get_path_to_build_archive() -> str
```
returns the path to where run.create_archive() creates the output archive


#### get_path_to_build_checkout

```python
def get_path_to_build_checkout() -> str
```
returns the path to the workspace build folder for the current script


#### get_path_to_checkout

```python
def get_path_to_checkout() -> str
```
returns the path where the current script is located in the workspace


#### get_path_to_store

```python
def get_path_to_store() -> str
```
returns the path to the spaces store (typically $HOME/.spaces/store)


#### get_platform_name

```python
def get_platform_name() -> str
```
returns the name of the current platform: macos-aarch64|macos-x86_64|linux-x86_64|linux-aarch64|windows-x86_64|windows-aarch64


#### is_platform_linux

```python
def is_platform_linux() -> bool
```
returns true if platform is linux


#### is_platform_macos

```python
def is_platform_macos() -> bool
```
returns true if platform is macos


#### is_platform_windows

```python
def is_platform_windows() -> bool
```
returns true if platform is Windows


#### set_minimum_version

```python
def set_minimum_version() -> int
```
sets the minimum version of spaces required to run the script



## Spaces Starlark Standard Functions

### Description

The spaces starlark standard library includes
functions for doing things like accessing the filesystem. The functions
in this library are executed immediately.

### `fs` Functions

#### append_string_to_file

```python
def append_string_to_file(path, content) -> None
```
Appends a string to a file. Creates the file if it doesn't exist.

- `path`: path relative to the workspace root
- `content`: contents to write

#### exists

```python
def exists(path) -> bool
```
Checks if the file/directory exists

- `path`: path relative to the workspace root

#### read_directory

```python
def read_directory(path) -> [str]
```
Reads the entries of a directory

- `path`: path relative to the workspace root

#### read_file_to_string

```python
def read_file_to_string(path) -> str
```
Reads the contents of the file as a string

- `path`: path relative to the workspace root

#### read_json_to_dict

```python
def read_json_to_dict(path) -> dict with parsed json
```
Reads and parses a json file

- `path`: path relative to the workspace root

#### read_toml_to_dict

```python
def read_toml_to_dict(path) -> str
```
Reads and parses a toml file

- `path`: path relative to the workspace root

#### read_yaml_to_dict

```python
def read_yaml_to_dict(path) -> dict with parsed yaml
```
Reads and parses a yaml file

- `path`: path relative to the workspace root

#### write_string_to_file

```python
def write_string_to_file(path, content) -> None
```
Writes a string to a file. Truncates the file if it exists. Creates it if it doesn't.

- `path`: path relative to the workspace root
- `content`: contents to write

### `hash` Functions

#### compute_sha256_from_file

```python
def compute_sha256_from_file(file_path) -> String
```
Computes the sha256 checksum for the contents of a file and returns the digest as a string.

- `file_path`: path to the file

### `json` Functions

#### string_to_dict

```python
def string_to_dict(content) -> dict
```
Converts a JSON formatted string to a dict.

- `content`: The JSON string to convert

#### to_string

```python
def to_string(value) -> dict
```
Converts a dict to a JSON formatted string.

- `value`: The Starlark value to convert

#### to_string_pretty

```python
def to_string_pretty(value) -> dict
```
Converts a dict to a JSON formatted string (multi-line, idented).

- `value`: The Starlark value to convert

### `process` Functions

#### exec

```python
def exec(exec) -> dict # with members `status`, `stdout`, and `stderr`
```
Executes a process

- `exec`: dict with members
  - `command`: name of the command to execute
  - `args`: optional list of arguments
  - `env`: optional dict of environment variables
  - `working_directory`: optional working directory (default is the workspace)
  - `stdin`: optional string to pipe to the process stdin

### `script` Functions

#### get_arg

```python
def get_arg(offset) -> str
```
Gets the argument at the specified offset (an empty string is returned if the argument doesn't exist).

- `offset`: int: offset of the argument to get.

#### get_args

```python
def get_args(offset) -> dict
```
Gets the arguments as a dict with 'ordered' and 'named' keys. `ordered` is a list of arguments that do not contain =, `named` is a map of key value pairs separated by =.

- `offset`: int: offset of the argument to get.

#### print

```python
def print(content) -> None
```
Prints a string to the stdout. Only use in a script.

- `content`: str: string content to print.

#### set_exit_code

```python
def set_exit_code(offset) -> none
```
Sets the exit code of the script. 
Use zero for success and non-zero for failure.
This doesn't exit the script.

- `offset`: int: offset of the argument to get.



