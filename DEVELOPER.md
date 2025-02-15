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
export VERSION=0.12.6
git tag -a v$VERSION -m "Update version"
git push origin tag v$VERSION
```

# Todo

- Add timestamp to log output
- checkout/sync should skip repos that are checkout and and already have changes
  - need to deal with read-only items - skip existing items
- Add ways to manage the store. Keep track of when archives are used. Delete old stuff.
  - Show which repos have worktrees checked out -- don't delete those
- soft-links in tools should be link to the sysroot file not the original file
- Make `spaces sync` work as expected to update workflows
- `spaces run <target>` should strip the relative path to make it easier to run rules
- Should all paths require `//...` to be at the workspace level and everything is relative to the rule directory?
  - should rules be allowed to be relative within a repo?
- Inspect needs to be better and more intuitive
- Inspect checkout workflows to get all URL dependencies
- How to skip reproducible workflows (capsules)
- How to build rules that concatenate compile commands? she-bang script in sdk?
- Checkout rules (Post Checkout) that requires a dependency at semver but doesn't check it out
  - This will be used for `@star/sdk` etc.
  - Can also be used to have parent project specify dependencies
