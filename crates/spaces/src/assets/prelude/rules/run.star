"""
User friendly wrapper functions for the spaces run built-in functions.
"""

load("visibility.star", "visibility_private")
load(
    "ws.star",
    "workspace_get_build_archive_info",
    "workspace_get_env_var",
    "workspace_is_env_var_set",
)

RUN_INPUTS_ONCE = []
RUN_INPUTS_ALWAYS = None
RUN_TYPE_ALL = "Run"
RUN_TYPE_DEFAULT = "Optional"
RUN_TYPE_TEST = "Test"
RUN_TYPE_SETUP = "Setup"
RUN_TYPE_PRECOMMIT = "PreCommit"
RUN_TYPE_CLEAN = "Clean"
RUN_TYPES = [
    RUN_TYPE_ALL,
    RUN_TYPE_TEST,
    RUN_TYPE_SETUP,
    RUN_TYPE_PRECOMMIT,
    RUN_TYPE_CLEAN,
]

# Print the output of the run rule while running spaces
RUN_LOG_LEVEL_APP = "App"
RUN_LOG_LEVEL_PASSTHROUGH = "Passthrough"

RUN_EXPECT_SUCCESS = "Success"
RUN_EXPECT_FAILURE = "Failure"
RUN_EXPECT_ANY = "Any"

# Kill Signals
RUN_SIGNAL_HUP = "Hup"
RUN_SIGNAL_INT = "Int"
RUN_SIGNAL_QUIT = "Quit"
RUN_SIGNAL_ABORT = "Abort"
RUN_SIGNAL_KILL = "Kill"
RUN_SIGNAL_ALARM = "Alarm"
RUN_SIGNAL_TERMINATE = "Terminate"
RUN_SIGNAL_USER1 = "User1"
RUN_SIGNAL_USER2 = "User2"

def run_load_file_contents(path: str) -> str:
    """
    Load the contents of a file and return it as a string.

    The file will be automatically added to the rule dependencies.

    Args:
        path (str): The path to the file to load.

    Returns:
        str: The contents of the file.
    """
    return "$RUN_LOAD_FILE_CONTENTS{" + path + "}"

def run_load_exit_code(rule: str) -> str:
    """
    Load the exit code of a previous run rule and return it as a string.

    The file will be automatically added to the rule dependencies.

    Args:
        rule (str): The rule to load the exit code from.

    Returns:
        str: The contents of the file.
    """
    return "$RUN_LOAD_EXIT_VALUE{" + rule + "}"

def run_load_env(key: str) -> str:
    """
    Load the value of an environment variable when the rule is executed.

    Args:
        key (str): The name of the environment variable to load.

    Returns:
        str: A token that will be replaced with the value of the environment variable at execution time.
    """
    return "$RUN_LOAD_ENV{" + key + "}"

# Provide thin wrapper for constants so that they can have docstrings
def run_inputs_once():
    """
    Assign `inputs` to `[]` to run the command once.

    Returns:
        list: []
    """
    return RUN_INPUTS_ONCE

def run_inputs_always():
    """
    Assign `inputs` to `None` to run the command every time.

    Returns:
        None
    """
    return RUN_INPUTS_ALWAYS

def run_type_all():
    """
    Assign `type` to `Run` to run the command with `spaces run`.

    The rules marked as `Run` are part of the `//:all` target.

    ```sh
    spaces run //:all
    ```

    Returns:
        str: "Run"
    """
    return RUN_TYPE_ALL

def run_type_test():
    """
    The rules added as `Test` are part of the `//:test` target.

    ```sh
    spaces run //:test
    ```

    Returns:
        str: "Test"
    """
    return RUN_TYPE_TEST

def run_type_setup():
    """
    The rules added as `Setup` are part of the `//:setup` target.

    ```sh
    spaces run //:setup
    ```

    Returns:
        str: "Setup"
    """
    return RUN_TYPE_SETUP

def run_type_precommit():
    """
    The rules added as `PreCommit` are part of the `//:pre-commit` target.

    ```sh
    spaces run //:pre-commit
    ```

    Returns:
        str: "PreCommit"
    """
    return RUN_TYPE_PRECOMMIT

def run_log_level_app():
    """
    Print the output of the run rule while running spaces

    Returns:
        str: "App"
    """
    return RUN_LOG_LEVEL_APP

