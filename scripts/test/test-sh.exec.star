#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/sh.star",
    "sh_capture",
    "sh_exit_code",
    "sh_lines",
    "sh_run",
)

# Shell (sh) module test results
sh_results = {
    "command_execution": {},
    "output_capture": {},
    "line_splitting": {},
    "exit_code_checking": {},
    "error_handling": {},
}

# ============================================================================
# Shell Module Tests
# ============================================================================

# Test sh_run - basic command execution
sh_run_result = sh_run("echo 'Hello from sh_run'")
sh_results["command_execution"]["run_status_zero"] = sh_run_result["status"] == 0
sh_results["command_execution"]["run_captures_stdout"] = "Hello from sh_run" in sh_run_result["stdout"]

# Test sh_run with check=True (should not raise for successful commands)
sh_run_check = sh_run("echo 'Test with check'", check = True)
sh_results["command_execution"]["run_check_success"] = sh_run_check["status"] == 0

# Test sh_run with failed command and check=False (should not raise)
sh_run_fail = sh_run("exit 42", check = False)
sh_results["command_execution"]["run_fail_status"] = sh_run_fail["status"] == 42

# Test sh_run capturing both stdout and stderr
sh_run_both = sh_run("echo 'output'; echo 'error' >&2", check = False)
sh_results["command_execution"]["run_captures_both"] = "output" in sh_run_both["stdout"] and "error" in sh_run_both["stderr"]

# Test sh_capture - simple string capture
captured = sh_capture("echo 'test line'")
sh_results["output_capture"]["capture_simple"] = captured == "test line"

# Test sh_capture - trimming trailing whitespace
captured_trimmed = sh_capture("echo 'test'")
sh_results["output_capture"]["capture_trimmed"] = captured_trimmed == "test" and "\n" not in captured_trimmed

# Test sh_capture - with check=False on failing command
captured_no_check = sh_capture("false", check = False)
sh_results["output_capture"]["capture_no_check"] = captured_no_check == ""

# Test sh_capture - multiple values
version = sh_capture("echo '1.2.3'")
sh_results["output_capture"]["capture_multi_line"] = version == "1.2.3"

# Test sh_lines - basic line splitting
lines = sh_lines("echo -e 'line1\\nline2\\nline3'")
sh_results["line_splitting"]["lines_count"] = len(lines) == 3
if len(lines) == 3:
    sh_results["line_splitting"]["lines_first"] = lines[0] == "line1"
    sh_results["line_splitting"]["lines_last"] = lines[2] == "line3"

# Test sh_lines - single line output
single_line = sh_lines("echo 'single'")
sh_results["line_splitting"]["lines_single"] = len(single_line) == 1 and single_line[0] == "single"

# Test sh_lines - empty output
empty_lines = sh_lines("echo -n ''", check = False)
sh_results["line_splitting"]["lines_empty"] = len(empty_lines) == 0

# Test sh_lines - with check=False
lines_no_check = sh_lines("exit 1", check = False)
sh_results["line_splitting"]["lines_no_check"] = len(lines_no_check) == 0

# Test sh_exit_code - successful command
exit_zero = sh_exit_code("true")
sh_results["exit_code_checking"]["exit_success"] = exit_zero == 0

# Test sh_exit_code - failed command
exit_nonzero = sh_exit_code("false")
sh_results["exit_code_checking"]["exit_failure"] = exit_nonzero != 0

# Test sh_exit_code - specific exit code
exit_specific = sh_exit_code("exit 42")
sh_results["exit_code_checking"]["exit_specific"] = exit_specific == 42

# Test sh_exit_code - file test command
exit_test_file = sh_exit_code("test -f /dev/null")
sh_results["exit_code_checking"]["exit_test"] = exit_test_file == 0

# Test error handling - sh_run with check=False does not raise
sh_run_no_check = sh_run("exit 1", check = False)
sh_results["error_handling"]["run_no_check_has_status"] = "status" in sh_run_no_check
sh_results["error_handling"]["run_no_check_has_stdout"] = "stdout" in sh_run_no_check

# Test error handling - sh_capture with check=False returns empty string on failure
capture_no_check = sh_capture("exit 1", check = False)
sh_results["error_handling"]["capture_no_check_is_empty"] = len(capture_no_check) == 0

# Test error handling - sh_lines with check=False returns empty list on failure
lines_no_check = sh_lines("exit 1", check = False)
sh_results["error_handling"]["lines_no_check_is_empty_list"] = len(lines_no_check) == 0

# Test error handling - successful commands work with check=True
sh_run_success = sh_run("true", check = True)
sh_results["error_handling"]["run_check_true_success"] = sh_run_success["status"] == 0

# Test error handling - capture with check=True on success
capture_success = sh_capture("echo 'success'", check = True)
sh_results["error_handling"]["capture_check_true_success"] = len(capture_success) > 0

# Test error handling - lines with check=True on success
lines_success = sh_lines("echo -e 'a\\nb'", check = True)
sh_results["error_handling"]["lines_check_true_success"] = len(lines_success) == 2

# ============================================================================
# Output Results
# ============================================================================

print("Shell (sh) Module Test Results:")
print("==============================")
print("")
print(json_dumps(sh_results, is_pretty = True))
print("")
print("All shell functions executed successfully!")
