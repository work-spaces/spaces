---
name: create-spaces-pr
description: Use this skill when making general code changes in this workspace's Spaces codebase. It enforces using sysroot/bin in PATH and validates changes with spaces rules as broadly as practical.
---

# Spaces Dev

Use this skill for day-to-day implementation, refactoring, and debugging work in this repository when the change affects Spaces code or rules.

## Required command environment

When running commands, always ensure `sysroot/bin` is first in `PATH`.

Use this pattern from the workspace root:

```sh
PATH=<workspace_root>/sysroot/bin:/usr/bin:/bin:/usr/sbin:/sbin <command>
```

To test spaces code changes, first check the changes using:

```sh
spaces run //spaces:check
```

Then build the changes and use the debug binary to run: 

```sh
spaces run //spaces:build
```

Then use:

```sh
spaces/target/debug/spaces ...
```

## Rule-first validation workflow

Prefer `spaces run <rule>` over invoking build/test tools directly.

1. Start by discovering available rules:

```sh
spaces query rules
```

2. Run the most targeted relevant rules first.
3. Then run broad validation using Spaces rules as much as possible.

## Code Validation and Formatting

```sh
spaces run //spaces:clippy
spaces run //spaces:fmt
```

## Change execution guidance

- Keep changes minimal and consistent with existing style.
- Prefer existing Spaces patterns in `spaces/spaces.star` and nearby crates/scripts.
- If a rule fails, report the failing rule label and the key error lines.
- In final summaries, list exactly which Spaces rules were executed.
