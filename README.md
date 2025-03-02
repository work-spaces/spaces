# spaces

## Try it Now

Quickly create a `python3.11` virtual environment usinv `uv`.

```sh
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/work-spaces/install-spaces/refs/heads/main/install.sh)"
export PATH=$HOME/.local/bin:$PATH
git clone https://github.com/work-spaces/workflows/
spaces checkout --workflow=workflows:lock,preload,python-sdk --name=python-quick-test
cd python-quick-test
spaces run
source ./env
python -c "print('hello')"
```

## About Spaces

How do you ensure everyone who checks out your code has all the same tools and dependencies?

Some common options include: 

- Docker. Put all the tools and dependencies in a container and you are set.
- Monorepos. Commit all source code to one big repo. 
  - Use additional tools like `nix` or `dotslash` to manage executables.
- Use your build system (e.g. `cmake`) to download and build depedencies
- Package managers such as `apt`, `brew`, or `choco`.
- Metabuild options such as `bitbake` or `buildstream`.

Finding the right one is challenging. 

`spaces` is a lightweight solution that lets you create a workspace with:

- Code you need to develop
- Source and/or binary dependencies
- Executable tools

Downloaded artifacts are hashed and managed in the `spaces` store for efficient sharing across projects.

`spaces` is a single binary. It is powered by `starlark` and `rust`. `starlark` is a python dialect that lets you write expressive rules to:

- `checkout` source code and tools to your workspace
- `run` tasks based on a dependency graph

All workflows use the same commands:

```sh
spaces checkout --workflow=<workflow directory>:<workflow script>,... --name=<workspace folder name>
cd <workspace folder name>
spaces run

# you can do inner-loop developement from the command line in the `spaces run` environment using
source ./env
```

