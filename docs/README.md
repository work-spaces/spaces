# For Developers

Publish a release by pushing a tag

```sh
export VERSION=x.y.z
git tag -a v$VERSION -m "Update version"
git push origin tag v$VERSION
```

# Custom Download Server

The included sample files below enable creating a custom server for distubuting spaces.

- `version.spaces.json`: manifest to be uploaded to a server
- `version.spaces.toml`: config file for users to point to the custom server

```sh
spaces version set-config <path to version.spaces.toml>
```
