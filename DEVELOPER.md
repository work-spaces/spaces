# For Developers

Use `spaces` to develop with `printer` and `easy-archiver` in the same workspace.

> Requires spaces v0.14.4 or greater

```sh
git clone https://github.com/work-spaces/workflows
spaces checkout --workflow=workflows:spaces-dev --name=issue-X-spaces-updates
cd issue-X-spaces-updates
source ./env
cargo build
```

# Internal Use Only

Publish a release by pushing a tag

```sh
export VERSION=x.y.z
git tag -a v$VERSION -m "Update version"
git push origin tag v$VERSION
```
