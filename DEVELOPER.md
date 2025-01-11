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
export VERSION=0.11.17
git tag -a v$VERSION -m "Update version"
git push origin tag v$VERSION
```

# Todo

- Allow some rules to pass stdout to terminal
 - this could be added to the rule definition - log stdout - add to info level logging
- Better cleanup of whitespace in printer
- reset elapsed time just before job starts
