#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/process.star",
    "process_capture",
    "process_is_running",
    "process_kill",
    "process_options",
    "process_pipeline",
    "process_run",
    "process_spawn",
    "process_stderr_capture",
    "process_stderr_file",
    "process_stderr_inherit",
    "process_stderr_merge",
    "process_stderr_null",
    "process_stdout_capture",
    "process_stdout_file",
    "process_stdout_inherit",
    "process_stdout_null",
    "process_wait",
)

# Process module test results
process_results = {
    "basic_execution": {},
    "output_capture": {},
    "stdin_handling": {},
    "environment_variables": {},
    "working_directory": {},
    "background_processes": {},
    "process_management": {},
    "pipelines": {},
    "options_builder": {},
    "stdout_helpers": {},
    "stderr_helpers": {},
}

# ============================================================================
# Basic Execution Tests
# ============================================================================

# Test: non-zero exit code — process_run without check=True returns exit code
nonzero_result = process_run(process_options(
    "sh",
    args = ["-c", "exit 2"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
))
process_results["basic_execution"]["nonzero_exit_code"] = nonzero_result["status"] == 2

# Test: check=True on a successful command (exit 0) must NOT raise.
# NOTE: Starlark has no try/except, so we cannot directly test that check=True
# raises on a failing command. We validate the positive case only: a command
# that exits 0 with check=True completes normally and returns the correct result.
check_success = process_run(process_options(
    "echo",
    args = ["check ok"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
    check = True,
))
process_results["basic_execution"]["check_true_success"] = (
    check_success["status"] == 0 and
    "check ok" in check_success["stdout"]
)

# ============================================================================
# Output Capture Tests
# ============================================================================

# Test process_capture — simple output capture (existing)
capture_result = process_capture(["echo", "captured output"])
process_results["output_capture"]["simple_capture"] = "captured output" in capture_result

# Test process_run with output capture (existing)
run_result = process_run({
    "command": "echo",
    "args": ["test output"],
    "stdout": "capture",
    "stderr": "capture",
})
process_results["output_capture"]["run_capture"] = (
    run_result["status"] == 0 and
    "test output" in run_result["stdout"]
)

# Test: stderr capture — command writes only to stderr; verify it is captured
stderr_result = process_run(process_options(
    "sh",
    args = ["-c", "echo error_message >&2"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
))
process_results["output_capture"]["stderr_captured"] = (
    "error_message" in stderr_result["stderr"] and
    stderr_result["status"] == 0
)

# ============================================================================
# Stdin Handling Tests
# ============================================================================

# Test: pass stdin to a command — cat echoes stdin verbatim to stdout
stdin_result = process_run(process_options(
    "cat",
    stdin = "hello from stdin\n",
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
))
process_results["stdin_handling"]["stdin_fed_to_process"] = (
    "hello from stdin" in stdin_result["stdout"] and
    stdin_result["status"] == 0
)

# ============================================================================
# Environment Variable Tests
# ============================================================================

# Test: custom env var is visible inside the subprocess
env_result = process_run(process_options(
    "sh",
    args = ["-c", "echo $MY_SPACES_TEST_VAR"],
    env = {"MY_SPACES_TEST_VAR": "spaces_env_value_42"},
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
))
process_results["environment_variables"]["env_var_passed"] = (
    "spaces_env_value_42" in env_result["stdout"] and
    env_result["status"] == 0
)

# ============================================================================
# Working Directory Tests
# ============================================================================

# Test: cwd override — run pwd inside /tmp and verify the output reflects it.
# NOTE: On macOS /tmp is a symlink to /private/tmp, so we check that "tmp"
# appears in the output rather than expecting an exact string match.
cwd_result = process_run(process_options(
    "sh",
    args = ["-c", "pwd"],
    cwd = "/tmp",
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
))
process_results["working_directory"]["cwd_respected"] = (
    cwd_result["status"] == 0 and
    "tmp" in cwd_result["stdout"]
)

# ============================================================================
# Background Process / process_wait Tests
# ============================================================================

# Spawn an echo process with output capture so we can verify process_wait's result
spawn_result = process_spawn(process_options(
    "echo",
    args = ["spawned process"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
))
process_results["background_processes"]["spawn_returns_handle"] = spawn_result > 0

# Test process_wait — should return a dict with status, stdout, stderr, duration_ms
wait_result = process_wait(spawn_result)
process_results["background_processes"]["wait_has_status"] = "status" in wait_result
process_results["background_processes"]["wait_has_stdout"] = "stdout" in wait_result
process_results["background_processes"]["wait_has_stderr"] = "stderr" in wait_result
process_results["background_processes"]["wait_has_duration_ms"] = "duration_ms" in wait_result
process_results["background_processes"]["wait_status_zero"] = wait_result["status"] == 0
process_results["background_processes"]["wait_stdout_content"] = (
    "spawned process" in wait_result["stdout"]
)

# ============================================================================
# Process Management Tests (is_running, kill)
# ============================================================================

# FIX for the previously broken is_running_check test:
# The old test called process_is_running() on a just-spawned echo, which may
# already have exited by the time we check (OS scheduling race). The reliable
# approach is to spawn a long-running process (sleep 30) and check is_running
# before and after killing it.

# Spawn a long-running process — it will definitely be running immediately after spawn
long_handle = process_spawn(process_options("sleep", args = ["30"]))

# is_running() must return True for a process that is sleeping for 30 seconds
process_results["process_management"]["is_running_true_for_long_process"] = (
    process_is_running(long_handle)
)

# Kill it with SIGKILL (immediate, unconditional termination)
kill_ok = process_kill(long_handle, "SIGKILL")
process_results["process_management"]["kill_returns_true"] = kill_ok == True

# Wait for the killed process — status must be non-zero (signal termination)
kill_wait_result = process_wait(long_handle)
process_results["process_management"]["kill_and_wait_nonzero_status"] = (
    kill_wait_result["status"] != 0
)

# ============================================================================
# Pipeline Tests
# ============================================================================

# Test process_pipeline — pipe echo output through grep (existing)
pipeline_result = process_pipeline([
    {"command": "echo", "args": ["line1\nline2\nline3"]},
    {"command": "grep", "args": ["line2"]},
])
process_results["pipelines"]["basic_pipeline"] = (
    "line2" in pipeline_result["stdout"] and
    pipeline_result["status"] == 0
)

# ============================================================================
# Stdout Helper Tests
# ============================================================================

# Test process_stdout_inherit helper
stdout_inherit = process_stdout_inherit()
process_results["stdout_helpers"]["stdout_inherit"] = stdout_inherit == "inherit"

# Test process_stdout_capture helper
stdout_capture = process_stdout_capture()
process_results["stdout_helpers"]["stdout_capture"] = stdout_capture == "capture"

# Test process_stdout_null helper
stdout_null = process_stdout_null()
process_results["stdout_helpers"]["stdout_null"] = stdout_null == "null"

# Test process_stdout_file helper (value check)
stdout_file_val = process_stdout_file("/tmp/test_stdout.txt")
process_results["stdout_helpers"]["stdout_file_value"] = (
    stdout_file_val == {"file": "/tmp/test_stdout.txt"}
)

# Test: stdout file redirect — run echo with stdout redirected to a file,
# then read the file back with cat to confirm content was written.
stdout_redirect_path = "/tmp/test_stdout_redirect.txt"
process_run(process_options(
    "echo",
    args = ["stdout_redirect_content"],
    stdout = process_stdout_file(stdout_redirect_path),
))
stdout_redirect_read = process_capture(["cat", stdout_redirect_path])
process_results["stdout_helpers"]["stdout_file_redirect"] = (
    "stdout_redirect_content" in stdout_redirect_read
)

# ============================================================================
# Stderr Helper Tests
# ============================================================================

# Test process_stderr_inherit helper
stderr_inherit = process_stderr_inherit()
process_results["stderr_helpers"]["stderr_inherit"] = stderr_inherit == "inherit"

# Test process_stderr_capture helper
stderr_capture_val = process_stderr_capture()
process_results["stderr_helpers"]["stderr_capture"] = stderr_capture_val == "capture"

# Test process_stderr_null helper
stderr_null = process_stderr_null()
process_results["stderr_helpers"]["stderr_null"] = stderr_null == "null"

# Test process_stderr_merge helper (value check)
stderr_merge_val = process_stderr_merge()
process_results["stderr_helpers"]["stderr_merge_value"] = stderr_merge_val == "merge"

# Test process_stderr_file helper (value check)
stderr_file_val = process_stderr_file("/tmp/test_stderr.txt")
process_results["stderr_helpers"]["stderr_file_value"] = (
    stderr_file_val == {"file": "/tmp/test_stderr.txt"}
)

# Test: stderr file redirect — run a command that writes to stderr, redirect to
# a file, then verify the file contains the expected content.
stderr_redirect_path = "/tmp/test_stderr_redirect.txt"
process_run(process_options(
    "sh",
    args = ["-c", "echo stderr_redirect_content >&2"],
    stdout = process_stdout_null(),
    stderr = process_stderr_file(stderr_redirect_path),
))
stderr_redirect_read = process_capture(["cat", stderr_redirect_path])
process_results["stderr_helpers"]["stderr_file_redirect"] = (
    "stderr_redirect_content" in stderr_redirect_read
)

# Test: merge stderr into stdout — use process_stderr_merge() so that output
# written to stderr appears in the captured stdout stream.
merge_result = process_run(process_options(
    "sh",
    args = ["-c", "echo merged_error_content >&2"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_merge(),
))
process_results["stderr_helpers"]["stderr_merge_into_stdout"] = (
    "merged_error_content" in merge_result["stdout"]
)

# ============================================================================
# Process Options Builder Tests
# ============================================================================

# Test process_options builder — simple case
opts_simple = process_options("echo", args = ["hello"])
process_results["options_builder"]["simple_options"] = (
    opts_simple["command"] == "echo" and
    opts_simple["args"] == ["hello"]
)

# Test process_options builder — with stdout helper
opts_capture = process_options("echo", args = ["test"], stdout = process_stdout_capture())
process_results["options_builder"]["options_with_stdout"] = opts_capture["stdout"] == "capture"

# Test process_options builder — with stderr helper
opts_stderr = process_options("echo", args = ["test"], stderr = process_stderr_merge())
process_results["options_builder"]["options_with_stderr"] = opts_stderr["stderr"] == "merge"

# Test process_options builder — full options with all fields set
opts_full = process_options(
    "echo",
    args = ["test"],
    env = {"VAR": "value"},
    cwd = "/tmp",
    stdin = "input",
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
    timeout_ms = 5000,
    check = True,
)
process_results["options_builder"]["full_options"] = (
    opts_full["command"] == "echo" and
    opts_full["args"] == ["test"] and
    opts_full["env"] == {"VAR": "value"} and
    opts_full["cwd"] == "/tmp" and
    opts_full["stdin"] == "input" and
    opts_full["stdout"] == "capture" and
    opts_full["stderr"] == "capture" and
    opts_full["timeout_ms"] == 5000 and
    opts_full["check"] == True
)

# Test process_options builder — default values are NOT included in the dict
opts_defaults = process_options("echo")
process_results["options_builder"]["defaults_omitted"] = (
    "args" not in opts_defaults and
    "env" not in opts_defaults and
    "cwd" not in opts_defaults and
    "stdin" not in opts_defaults and
    "stdout" not in opts_defaults and
    "stderr" not in opts_defaults and
    "timeout_ms" not in opts_defaults and
    "check" not in opts_defaults
)

# Test process_run using a process_options-built dict
opts_for_run = process_options(
    "echo",
    args = ["output from builder"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
)
run_with_builder = process_run(opts_for_run)
process_results["options_builder"]["run_with_built_options"] = (
    "output from builder" in run_with_builder["stdout"] and
    run_with_builder["status"] == 0
)

# ============================================================================
# Output Results
# ============================================================================

print("Process Module Test Results:")
print("===========================")
print("")
print(json_dumps(process_results, is_pretty = True))
print("")
print("All process functions executed successfully!")
