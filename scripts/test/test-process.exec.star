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
# Process Module Tests
# ============================================================================

# Test process_capture - simple output capture
capture_result = process_capture(["echo", "captured output"])
process_results["output_capture"]["simple_capture"] = "captured output" in capture_result

# Test process_run with output capture
run_result = process_run({
    "command": "echo",
    "args": ["test output"],
    "stdout": "capture",
    "stderr": "capture",
})
process_results["output_capture"]["run_capture"] = run_result["status"] == 0 and "test output" in run_result["stdout"]

# Test process_spawn and process_wait
spawn_result = process_spawn({
    "command": "echo",
    "args": ["spawned process"],
})
process_results["background_processes"]["spawn_returns_handle"] = spawn_result > 0

# Test process_is_running (process should have already finished)
is_running_result = process_is_running(spawn_result)
process_results["process_management"]["is_running_check"] = not is_running_result

# Test process_pipeline - multiple commands
pipeline_result = process_pipeline([
    {"command": "echo", "args": ["line1\nline2\nline3"]},
    {"command": "grep", "args": ["line2"]},
])
process_results["pipelines"]["basic_pipeline"] = "line2" in pipeline_result["stdout"] and pipeline_result["status"] == 0

# ============================================================================
# Process Options Builder and Helper Tests
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

# Test process_stdout_file helper
stdout_file = process_stdout_file("/tmp/test_stdout.txt")
process_results["stdout_helpers"]["stdout_file"] = stdout_file == {"file": "/tmp/test_stdout.txt"}

# Test process_stderr_inherit helper
stderr_inherit = process_stderr_inherit()
process_results["stderr_helpers"]["stderr_inherit"] = stderr_inherit == "inherit"

# Test process_stderr_capture helper
stderr_capture = process_stderr_capture()
process_results["stderr_helpers"]["stderr_capture"] = stderr_capture == "capture"

# Test process_stderr_null helper
stderr_null = process_stderr_null()
process_results["stderr_helpers"]["stderr_null"] = stderr_null == "null"

# Test process_stderr_merge helper
stderr_merge = process_stderr_merge()
process_results["stderr_helpers"]["stderr_merge"] = stderr_merge == "merge"

# Test process_stderr_file helper
stderr_file = process_stderr_file("/tmp/test_stderr.txt")
process_results["stderr_helpers"]["stderr_file"] = stderr_file == {"file": "/tmp/test_stderr.txt"}

# Test process_options builder - simple case
opts_simple = process_options("echo", args = ["hello"])
process_results["options_builder"]["simple_options"] = opts_simple["command"] == "echo" and opts_simple["args"] == ["hello"]

# Test process_options builder - with stdout helper
opts_capture = process_options("echo", args = ["test"], stdout = process_stdout_capture())
process_results["options_builder"]["options_with_stdout"] = opts_capture["stdout"] == "capture"

# Test process_options builder - with stderr helper
opts_stderr = process_options("echo", args = ["test"], stderr = process_stderr_merge())
process_results["options_builder"]["options_with_stderr"] = opts_stderr["stderr"] == "merge"

# Test process_options builder - full options
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

# Test process_options builder - default values not included
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

# Test process_run with options builder
opts_for_run = process_options(
    "echo",
    args = ["output from builder"],
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
)
run_with_builder = process_run(opts_for_run)
process_results["options_builder"]["run_with_built_options"] = "output from builder" in run_with_builder["stdout"] and run_with_builder["status"] == 0

# ============================================================================
# Output Results
# ============================================================================

print("Process Module Test Results:")
print("===========================")
print("")
print(json_dumps(process_results, is_pretty = True))
print("")
print("All process functions executed successfully!")
