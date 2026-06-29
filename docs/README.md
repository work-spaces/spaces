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

## Demo GIF

Created using:
- https://github.com/asciinema/asciinema
- https://github.com/asciinema/agg


Tools

```sh
cargo install --locked --git https://github.com/asciinema/asciinema
cargo install --locked --git https://github.com/asciinema/agg
```

Make the font large (zoom in using iterm2)

Commands (need to by hand typed):

```sh
asciinema rec spaces-demo.cast
spaces about
spaces checkout-repo \
  --url=https://github.com/work-spaces/spaces \
  --rev=main \
  --new-branch=spaces \
  --name=issue-x-fix-something
cd issue-x-fix-something
spaces run //spaces:check
<ctrld+d>
```

```sh
agg --theme=asciinema --font-family="0xProto Nerd Font Mono" --speed=2 spaces-demo.cast spaces-demo.gif
```
