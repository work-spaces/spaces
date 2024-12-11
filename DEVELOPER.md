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
spaces checkout --script=workflows/spaces-develop.star --name=spaces-updates
cd spaces
source env
cargo build
```

Publish a release by pushing a tag

```sh
export VERSION=0.11.6
git tag -a v$VERSION -m "Update version"
git push origin tag v$VERSION
```

# Todo

- Create a `--create-lock-file` command line argument for checkout. This will record hashes for all git rules that use a branch rather than a tag
- Create a `--lock-file` command line argument for checkout. This will override any git rules that use a branch with the commit in the lock file.
- When running `spaces run` with a capsule, queue up a single job?