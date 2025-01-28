# For Developers

Use `spaces` to develop with `printer` and `easy-archiver` in the same workspace.

> Requires spaces v0.11.24 or greater

```sh
git clone https://github.com/work-spaces/workflows
spaces checkout --workflow=workflows:spaces-develop --name=spaces-updates
cd spaces
source env
cargo build
```

# Internal Use Only

Publish a release by pushing a tag

```sh
export VERSION=0.11.29
git tag -a v$VERSION -m "Update version"
git push origin tag v$VERSION
```

# Todo

- Add timestamp to log output
- checkout/sync should skip repos that are checkout and and already have changes
- loading workspace is slow because of checking for workflow paths on every file instead of every directory.