def run_log_level_passthrough():
    """
    Print the output of the run rule while running spaces with no additional markings

    Returns:
        str: "Passthrough"
    """
    return RUN_LOG_LEVEL_PASSTHROUGH

def run_expect_success():
    """
    Expect the command to succeed

    Returns:
        str: "Success"
    """
    return RUN_EXPECT_SUCCESS

def run_expect_failure():
    """
    Expect the command to fail.

    If the command fails and is expected to fail, spaces exits successfully.

    Returns:
        str: "Failure"
    """
    return RUN_EXPECT_FAILURE

def run_expect_any():
    """
    Expect the command to succeed or fail.

    `spaces` exits successfully if the command succeeds or fails.

    Returns:
        str: "Any"
    """
    return RUN_EXPECT_ANY

def run_signal_hup():
    """
    Gets the Hangup signal
    """
    return RUN_SIGNAL_HUP

def run_signal_int():
    """
    Gets the Interrupt signal
    """
    return RUN_SIGNAL_INT

def run_signal_quit():
    """
    Gets the Quit signal
    """
    return RUN_SIGNAL_QUIT

def run_signal_abort():
    """
    Gets the Abort signal
    """
    return RUN_SIGNAL_ABORT

def run_signal_kill():
    """
    Gets the Kill signal
    """
    return RUN_SIGNAL_KILL

def run_signal_alarm():
    """
    Gets the Alarm signal
    """
    return RUN_SIGNAL_ALARM

def run_signal_terminate():
    """
    Gets the Terminate signal
    """
    return RUN_SIGNAL_TERMINATE

def run_signal_user1():
    """
    Gets the User1 signal
    """
    return RUN_SIGNAL_USER1

def run_signal_user2():
    """
    Gets the User2 signal
    """
    return RUN_SIGNAL_USER2

def _run_get_effective_env(env: dict, workspace_vars: list[str]) -> dict:
    effective_env = {}

    for workspace_var in workspace_vars:
        if workspace_is_env_var_set(workspace_var):
            effective_env[workspace_var] = workspace_get_env_var(workspace_var)

    effective_env.update(env)

    return effective_env

def run_add_exec_setup(
        name: str,
        command: str,
        help: str | None = None,
        args: list[str] = [],
        env: dict = {},
        workspace_vars: list[str] = [],
        deps: list[str] | list[dict] = [],
        apply_trailing_args_to: str | None = None,
        working_directory: str | None = None,
        platforms: list[str] | None = None,
        log_level: str | None = None,
        redirect_stdout: str | None = None,
        timeout: float | None = None,
        visibility: str | dict[str, list[str]] | None = None,
        expect: str = RUN_EXPECT_SUCCESS):
    """
    Adds a command as a setup rule. It will run only once and all run rules will depend on it.

    All setup rules can be executed with:

    ```sh
    spaces run //:setup
    ```

    Args:
        name: The name of the rule.
        command: The command to execute.
        help: The help message for the rule.
        args: The arguments to pass to the command
        deps: The rule dependencies
        env: key value pairs of environment variables
        workspace_vars: Workspace environment variable names to copy into env.
        apply_trailing_args_to: The name of the rule that will receive command line trailing args when this rule is run directly.
        working_directory: The directory to run the command (default is workspace root).
        platforms: Platforms to run on (default is all).
        log_level: The log level to use None|App
        redirect_stdout: The file to redirect stdout to (prefer to parse the log file).
        timeout: Number of seconds to run before sending a kill signal.
        expect: The expected result of the command Success|Failure|Any. (default is Success)
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.
    """

    EFFECTIVE_ENV = _run_get_effective_env(env, workspace_vars)

    run.add_exec(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "help": help,
            "type": RUN_TYPE_SETUP,
            "inputs": RUN_INPUTS_ONCE,
            "visibility": visibility,
            "apply_trailing_args_to": apply_trailing_args_to,
        },
        exec = {
            "command": command,
            "args": args,
            "working_directory": working_directory,
            "env": EFFECTIVE_ENV,
            "expect": expect,
            "log_level": log_level,
            "redirect_stdout": redirect_stdout,
            "timeout": timeout,
        },
    )

