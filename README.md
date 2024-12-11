# spaces

## Try it Now

Quickly create a `python3.11` virtual environment usinv `uv`.

```sh
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/work-spaces/install-spaces/refs/heads/main/install.sh)"
export PATH=$HOME/.local/bin:$PATH
git clone https://github.com/work-spaces/workflows/
spaces checkout --script=workflows/preload --script=workflows/python-sdk --name=python-quick-test
cd python-quick-test
spaces run
source env
python -c "print('hello')"
```

## About Spaces

How do you get all the tools and source code at the right version for your project? How do you ensure everyone who checks out your code has all the same tools and dependencies?

Some common options include: 

- Docker. Put all the tools and dependencies in a container and you are set.
- Monorepos. Commit all source code to one big repo. 
  - Use additional tools like `nix` or `dotslash` to manage executables.
- Give everyone a powerful workstation running the same version of Linux.
- Package managers such as `apt`, `brew`, or `choco`.

Finding the one that is just right for you is tricky. 

`spaces` is a lightweight solution that lets you create a workspace with:

- Code you need to developer
- Source and binary dependencies
- Executable tools

All are precisely versioned and efficiently managed sharing tools and repos in the spaces store.

`spaces` is a single binary. It is powered by `starlark` and `rust`. `starlark` is a python dialect that lets you write expressive rules to:

- `checkout` source code and tools to your workspace
  - All artifacts are hashed ensuring each workspace is identical.
- `run` your depedency based workflow

All projects use the same commands:

```sh
spaces checkout --script=<your script> --name=<your workspace folder>
cd <your workspace folder>
spaces run

# you can do inner-loop developement from the command line in the `spaces run` environment using
source env
```

Here is an example from the spaces [workflows repo](https://github.com/work-spaces/workflows/).

```python
# load the rust script from the sysroot repository
# // indicates the workspace root.
load("//@sdk/star/rust.star", "rust_add")
load("//@sdk/star/checkout.star", "checkout_add_repo")
load("//@sdk/star/run.star", "run_add_exec")

# Checkout the spaces repo
checkout_add_repo(
    "spaces",
    url = "https://github.com/work-spaces/spaces",
    rev = "main",
)

# any files matching *.spaces.star within 
# checked-out repos are processed in order

# Grab the rust toolchain
rust_add("rust_toolchain", "1.80")


run_add_exec(
    "builds_spaces",
    command = "cargo",
    working_directory = "spaces",
    args =  [
        "build",
        "--profile=release",
    ],
)
```

The actual workflow example checks out `spaces` as well as two closely coupled crates that are in separate repos.
The checkout script configures IDE settings and cargo overrides to use the local copies of `printer` and `easy-archiver`.

```sh
spaces checkout --script=workflows/preload --script=workflows/spaces-develop --name=spaces-build-test
cd spaces-build-test
spaces run
```

## Installing Spaces

`spaces` is a statically linked binary. Download it in the releases section.

Or run this to install spaces at `$HOME/.local/bin`:

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
- Run Phase: execute user-defined rules to build, test, run or deploy your project

### Spaces Starlark SDK

The [spaces starlark SDK](https://github.com/work-spaces/sdk) is optionally added in a `preload` script. If you want to use any `load` statements
in your checkout scripts, you need to pass two scripts to `spaces checkout`. The first script will simply use `checkout.add_repo()` to add the SDK
repository. When the second script runs, the SDK will be available for use with `load` statements.

### More about Checkout

Typcially two scripts are passed. A `preload` script to checkout the starlark SDK and a workflow script defining
code and tools for the workspace.

```sh
spaces checkout --script=preload --script=my-project --name=build-my-project
```

During checkout, `spaces` populates a workspace by evaulating the checkout rules in the workflow scripts. The workspace will look something like:

- `@sdk`: spaces starlark SDK
- `@packages`: spaces starlark SDK dependency for downloading platform binary archives
- `@sources`: spaces starlark SDK dependency for downloading sources
- `@logs`: log files for task output
- `env`: Use `source env` to configure the command line for development
- `env.spaces.star`: starlark file generated by `spaces` containing env info.
- `preload.spaces.star`: copy of the preload workflow script
- `my-project.spaces.star`: copy of the project workflow script
- `sysroot`: hardlinks to what you normally put in `/usr/local` with precise versions for your project
- source folders: source code repositories checked out based on the rules in `my-project.spaces.star`
- assets: checkout rules can add arbitrary files to the workspace
  - For example, dependecies can populate `.vscode/settings.json`
  - create local git hooks.
  - create hard/soft links to system resources

`checkout.add_repo()` adds a repository to the workspace. `spaces` transitively evaluates scripts matching `*spaces.star` found in checked-out out repos.

### Run phase

Run rules build a dependency graph of targets. Run rules have:

- A Rule:
    - `name`: the way to refer to this task when adding dependencies to other tasks
    - `deps`: explicit dependencies that must run before this task
    - `type`: `Setup`, `Run` (default), or `Optional`. 
     - Non-setup rules depend on `Setup` rules. 
     - `Optional` rules only run if needed (similar to "Exclude from all").
- An Action
    - For example, `run.add_exec()` adds a process (`command` and `args`) to the depedency graph.

You can execute your run rules in the workspace using:

```sh
spaces run
```

To enter the `spaces` execution environment used by `spaces run`, use:

```sh
source env
```

You will have `<workspace>/sysroot/bin` on your path and limited paths on the host system (as specified by the checkout rules).

## Writing Spaces Starlark Script

`starlark` is a dialect of python. 

`starlark` symbols originate from three sources:

- The [standard starlark specification](https://github.com/bazelbuild/starlark/blob/master/spec.md)
- built-ins that call `rust` functions within the `spaces` binary (see the [API](API.md))
- `load()` statements that import variables from other starlark scripts
  - `load()` paths can either be relative to the current `star` file or prefixed with `//` to refer to the workspace root.
