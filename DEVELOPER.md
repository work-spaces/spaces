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
export VERSION=0.14.0
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
  - This seems to be working. Could pull default/blobless clones if no changes have been made
- How to build rules that concatenate compile commands? she-bang script in sdk? 
- Format multi-line help info
- Use ellises in progress bar for names that exceed a length
- Add a `.star` she-bang script to watch a file for some content
  - have a sampling period and a timeout
- Capsules should install to sysroot, not the build folder
