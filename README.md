# spaces

`spaces` is a poly-repo workflow tool. It efficiently manages sharing repositories and host tools across local workflows.

## Cargo Example

This example creates a workspace that allows you to develop `spaces` right next to `printer-rs`. `printer-rs` provides the `spaces` printer and is commonly developed alongside `spaces`.

Consider the file `config/develop_spaces.toml`:

```toml
[settings]
branch = "user/{USER}/{SPACE}-{UNIQUE}"

[vscode.extensions]
recommendations = ["rust-lang.rust-analyzer"]

[vscode.settings]
"editor.formatOnSave" = true

[repositories]
spaces = { git = "https://github.com/tyler-gilbert/spaces", branch = "main" }
printer = { git = "https://github.com/tyler-gilbert/printer-rs", branch = "main" }

[cargo.patches]
spaces = ["printer"]

[cargo.build]
rustc-wrapper = "sccache"
```

I can run:

```sh
spaces create --name=spaces-dev --config=config/develop_spaces.toml
```

This will create:

- spaces-dev
  - .vscode
    - extensions.json
    - settings.json
    - tasks.json (coming soon)
  - .config/cargo.toml
  - spaces
  - printer-rs



