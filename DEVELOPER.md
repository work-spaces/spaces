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
export VERSION=0.12.5
git tag -a v$VERSION -m "Update version"
git push origin tag v$VERSION
```

# Todo

- Add timestamp to log output
- checkout/sync should skip repos that are checkout and and already have changes
- Add ways to manage the store. Keep track of when archives are used. Delete old stuff.
  - Show which repos have worktrees checked out -- don't delete those
- soft-links in tools should be link to the sysroot file not the original file
- Make `spaces sync` work as expected to update workflows
- `spaces run <target>` should strip the relative path to make it easier to run rules
- Should all paths require `//...` to be at the workspace level and everything is relative to the rule directory?