Here is an abbreviated example from the spaces [workflows repo](https://github.com/work-spaces/workflows/).

```python
# load the rust script from the sysroot repository
# // indicates the workspace root.
load("//@star/packages/star/rust.star", "rust_add")
load("//@star/sdk/star/checkout.star", "checkout_add_repo")
load("//@star/sdk/star/run.star", "run_add_exec")

# Checkout the spaces repo
checkout_add_repo(
    "spaces",
    url = "https://github.com/work-spaces/spaces",
    rev = "main",
)

# Grab the rust toolchain
rust_add("rust_toolchain", "1.80")

run_add_exec(
    "build_spaces",
    command = "cargo",
    working_directory = "spaces",
    args =  [
        "build",
        "--profile=release",
    ],
)
```

## Installing Spaces

`spaces` is a statically linked binary. Download it in the releases section.

Or install `spaces` at `$HOME/.local/bin`:

```sh
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/work-spaces/install-spaces/refs/heads/main/install.sh)"
```

The command above requires `curl`, `unzip` and `sed`.

Or install from source using `cargo`:

```sh
git clone https://github.com/work-spaces/spaces
cd spaces
cargo install --path=crates/spaces --root=$HOME/.local --profile=release
```

Use `spaces` in github actions with https://github.com/work-spaces/install-spaces.

## Using Spaces

`spaces` executes `starlark` scripts in two phases:

- Checkout Phase: collects everything from source code to tools into a workspace folder
    - `git` repos are checked out in the workspace (either direct clones or using worktrees)
    - Archives (including platform binaries) are downloaded to `$HOME/.spaces/store`. Contents are hardlinked to the workspace `sysroot`.
- Run Phase: execute user-defined rules to build, test, run and/or deploy your project

### Spaces Starlark SDK

The `spaces` starlark [SDK](https://github.com/work-spaces/sdk) is added in a `preload` script during checkout. `spaces checkout` can process multiple scripts in order. The first script cannot use any `load` statements because nothing has been populated in the workspace. The first script populates the workspace with the SDK. Subsequent checkout scripts can `load` functions populated in the workspace by preceding scripts.

```sh
spaces checkout --script=preload --script=my-project --name=build-my-project
```

```python
# preload.spaces.star

# checkout.add_repo() is a built-in: use `spaces docs` to see documentation of built-in functions
# checkout_add_repo() is a convenience wrapper function defined in https://github.com/work-spaces/sdk.
# Scripts that run after this one, can use `load("//@star/sdk/star/checkout.star", "checkout_add_repo")`
# instead of calling the built-in directly.
checkout.add_repo(
    rule = {"name": "@star/sdk"},  # stores this repo in the workspaces at `@star/sdk`
                                   #   the `@star` folder is a conventional location for
                                   #   common, loadable starlark code
    repo = {
        "url": "https://github.com/work-spaces/sdk",
        "rev": "main",
        "checkout": "Revision",
        "clone": "Blobless"
    }
)
```

```python
# my-project.spaces.star

load("//@star/sdk/star/checkout.star", "checkout_add_repo")

# This is easier to use than checkout.add_repo() but isn't available in the initial script
checkout_add_repo(
  "my-project",
  url = "https://github.com/my-org/my-project",
  rev = "main
)
```


### More About Checkout

During checkout, `spaces` populates a workspace by evaulating the checkout rules. The workspace will look something like:

- `@star`: dedicated folder for preloaded starlark files
- `.spaces`: folder with logs and config files
- `env`: Use `source ./env` to configure the command line for inner-loop development
- `env.spaces.star`: starlark file generated by `spaces` containing env info.
- `preload.spaces.star`: copy of the preload workflow script
- `my-project.spaces.star`: copy of the project workflow script
- `sysroot`: hardlinks to what you normally put in `/usr/local` with precise versions for your project
- source folders: source code repositories checked out based on the rules in `my-project.spaces.star`
- assets: checkout rules can add arbitrary files to the workspace
  - For example, rules can populate `.vscode/settings.json` for easy IDE bring-up
  - create local git hooks.
  - create hard/soft links to system resources

`checkout.add_repo()` adds a repository to the workspace. `spaces` transitively evaluates scripts matching `*spaces.star` found in the root folder of checked-out repos.

### Run phase

Run rules build a dependency graph of targets. Run rules have:

- A Rule:
  - `name`: the way to refer to this task when adding dependencies to other tasks
  - `deps`: explicit dependencies that must run before this task
  - `type`: `Setup`, `Run` (default), or `Optional`. 
    - Non-setup rules depend on `Setup` rules. 
    - `Optional` rules only run if needed (similar to "Exclude from all").
  - `inputs`: globs for specifying rule inputs. 
    - Use `None` (the default) to run the rule everytime
    - Use `[]` to run the rule once (nice for `Setup` rules)
    - Use a glob such as `["+my-project/**", "-my-project/tmp/**"]` to run if any matching changes
- An Action
    - For example, `run.add_exec()` adds a process (`command` and `args`) to the depedency graph.

Execute all non-optional rules (plus dependencies) using:

```sh
spaces run
```

Execute a specific rule plus dependencies:

```sh
spaces run my-project:build
```

To enter the `spaces` execution environment used by `spaces run`, use:

```sh
source ./env
```

## Writing Spaces Starlark Script

`starlark` is a dialect of python. `starlark` symbols originate from three sources:

- The [standard starlark specification](https://github.com/bazelbuild/starlark/blob/master/spec.md)
- built-ins that call `rust` functions within the `spaces` binary (see the [API](API.md))
  - All the commonly used spaces built-ins have function wrappers defined in the [SDK](https://github.com/work-spaces/sdk)
- `load()` statements that import variables or functions from other starlark scripts
  - `load()` paths can either be relative to the current `star` file or prefixed with `//` to refer to the workspace root.

### Understanding Paths in spaces

Paths for `load()`, `working_directory` and rule names can be either relative to the spaces file where they are declared or relative to the workspace root. Paths prefixed with `//` are always relative to the workspace.

#### `load()` Paths

By convention, the [SDK](https://github.com/work-spaces/sdk) is loaded into the workspace at `@star/sdk`. Files can be loaded from the SDK using, for example:

```python
load("//@star/sdk/star/info.star", "info_set_minimum_version")
```

Within `run.star` which is a sibling of `info.star`:

```python
load("info.star", "info_set_minimum_version")
```

#### `working_directory` Paths

The example below shows how `spaces` treats the `working_directory` value in run rules.

```python
load("//@star/sdk/star/run.star", "run_add_exec")

# Run in the same directory as the containing file
run_add_exec(
  "list_directory",
  command = "ls",
  working_directory = "."
)

# To execute in the workspace build folder:
run_add_exec(
  "list_build_directory",
  command = "ls",
  working_directory = "//build"
)

# The default behavior is to execute in the workspace root
run_add_exec(
  "list_workspace_directory",
  command = "ls",
)
```

#### Rule Paths

If the above rules are defined in the workspace at `my-project/spaces.star`, they are run using the commands below. `[.]spaces.star` is converted to `:`.

```sh
# from workspace root
spaces run //my-project:list_directory
cd my-project
spaces run :list_directory
```

If the rules are in `my-project/show.spaces.star`, they are run using:

```sh
# from workspace root
spaces run //my-project/show:list_directory
cd my-project
spaces run show:list_directory
```

### Adding Checkout Rules

The most common way to add source code is using `checkout_add_repo()`. Here is an example:

```python
load("//@star/sdk/star/checkout.star", "checkout_add_repo", "CHECKOUT_CLONE_BLOBLESS")

checkout_add_repo(
  "spaces",                # name of the rule and location of the repo in the workspace
  url = "https://github.com/work-spaces/spaces", # url to clone
  clone = CHECKOUT_CLONE_BLOBLESS  # use a blobless clone
)
```

### Adding Run Rules

The most common run rule is to execute a shell command using `run_add_exec()`.

```python

load("//@star/sdk/star/run.star", "run_add_exec", "RUN_LOG_LEVEL_APP)

run_add_exec(
  "show",                   # name of the rule
  command = "ls",           # command to execute in the shell
  args = ["-alt"],          # arguments to pass to ls
  working_directory = ".",  # execute in the directory where this rule is
                            #   default is to execute at the workspace root
  deps = ["another_rule"],  # run this rule after `another_rule` completes
  log_level = RUN_LOG_LEVEL_APP  # Show the output of the rule to the user
)
```

## Uninstall Spaces

- Delete the binary: `rm $HOME/.local/bin/spaces`.
- Delete the store: `rm -rf $HOME/.spaces`
