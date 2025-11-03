# spaces

## What is `spaces`?

- Reproducible workspace builder that manages
  - dev tools: `cmake`, `clang`, `cargo`, `python` and anything else you can specify
  - archives: populate your workspace from any archive on the internet
  - repos: clone one or more repositories into your workspace
  - assets: populate your workspace with IDE settings such as `.vscode/settings.json` or `.zed/setting.json`
  - environment: specify variables or inherit from the system as needed
- Lightweight meta-build task-runner
  - Create rules that run in series or parallel using `starlark`
  - Execute anything you can call from the command line in precise folder locations and environment
  - Auto-skip rules that don't need to run again
- An awesome inner-loop shell
  - Start a shell with an environment that exactly matches the `spaces` task runner.

## Demo

- Checkout the `spaces` sources and dependencies including an isolated `rust` toolchain.
- Use the `spaces` task runner to build or use `spaces shell` to access dev tools directly in the workspace environment.

![spaces.run.shell](https://github.com/work-spaces/work-spaces.github.io/releases/download/assets-v0.1.0/spaces_demo.gif)

## Read the Docs

[Spaces Documentation](https://work-spaces.github.io/)

## Contribute

You can create a workspace dedicated to improving `spaces`.

```sh
spaces checkout-repo \
  --url=https://github.com/work-spaces/spaces \
  --rev=main \
  --new-branch=spaces \
  --name=issue-x-fix-something
cd issue-x-fix-something
spaces run //spaces:check
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

## Uninstall Spaces

- Delete the binary: `rm $HOME/.local/bin/spaces`.
- Delete the store: `rm -rf $HOME/.spaces`
