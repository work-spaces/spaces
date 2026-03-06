# API Breaking Changes Required for Migrated to v0.16.x

Set `SPACES_ENV_WARN_DEPRECATED=0.16` to emit warnings in `>0.15.25` for violations.

- Default visibility will be private instead of public #484
- Remove support for worktrees #345
- Remove checkout.update_env #143
- Asset source destination paths need to use `//` otherwise the path will be relative #525
- Deps specified in the same file must be prefixed with `:` #475
- Functionality of `inputs` will be replaced with `deps` on files (with support for globs)
- Remove `outputs`. Use `targets` instead #585
