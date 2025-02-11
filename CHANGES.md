# Spaces Change log

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