# For Developers

Clone and develop with just the spaces repo:

```sh
git clone https://github.com/work-spaces/spaces
cd spaces
cargo build
# install
cargo install --profile=release
```

Use `spaces` to develop with `printer` and `easy-archiver` in the same workspace.

```sh
git clone https://github.com/work-spaces/workflows
spaces checkout --script=workflows/preload --script=workflows/spaces-develop --name=spaces-updates
cd spaces
source env
cargo build
```

Publish a release by pushing a tag

```sh
export VERSION=0.11.8
git tag -a v$VERSION -m "Update version"
git push origin tag v$VERSION
```

# Todo

- When running `spaces run` with a capsule, queue up a single job?
- Upload capsules using oras to ghcr.io
  - oras can be another checkout rule like archive and git
  - uploading to oras will happen through starlark exec run rules
- add a run rule to send a signal to another running process (by rule)
- Instead of running spaces recursively for capsules, can it be run in the same process?
  - starlark scripts need some way to track state and know which workspace they are running in
    - only evaulate one script at a time
  - or instead of using stdout, pipe all the stdout to a shared printer?
  - or have a printer server running to work with IO
  - Put all state in the `Workspace` and then pass that around use state_lock inside an Arc
- Allow comma separated scripts from the command line --script=lock,preload,node-sdk
- Check SPACES_WORKFLOWS ENV for workflows folders: search folders in order to scripts
- Allow some rules to pass stdout to terminal
- option to remove bold() from printer
- Fix worktrees multiple access issue.