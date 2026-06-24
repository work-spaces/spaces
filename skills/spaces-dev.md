---
name: create-spaces-pr
description: Use this skill when making general code changes in this workspace's Spaces codebase and validates changes with spaces rules as broadly as practical.
---

# Spaces Dev

Use this skill for day-to-day implementation, refactoring, and debugging work in this repository when the change affects Spaces code or rules.

## Required command environment

When running `spaces`, there is no need to set `PATH`.

To test spaces code changes, first check the changes using:

```sh
spaces run //spaces:check
```

If running the binary is needed for testing, it can be built using (don't run this just for the sake of it):

```sh
spaces run //spaces:build
```

Then use this binary to run any testing on the build:

```sh
build/target/debug/spaces ...
```

## Rule-first validation workflow

Prefer `spaces run <rule>` over invoking build/test tools directly.

1. Start by discovering available rules:

```sh
spaces query rules
```

All rules support passing additional arguments to the rule invocation by adding the args after a trailing `--`.

2. Run the most targeted relevant rules first.
3. Then run broad validation using Spaces rules as much as possible.

## Code Validation and Formatting

```sh
spaces run //spaces:clippy
spaces run //spaces:format
```

## Change execution guidance

- Keep changes minimal and consistent with existing style.
- Prefer existing Spaces patterns in `spaces/spaces.star` and nearby crates/scripts.
- If a rule fails, report the failing rule label and the key error lines.
- In final summaries, list exactly which Spaces rules were executed.

## Running Tools Directly

When running tools directly, ensure `sysroot/bin` is first in `PATH`. Use this pattern from the workspace root:

```sh
PATH=<workspace_root>/sysroot/bin:/usr/bin:/bin:/usr/sbin:/sbin <command>
```

sysroot/bin contains sccache, rg, and other helpful tools.