def run_add_exec_test(
        name: str,
        command: str,
        help: str | None = None,
        args: list[str] = [],
        env: dict = {},
        workspace_vars: list[str] = [],
        deps: list[str] | list[dict] = [],
        inputs: list[str] | None = RUN_INPUTS_ALWAYS,
        apply_trailing_args_to: str | None = None,
        working_directory: str | None = None,
        platforms: list[str] | None = None,
        log_level: str | None = None,
        redirect_stdout: str | None = None,
        timeout: float | None = None,
        visibility: str | dict[str, list[str]] | None = None,
        expect: str = RUN_EXPECT_SUCCESS):
    """
    Adds a command as a test rule.

    All test rules can be executed with:

    ```sh
    spaces run //:test
    ```

    Args:
        name: The name of the rule.
        command: The command to execute.
        help: The help message for the rule.
        args: The arguments to pass to the command
        deps: The rule dependencies
        inputs: List of globs to specify the inputs. If the inputs are unchanged, the command will not run.
        env: key value pairs of environment variables
        workspace_vars: Workspace environment variable names to copy into env.
        apply_trailing_args_to: The name of the rule that will receive command line trailing args when this rule is run directly.
        working_directory: The directory to run the command (default is workspace root).
        platforms: Platforms to run on (default is all).
        log_level: The log level to use None|App
        redirect_stdout: The file to redirect stdout to (prefer to parse the log file).
        timeout: Number of seconds to run before sending a kill signal.
        expect: The expected result of the command Success|Failure|Any. (default is Success)
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.
    """

    run.add_exec(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "help": help,
            "type": "Test",
            "inputs": inputs,
            "visibility": visibility,
            "apply_trailing_args_to": apply_trailing_args_to,
        },
        exec = {
            "command": command,
            "args": args,
            "working_directory": working_directory,
            "env": _run_get_effective_env(env, workspace_vars),
            "expect": expect,
            "log_level": log_level,
            "redirect_stdout": redirect_stdout,
            "timeout": timeout,
        },
    )

def run_add_exec_precommit(
        name: str,
        command: str,
        help: str | None = None,
        args: list[str] = [],
        env: dict = {},
        workspace_vars: list[str] = [],
        deps: list[str] | list[dict] = [],
        inputs: list[str] | None = RUN_INPUTS_ALWAYS,
        apply_trailing_args_to: str | None = None,
        working_directory: str | None = None,
        platforms: list[str] | None = None,
        log_level: str | None = None,
        redirect_stdout: str | None = None,
        timeout: float | None = None,
        visibility: str | dict[str, list[str]] | None = None,
        expect: str = RUN_EXPECT_SUCCESS):
    """
    Adds a command as a pre-commit rule.

    All pre-commit rules can be executed with:

    ```sh
    spaces run //:pre-commit
    ```

    Args:
        name: The name of the rule.
        command: The command to execute.
        help: The help message for the rule.
        args: The arguments to pass to the command
        deps: The rule dependencies
        inputs: List of globs to specify the inputs. If the inputs are unchanged, the command will not run.
        env: key value pairs of environment variables
        workspace_vars: Workspace environment variable names to copy into env.
        apply_trailing_args_to: The name of the rule that will receive command line trailing args when this rule is run directly.
        working_directory: The directory to run the command (default is workspace root).
        platforms: Platforms to run on (default is all).
        log_level: The log level to use None|App
        redirect_stdout: The file to redirect stdout to (prefer to parse the log file).
        timeout: Number of seconds to run before sending a kill signal.
        expect: The expected result of the command Success|Failure|Any. (default is Success)
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.
    """

    run.add_exec(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "help": help,
            "type": RUN_TYPE_PRECOMMIT,
            "inputs": inputs,
            "visibility": visibility,
            "apply_trailing_args_to": apply_trailing_args_to,
        },
        exec = {
            "command": command,
            "args": args,
            "working_directory": working_directory,
            "env": _run_get_effective_env(env, workspace_vars),
            "expect": expect,
            "log_level": log_level,
            "redirect_stdout": redirect_stdout,
            "timeout": timeout,
        },
    )

