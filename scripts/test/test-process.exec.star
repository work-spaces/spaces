#!/usr/bin/env spaces

load(
    "//@star/prelude/exec/json.star",
    "json_dumps",
)
load(
    "//@star/prelude/exec/process.star",
    "process_capture",
    "process_is_running",
    "process_kill",
    "process_options",
    "process_pipeline",
    "process_read_lines",
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
load(
    "//@star/prelude/exec/time.star",
    "time_sleep_milliseconds",
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
    "streaming_output": {},
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
process_results["basic_execution"]["nonzero_exit_code"] = nonzero_result.get("status") == 2

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
    check_success.get("status") == 0 and
    "check ok" in check_success.get("stdout")
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
    run_result.get("status") == 0 and
    "test output" in run_result.get("stdout")
)

# Test: stderr capture — command writes only to stderr; verify it is captured
stderr_result = process_run(process_options(
    "sh",
    args = ["-c", "echo error_message >&2"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
))
process_results["output_capture"]["stderr_captured"] = (
    "error_message" in stderr_result.get("stderr") and
    stderr_result.get("status") == 0
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
    "hello from stdin" in stdin_result.get("stdout") and
    stdin_result.get("status") == 0
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
    "spaces_env_value_42" in env_result.get("stdout") and
    env_result.get("status") == 0
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
    cwd_result.get("status") == 0 and
    "tmp" in cwd_result.get("stdout")
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
process_results["background_processes"]["wait_status_zero"] = wait_result.get("status") == 0
process_results["background_processes"]["wait_stdout_content"] = (
    "spawned process" in wait_result.get("stdout")
)

# Test: spawn accepts allow_orphans=False in options and process can be managed.
allow_orphans_false_handle = process_spawn(process_options(
    "sleep",
    args = ["30"],
    allow_orphans = False,
))
process_results["background_processes"]["spawn_allow_orphans_false_handle"] = (
    allow_orphans_false_handle > 0
)
process_results["background_processes"]["spawn_allow_orphans_false_running"] = (
    process_is_running(allow_orphans_false_handle)
)
process_kill(allow_orphans_false_handle, "SIGKILL")
allow_orphans_false_wait = process_wait(allow_orphans_false_handle)
process_results["background_processes"]["spawn_allow_orphans_false_wait_nonzero"] = (
    allow_orphans_false_wait.get("status") != 0
)

# Test: spawn accepts allow_orphans=True in options and process can be managed.
allow_orphans_true_handle = process_spawn(process_options(
    "sleep",
    args = ["30"],
    allow_orphans = True,
))
process_results["background_processes"]["spawn_allow_orphans_true_handle"] = (
    allow_orphans_true_handle > 0
)
process_results["background_processes"]["spawn_allow_orphans_true_running"] = (
    process_is_running(allow_orphans_true_handle)
)
process_kill(allow_orphans_true_handle, "SIGKILL")
allow_orphans_true_wait = process_wait(allow_orphans_true_handle)
process_results["background_processes"]["spawn_allow_orphans_true_wait_nonzero"] = (
    allow_orphans_true_wait.get("status") != 0
)

# Test: spawn named argument API for allow_orphans is accepted.
spawn_allow_orphans_arg_false = process_spawn(
    process_options("sleep", args = ["30"]),
    allow_orphans = False,
)
process_results["background_processes"]["spawn_allow_orphans_arg_false_handle"] = (
    spawn_allow_orphans_arg_false > 0
)
process_results["background_processes"]["spawn_allow_orphans_arg_false_running"] = (
    process_is_running(spawn_allow_orphans_arg_false)
)
process_kill(spawn_allow_orphans_arg_false, "SIGKILL")
spawn_allow_orphans_arg_false_wait = process_wait(spawn_allow_orphans_arg_false)
process_results["background_processes"]["spawn_allow_orphans_arg_false_wait_nonzero"] = (
    spawn_allow_orphans_arg_false_wait.get("status") != 0
)

spawn_allow_orphans_arg_true = process_spawn(
    process_options("sleep", args = ["30"]),
    allow_orphans = True,
)
process_results["background_processes"]["spawn_allow_orphans_arg_true_handle"] = (
    spawn_allow_orphans_arg_true > 0
)
process_results["background_processes"]["spawn_allow_orphans_arg_true_running"] = (
    process_is_running(spawn_allow_orphans_arg_true)
)
process_kill(spawn_allow_orphans_arg_true, "SIGKILL")
spawn_allow_orphans_arg_true_wait = process_wait(spawn_allow_orphans_arg_true)
process_results["background_processes"]["spawn_allow_orphans_arg_true_wait_nonzero"] = (
    spawn_allow_orphans_arg_true_wait.get("status") != 0
)

# ============================================================================
# Streaming Output Tests (process_read_lines)
# ============================================================================

# Spawn a long-running process that writes to both stdout and stderr immediately.
# Use tee=True to ensure spawn accepts tee while still buffering for read_lines().
streaming_handle = process_spawn(process_options(
    "sh",
    args = ["-c", "echo live_stdout_line; echo live_stderr_line >&2; sleep 30"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
    tee = True,
))
process_results["streaming_output"]["streaming_handle_created"] = streaming_handle > 0

# Poll non-destructively for output visibility while process is still running.
# Sleep 10 ms between iterations so the output-pump threads have time to
# buffer data and we avoid spinning on a busy CI runner.
seen_stdout = False
seen_stderr = False
for _ in range(200):
    if not process_is_running(streaming_handle):
        break
    snapshot_stdout = process_read_lines(streaming_handle, "stdout", drain = False)
    snapshot_stderr = process_read_lines(streaming_handle, "stderr", drain = False)
    if "live_stdout_line" in snapshot_stdout:
        seen_stdout = True
    if "live_stderr_line" in snapshot_stderr:
        seen_stderr = True
    if seen_stdout and seen_stderr:
        break
    time_sleep_milliseconds(10)

process_results["streaming_output"]["read_output_visible_before_wait"] = (
    seen_stdout and seen_stderr
)

# Default drain=True should consume buffered bytes.
drained_stdout = process_read_lines(streaming_handle, "stdout")
drained_stderr = process_read_lines(streaming_handle, "stderr")
process_results["streaming_output"]["read_output_default_drain_stdout"] = (
    "live_stdout_line" in drained_stdout
)
process_results["streaming_output"]["read_output_default_drain_stderr"] = (
    "live_stderr_line" in drained_stderr
)

# After draining, non-destructive snapshot should now be empty.
post_drain_stdout = process_read_lines(streaming_handle, "stdout", drain = False)
post_drain_stderr = process_read_lines(streaming_handle, "stderr", drain = False)
process_results["streaming_output"]["read_output_drain_clears_stdout"] = (
    post_drain_stdout == []
)
process_results["streaming_output"]["read_output_drain_clears_stderr"] = (
    post_drain_stderr == []
)

# Test: max_lines limits how many complete lines are returned and drained per call.
max_lines_handle = process_spawn(process_options(
    "sh",
    args = ["-c", "echo max_lines_1; echo max_lines_2; echo max_lines_3; sleep 30"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
))
process_results["streaming_output"]["read_output_max_lines_handle_created"] = (
    max_lines_handle > 0
)

# Give the spawned process a moment to emit lines before reading.
process_run(process_options("sleep", args = ["1"]))

max_lines_first = process_read_lines(max_lines_handle, "stdout", max_lines = 2)
max_lines_first_stderr = process_read_lines(max_lines_handle, "stderr", max_lines = 2)
process_results["streaming_output"]["read_output_max_lines_first_chunk"] = (
    max_lines_first == ["max_lines_1", "max_lines_2"] and
    max_lines_first_stderr == []
)

max_lines_second = process_read_lines(max_lines_handle, "stdout", max_lines = 2)
max_lines_second_stderr = process_read_lines(max_lines_handle, "stderr", max_lines = 2)
process_results["streaming_output"]["read_output_max_lines_second_chunk"] = (
    max_lines_second == ["max_lines_3"] and
    max_lines_second_stderr == []
)

max_lines_after_stdout = process_read_lines(max_lines_handle, "stdout", drain = False)
max_lines_after_stderr = process_read_lines(max_lines_handle, "stderr", drain = False)
process_results["streaming_output"]["read_output_max_lines_drain_preserves_remainder"] = (
    max_lines_after_stdout == [] and
    max_lines_after_stderr == []
)

# Test: max_lines with drain=False does not consume data.
max_lines_nondrain_handle = process_spawn(process_options(
    "sh",
    args = ["-c", "echo max_lines_snapshot_1; echo max_lines_snapshot_2; sleep 30"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
))
process_results["streaming_output"]["read_output_max_lines_nondrain_handle_created"] = (
    max_lines_nondrain_handle > 0
)

# Give the spawned process a moment to emit lines before reading.
process_run(process_options("sleep", args = ["1"]))

max_lines_nondrain_first = process_read_lines(
    max_lines_nondrain_handle,
    "stdout",
    drain = False,
    max_lines = 1,
)
max_lines_nondrain_second = process_read_lines(
    max_lines_nondrain_handle,
    "stdout",
    drain = False,
    max_lines = 1,
)
max_lines_nondrain_final = process_read_lines(max_lines_nondrain_handle, "stdout")

process_results["streaming_output"]["read_output_max_lines_nondrain_snapshot"] = (
    max_lines_nondrain_first == ["max_lines_snapshot_1"] and
    max_lines_nondrain_second == ["max_lines_snapshot_1"]
)
process_results["streaming_output"]["read_output_max_lines_nondrain_not_consumed"] = (
    max_lines_nondrain_final == ["max_lines_snapshot_1", "max_lines_snapshot_2"]
)

# Cleanup process handles.
process_kill(streaming_handle, "SIGKILL")
streaming_wait = process_wait(streaming_handle)
process_results["streaming_output"]["streaming_wait_nonzero_after_kill"] = (
    streaming_wait.get("status") != 0
)

process_kill(max_lines_handle, "SIGKILL")
max_lines_wait = process_wait(max_lines_handle)
process_results["streaming_output"]["read_output_max_lines_wait_has_status"] = (
    "status" in max_lines_wait
)

process_kill(max_lines_nondrain_handle, "SIGKILL")
max_lines_nondrain_wait = process_wait(max_lines_nondrain_handle)
process_results["streaming_output"]["read_output_max_lines_nondrain_wait_has_status"] = (
    "status" in max_lines_nondrain_wait
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
    kill_wait_result.get("status") != 0
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
    "line2" in pipeline_result.get("stdout") and
    pipeline_result.get("status") == 0
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
    "merged_error_content" in merge_result.get("stdout")
)

# ============================================================================
# Process Options Builder Tests
# ============================================================================

# Test process_options builder — simple case
opts_simple = process_options("echo", args = ["hello"])
process_results["options_builder"]["simple_options"] = (
    opts_simple.get("command") == "echo" and
    opts_simple.get("args") == ["hello"]
)

# Test process_options builder — with stdout helper
opts_capture = process_options("echo", args = ["test"], stdout = process_stdout_capture())
process_results["options_builder"]["options_with_stdout"] = opts_capture.get("stdout") == "capture"

# Test process_options builder — with stderr helper
opts_stderr = process_options("echo", args = ["test"], stderr = process_stderr_merge())
process_results["options_builder"]["options_with_stderr"] = opts_stderr.get("stderr") == "merge"

# Test process_options builder — with tee
opts_tee = process_options("echo", args = ["test"], tee = True)
process_results["options_builder"]["options_with_tee"] = opts_tee.get("tee") == True

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
    tee = True,
    allow_orphans = True,
)
process_results["options_builder"]["full_options"] = (
    opts_full.get("command") == "echo" and
    opts_full.get("args") == ["test"] and
    opts_full.get("env") == {"VAR": "value"} and
    opts_full.get("cwd") == "/tmp" and
    opts_full.get("stdin") == "input" and
    opts_full.get("stdout") == "capture" and
    opts_full.get("stderr") == "capture" and
    opts_full.get("timeout_ms") == 5000 and
    opts_full.get("check") == True and
    opts_full.get("tee") == True and
    opts_full.get("allow_orphans") == True
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
    "check" not in opts_defaults and
    "tee" not in opts_defaults and
    "allow_orphans" not in opts_defaults
)

# Test process_options builder — with allow_orphans True/False
opts_allow_orphans_true = process_options("echo", allow_orphans = True)
process_results["options_builder"]["options_with_allow_orphans_true"] = (
    opts_allow_orphans_true.get("allow_orphans") == True
)

opts_allow_orphans_false = process_options("echo", allow_orphans = False)
process_results["options_builder"]["options_with_allow_orphans_false"] = (
    opts_allow_orphans_false.get("allow_orphans") == False
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
    "output from builder" in run_with_builder.get("stdout") and
    run_with_builder.get("status") == 0
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
