# spaces

## Try it Now

Quickly create a `python3.12` virtual environment usinv `uv`.

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

How do you get all the tools and source code at the right version to build, debug, and deploy your project? How do you ensure everyone who checks out your code has all the same tools and dependencies?

Some common options include: 

- Docker. Put all the tools and dependencies in a container and you are set.
- Monorepos. Commit all source code to one big repo. Use additional tools like `nix` or `dotslash` to manage executables.
- Give everyone a powerful workstation running the same version of Linux.

These solutions have drawbacks. Docker runs a VM on mac and windows creating a heavyweight solution that creates a second class developer experience. Monorepos can get so large they require specialized tooling to manage (worktrees, partial clones, virtual filesystems). Monorepos can put a heavy cognitive load on developers that just need to work on a small part of the codebase. Linux workstations are a big expense when developers have powerful laptops capable of doing the job.

`spaces` is a lightweight solution to this problem suitable to simple workflows. It is powered by `starlark` and `rust`. You specify your project's direct dependencies in a `spaces` checkout script. You write a `spaces` run script with the build and deploy steps.

- tools are specified with a per-platform hash
- sources can be a git repo/revision or archive/hash

This allows you to create a precise workspace that has minimal system dependencies. `spaces` downloads artifacts to a common store (`~/.spaces/store`) and creates hardlinks to each workspace. `spaces` checks out dependencies transitely for repos that include `spaces` scripts.

All projects use the same commands:

```sh
spaces checkout --script=<your checkout script>.checkout.star --name=<your workspace folder>
cd <your workspace folder>
spaces run

# you can work the command line in the spaces run environment using
source env
```

Here is an example from the spaces [workflows repo](https://github.com/work-spaces/workflows/):

```python

# load the rust script from the sysroot repository
load("sysroot-packages/star/rust.star", "add_rust")

# Checkout the spaces repo
checkout.add_repo(
    rule = { "name": "spaces" },
    repo = {
        "url": "https://github.com/work-spaces/spaces",
        "rev": "main",
        "checkout": "Revision",
    },
)

# any files matching *.spaces.star within 
# checked-out repos are processed in order

# Grab the rust toolchain
add_rust(
    rule_name = "rust_toolchain",
    toolchain_version = "1.80",
)

# Checkout scripts can't have run rules
# So we generate a run script for the workspace
run_rules = """
run.add_exec(
    rule = { "name": "build" },
    exec = {
        "command": "cargo",
        "working_directory": "spaces",
        "args": [
            "build",
            "--profile=release",
        ],
    },
)
"""

checkout.add_asset(
    rule = { "name": "build_spaces_star" },
    asset = {
        "destination": "build.spaces.star",
        "content" = run_rules
    }
)
```

```sh
spaces checkout --script=preload.checkout.star --script=spaces-develop.checkout.star --name=spaces-build-test
cd spaces-build-test
spaces run
```

## Installing Spaces

`spaces` is a statically linked binary. Download it in the releases section.

Or run this:

```sh
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/work-spaces/install-spaces/refs/heads/main/install.sh)"
```

It requires `curl`, `unzip` and `sed`.

Or install from source using `cargo`:

```sh
git clone https://github.com/work-spaces/spaces
cd spaces
cargo install --path=crates/spaces --root=$HOME/.local --profile=release
```

Use `spaces` in github actions with https://github.com/work-spaces/install-spaces.

## Using Spaces

`spaces` executes `starlark` scripts in two phases:

- Checkout Phase: collects everything from source code to tools into a local folder
    - `git` repos are checked out in the workspace (either direct clones or using worktrees)
    - Archives (including platform binaries) are downloaded to `$HOME/.spaces/store`. Contents are hardlinked to the workspace `sysroot`.
- Run Phase: execute user-defined rules to build, test, run or deploy your project

### More about Checkout

You pass a `<file>.checkout.star` file and a name to `spaces`:

```sh
spaces checkout --script=my-project.checkout.star --name=build-my-project
```

During checkout, `spaces` populates a with everything you specify for your project. The workspace will look something like:

- `sysroot`: hardlinks to what you normally put in `/usr/local` with precise versions for your project
- `env`: Use `source env` to configure the command line for development
- `@logs`: log files for task output
- `env.spaces.star`: starlark file generated by `spaces` containing env info.
- source folders: source code repositories that you want to modify
- assets: files that dependencies will copy to the top-level workspace
  - For example, dependecies can populate `.vscode/settings.json` or create local git hooks.

`checkout.add_repo()` adds a repository to the workspace. `spaces` transitively evaluates scripts matching `*spaces.star` found in checked-out out repos.

### Run phase

Run rules execute tasks based on the dependency graph. Run rules have:

- A Rule:
    - `name`: the way to refer to this task when adding dependencies to other tasks
    - `deps`: explicit dependencies that must run before this task
    - `type`: `Setup`, `Run` (default), or `Optional`. Non-setup rules depend on `Setup` rules. `Optional` rules only run if activated.
- An Action
    - For example `run.add_exec()`, add a process (`command` and `args`) to the build graph.

You can execute your run rules in the workspace using:

```sh
spaces run
```

To enter the `spaces` execution environment used by `spaces run`, use:

```sh
source env
```

You will have `sysroot/bin` on your path and limited paths on the host system (as specified by the checkout rules).

## Writing Spaces Starlark Script

`starlark` is a sub-set of python. 

`starlark` has symbols available from three sources:

- The [standard starlark specification](https://github.com/bazelbuild/starlark/blob/master/spec.md)
- `spaces` built-ins (see the [API](API.md))
- Other scripts imported using `load()` statements.

The [standard starlark specification](https://github.com/bazelbuild/starlark/blob/master/spec.md) provides native types and associated functions.

`spaces` built-ins are `starlark` functions bound to `rust` implementations. This includes `checkout` and `run` rules which build a dependency graph and immediate-mode functions like reading `json` or `toml` files. See the [API](API.md) documentation.