def run_add_exec_clean(
        name: str,
        command: str,
        help: str | None = None,
        args: list[str] = [],
        env: dict = {},
        workspace_vars: list[str] = [],
        deps: list[str] | list[dict] = [],
        inputs: list[str] | None = RUN_INPUTS_ALWAYS,
        apply_trailing_args_to: str | None = None,
        working_directory: str | None = None,
        platforms: list[str] | None = None,
        log_level: str | None = None,
        redirect_stdout: str | None = None,
        timeout: float | None = None,
        visibility: str | dict[str, list[str]] | None = None,
        expect: str = RUN_EXPECT_SUCCESS):
    """
    Adds a command as a clean rule.

    All test rules can be executed with:

    ```sh
    spaces run //:clean
    ```

    Args:
        name: The name of the rule.
        command: The command to execute.
        help: The help message for the rule.
        args: The arguments to pass to the command
        deps: The rule dependencies
        inputs: List of globs to specify the inputs. If the inputs are unchanged, the command will not run.
        env: key value pairs of environment variables
        workspace_vars: Workspace environment variable names to copy into env.
        apply_trailing_args_to: The name of the rule that will receive command line trailing args when this rule is run directly.
        working_directory: The directory to run the command (default is workspace root).
        platforms: Platforms to run on (default is all).
        log_level: The log level to use None|App
        redirect_stdout: The file to redirect stdout to (prefer to parse the log file).
        timeout: Number of seconds to run before sending a kill signal.
        expect: The expected result of the command Success|Failure|Any. (default is Success)
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.
    """

    run.add_exec(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "help": help,
            "type": RUN_TYPE_CLEAN,
            "inputs": inputs,
            "visibility": visibility,
            "apply_trailing_args_to": apply_trailing_args_to,
        },
        exec = {
            "command": command,
            "args": args,
            "working_directory": working_directory,
            "env": _run_get_effective_env(env, workspace_vars),
            "expect": expect,
            "log_level": log_level,
            "redirect_stdout": redirect_stdout,
            "timeout": timeout,
        },
    )

def run_add_exec(
        name: str,
        command: str,
        help: str | None = None,
        args: list[str] = [],
        env: dict = {},
        workspace_vars: list[str] = [],
        deps: list[str] | list[dict] = [],
        inputs: list[str] | None = RUN_INPUTS_ALWAYS,
        apply_trailing_args_to: str | None = None,
        target_files: list[str] | None = None,
        target_dirs: list[str] | None = None,
        type: str | None = None,
        working_directory: str | None = None,
        platforms: list[str] | None = None,
        log_level: str | None = None,
        redirect_stdout: str | None = None,
        timeout: float | None = None,
        visibility: str | dict[str, list[str]] | None = None,
        expect: str = RUN_EXPECT_SUCCESS):
    """
    Adds a command to the run dependency graph

    Args:
        name: The name of the rule.
        command: The command to execute.
        help: The help message for the rule.
        args: The arguments to pass to the command.
        type: The exec type (Run|Setup|Optional (default)|PreCommit|Clean|Test)
        deps: The rule dependencies that must be run before this command
        inputs: List of globs to specify the inputs. If the inputs are unchanged, the command will not run.
        env: key value pairs of environment variables
        workspace_vars: Workspace environment variable names to copy into env.
        apply_trailing_args_to: The name of the rule that will receive command line trailing args when this rule is run directly.
        working_directory: The directory to run the command (default is workspace root).
        platforms: Platforms to run on (default is all).
        log_level: The log level to use None|App|Passthrough
        target_files: A list of files created by this rule
        target_dirs: A list of directories populated by this rule
        expect: The expected result of the command Success|Failure|Any. (default is Success)
        redirect_stdout: The file to redirect stdout to (prefer to parse the log file).
        timeout: Number of seconds to run before sending a kill signal.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.
    """

    EFFECTIVE_TYPE = type if type != None else RUN_TYPE_DEFAULT

    TARGET_FILES = [{"File": file} for file in target_files] if target_files else []
    TARGET_DIRS = [{"Directory": dir} for dir in target_dirs] if target_dirs else []

    run.add_exec(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "help": help,
            "type": EFFECTIVE_TYPE,
            "inputs": inputs,
            "visibility": visibility,
            "apply_trailing_args_to": apply_trailing_args_to,
            "targets": TARGET_FILES + TARGET_DIRS,
        },
        exec = {
            "command": command,
            "args": args,
            "working_directory": working_directory,
            "env": _run_get_effective_env(env, workspace_vars),
            "expect": expect,
            "log_level": log_level,
            "redirect_stdout": redirect_stdout,
            "timeout": timeout,
        },
    )

