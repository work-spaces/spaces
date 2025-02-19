# Spaces Change log

## v0.14.1

- Use ... on with rule progress if rule name is too long
- Format the log header as yaml
- Add a `=======...` divider to the top of the log to denote header vs content
- Add `info.get_log_divider_string()` to return the string used to divide the log header and content
- Add `info.parse_log_file(path)` to parse the log into a starlark dict

## v0.14.0

- All load statements, `working_directory` and rule names use:
  - `//` to denote a workspace path
  - `some/path` to denote a relative path from the directory location
- Move workspace related built-ins from `info` to `workspace`
- Add built-in: `hash.compute_sha256_from_string()`
- Add `--has-help` option to inspect to only show rules with `help` provided
- Fix issues with `inspect` filtering `//`
- Generate lock files and env file with `CONSTANTS` convention

## v0.13.1

- Improve `spaces inspect`
  - limit to the current folder by default when not at root
  - `--filter=spaces` transforms to `--filter="*spaces*"` so it works more like grep
- Update task hashing to go over the topograph and incorporate digest of deps
  - Currently only used for informational purposes - not available to rules

## v0.13.0

- API breaking change: Remove `capsules` in rust. Starlark implements capsules in SDK v0.2.0
- Allow checkout rules to be optional (skipped). This supports starlark capsules
- Cargo clippy fixes

## v0.12.6

- Fix bug when checking out the same repo at different paths
- Add workspace members to workspace settings file
- Add info.abort() 
- Add info.get_path_to_workspace_member() and info.is_path_to_workspace_member_available()
- create `ws` sub-crate to simplify `workspace.rs`

## v0.12.5

- Fix bug where exclude glob of single file is treated as include
- Allow omitting relative path to current directory in workspace with `run` and `inspect`

## v0.12.4

- Fix bug where task completion is not signaled on some errors

## v0.12.3

- Release signal mutex before log bug fix.

## v0.12.2

- `redirect_stdout` always writes to `build` folder. Fix bug to create directory structure to `redirect_stdout` files
- Add `info.get_path_to_log_file()` to get the current log file for any rule.

## v0.12.1

- Fixes a regression with `inspect` and `checkout` caused by creating run `:all` target.

## v0.12.0

- Add phantom `:all` target to run all `Run` targets. In line with recent `@star/sdk` change to make "Optional" the default run type. `spaces run` will run the phantom `:all` target.

## v0.11.33

- Suggest similar target names
- Save workspace spaces modules in settings file. Rescan using `--rescan`.

## v0.11.32

- Ignore hidden directories when scanning the workspace
- If `gh` fails, try using HTTPS. Recommend `gh auth login` if both fail

## v0.11.31

- Change threshold to dump log to terminal to 10MB

## v0.11.30

- Raise an error if trying to checkout a script named `env.spaces.star`. This will conflict with a spaces generated file.

## v0.11.29

- Performance improvement while loading the workspace