def run_add_kill_exec(
        name: str,
        target: str,
        signal: str = RUN_SIGNAL_KILL,
        help: str | None = None,
        expect: str = RUN_EXPECT_SUCCESS,
        deps: list[str] | list[dict] = [],
        type: str | None = None,
        platforms: list[str] | None = None,
        visibility: str | dict[str, list[str]] | None = None):
    """
    Adds a target that will send a signal to another target.

    Args:
        name: The name of the rule.
        target: The name of the rule to kill.
        signal: The signal to send to the target.
        help: The help message for the rule.
        expect: The expected result of the kill. (default is Success)
        deps: Run rule dependencies.
        type: See [run_add_exec()](#run_add_exec)
        platforms: Platforms to run on (default is all).
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.
    """

    EFFECTIVE_TYPE = type if type != None else RUN_TYPE_DEFAULT

    run.add_kill_exec(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "help": help,
            "type": EFFECTIVE_TYPE,
            "inputs": None,
            "visibility": visibility,
        },
        kill = {
            "target": target,
            "signal": signal,
            "expect": expect,
        },
    )

def run_add(
        name: str,
        deps: list[str],
        apply_trailing_args_to: str | None = None,
        help: str | None = None,
        type: str | None = None,
        platforms: list[str] | None = None,
        visibility: str | dict[str, list[str]] | None = None):
    """
    Adds a rule to the workspace with no associated command.

    This rule can be used to consolidate dependencies into a single rule.

    Args:
        name: The name of the rule.
        deps: List of dependencies for the target.
        apply_trailing_args_to: The name of the rule that will get command line trailing args when this rule is run directly.
        platforms: List of platforms to build the target for (default is all).
        type: See [run_add_exec()](#run_add_exec)
        help: The help message for the rule.
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.
    """

    run.add(
        rule = {
            "name": name,
            "deps": deps,
            "platforms": platforms,
            "type": type,
            "help": help,
            "visibility": visibility,
            "apply_trailing_args_to": apply_trailing_args_to,
        },
    )

def run_add_to_all(
        name: str,
        deps: list[str],
        visibility: str | dict[str, list[str]] | None = None):
    """
    Creates a target rule called name with deps and part of `:all`.

    Targets will run with `spaces run` or `spaces run //:all`.

    Args:
        name: The name of the rule.
        deps: List of dependencies to run with `spaces run`
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.
    """

    run_add(name, deps, type = RUN_TYPE_ALL, visibility = visibility)

def run_add_archive(
        name: str,
        archive_name: str,
        deps: list[str],
        version: str,
        source_directory: str,
        suffix: str = "tar.gz",
        includes: list[str] | None = None,
        excludes: list[str] | None = None,
        platform: str | None = None,
        visibility: str | dict[str, list[str]] | None = None):
    """
    Adds an archive target to the workspace.

    This rule can be used to consolidate dependencies into a single target.

    Args:
        name: The name of the rule.
        archive_name: The base name of the archive that will be created
        suffix: The archive suffix (tar.gz or zip for examples)
        deps: List of dependencies to run with `spaces run`
        version: The version of the archive.
        source_directory: The directory containing the source files to archive.
        includes: List of globs to include in the archive.
        excludes: List of globs to exclude from the archive.
        platform: The platform to build the target for (default is all).
        visibility: Rule visibility: `Public|Private|Rules[]`. See visibility.star for more info.

    Returns:
        A tuple containing (<path to the archive>, <sha256 checksum of the archive>).
    """

    effective_platform = info.get_platform_name() if platform == None else platform

    archive_info = {
        "input": source_directory,
        "name": archive_name,
        "version": version,
        "driver": suffix,
        "platform": effective_platform,
        "includes": includes,
        "excludes": excludes,
    }

    run.add_archive(
        rule = {
            "name": name,
            "deps": deps,
            "visibility": visibility,
        },
        archive = archive_info,
    )

    archive_output_info = workspace_get_build_archive_info(name, archive = archive_info)

    return (archive_output_info["archive_path"], archive_output_info["sha256_path"])
