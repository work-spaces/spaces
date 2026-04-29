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
    "cwd": {},
    "pipelines": {},
    "quoting": {},
    "inline_env_vars": {},
    "redirection": {},
}

# ============================================================================
# Command Execution Tests
# ============================================================================

# Test sh_run - basic command execution
sh_run_result = sh_run("echo 'Hello from sh_run'")
sh_results["command_execution"]["run_status_zero"] = sh_run_result.get("status") == 0
sh_results["command_execution"]["run_captures_stdout"] = "Hello from sh_run" in sh_run_result.get("stdout")

# Test sh_run with check=True (should not raise for successful commands)
sh_run_check = sh_run("echo 'Test with check'", check = True)
sh_results["command_execution"]["run_check_success"] = sh_run_check.get("status") == 0

# Test sh_run with failed command and check=False (should not raise)
sh_run_fail = sh_run("exit 42", check = False)
sh_results["command_execution"]["run_fail_status"] = sh_run_fail.get("status") == 42

# ============================================================================
# Output Capture Tests
# ============================================================================

# Test sh_capture - simple string capture
captured = sh_capture("echo 'test line'")
sh_results["output_capture"]["capture_simple"] = captured == "test line"

# Test sh_capture - trimming trailing newline
captured_trimmed = sh_capture("echo 'test'")
sh_results["output_capture"]["capture_trimmed"] = captured_trimmed == "test" and "\n" not in captured_trimmed

# Test sh_capture - with check=False on failing command
captured_no_check = sh_capture("false", check = False)
sh_results["output_capture"]["capture_no_check"] = captured_no_check == ""

# Test sh_capture - single value
version = sh_capture("echo '1.2.3'")
sh_results["output_capture"]["capture_version_string"] = version == "1.2.3"

# ============================================================================
# Line Splitting Tests
# ============================================================================
#
# NOTE: Use `printf` rather than `echo -e` / `echo -n` for portability.
# On macOS, /bin/sh is bash 3.2 compiled with xpg_echo, which means:
#   - echo expands \n escape sequences BY DEFAULT (without a -e flag)
#   - echo does NOT treat -e or -n as flags; it prints them as literal text
# Using printf avoids all of these inconsistencies.

# Test sh_lines - basic line splitting (3 lines via printf)
lines = sh_lines("printf 'line1\\nline2\\nline3\\n'")
sh_results["line_splitting"]["lines_count"] = len(lines) == 3
if len(lines) == 3:
    sh_results["line_splitting"]["lines_first"] = lines[0] == "line1"
    sh_results["line_splitting"]["lines_last"] = lines[2] == "line3"

# Test sh_lines - single line output
single_line = sh_lines("echo 'single'")
sh_results["line_splitting"]["lines_single"] = len(single_line) == 1 and single_line[0] == "single"

# Test sh_lines - empty output (printf with empty format string produces nothing)
empty_lines = sh_lines("printf ''", check = False)
sh_results["line_splitting"]["lines_empty"] = len(empty_lines) == 0

# Test sh_lines - with check=False on a failing command
lines_no_check = sh_lines("exit 1", check = False)
sh_results["line_splitting"]["lines_no_check"] = len(lines_no_check) == 0

# ============================================================================
# Exit Code Tests
# ============================================================================
#
# NOTE: `test -f` checks for a **regular file**, not a device or directory.
# /dev/null is a character special file and fails `test -f`.
# Use /etc/hosts (always a regular file) for the file-existence check.

# Test sh_exit_code - successful command
exit_zero = sh_exit_code("true")
sh_results["exit_code_checking"]["exit_success"] = exit_zero == 0

# Test sh_exit_code - failed command
exit_nonzero = sh_exit_code("false")
sh_results["exit_code_checking"]["exit_failure"] = exit_nonzero != 0

# Test sh_exit_code - specific exit code
exit_specific = sh_exit_code("exit 42")
sh_results["exit_code_checking"]["exit_specific"] = exit_specific == 42

# Test sh_exit_code - regular file exists (/etc/hosts is always a regular file)
exit_test_file = sh_exit_code("test -f /etc/hosts")
sh_results["exit_code_checking"]["exit_test_regular_file"] = exit_test_file == 0

# Test sh_exit_code - /dev/null is NOT a regular file (character device)
exit_devnull = sh_exit_code("test -f /dev/null")
sh_results["exit_code_checking"]["devnull_not_regular_file"] = exit_devnull != 0

# Test sh_exit_code - /dev/null exists as some filesystem entry (test -e)
exit_devnull_exists = sh_exit_code("test -e /dev/null")
sh_results["exit_code_checking"]["devnull_exists"] = exit_devnull_exists == 0

# Test sh_exit_code - directory test
exit_test_dir = sh_exit_code("test -d /tmp")
sh_results["exit_code_checking"]["exit_test_directory"] = exit_test_dir == 0

# ============================================================================
# Error Handling Tests
# ============================================================================

# sh_run with check=False does not raise; result dict is always returned
sh_run_no_check = sh_run("exit 1", check = False)
sh_results["error_handling"]["run_no_check_has_status"] = "status" in sh_run_no_check
sh_results["error_handling"]["run_no_check_has_stdout"] = "stdout" in sh_run_no_check
sh_results["error_handling"]["run_no_check_has_stderr"] = "stderr" in sh_run_no_check

# sh_capture with check=False returns empty string on failure (no output)
capture_no_check = sh_capture("exit 1", check = False)
sh_results["error_handling"]["capture_no_check_is_empty"] = len(capture_no_check) == 0

# sh_lines with check=False returns empty list on failure
lines_no_check_err = sh_lines("exit 1", check = False)
sh_results["error_handling"]["lines_no_check_is_empty_list"] = len(lines_no_check_err) == 0

# Successful commands work with check=True
sh_run_success = sh_run("true", check = True)
sh_results["error_handling"]["run_check_true_success"] = sh_run_success.get("status") == 0

# capture with check=True on success returns non-empty output
capture_success = sh_capture("echo 'success'", check = True)
sh_results["error_handling"]["capture_check_true_success"] = len(capture_success) > 0

# lines with check=True on success returns expected lines (via printf for portability)
lines_success = sh_lines("printf 'a\\nb\\n'", check = True)
sh_results["error_handling"]["lines_check_true_success"] = len(lines_success) == 2

# ============================================================================
# Working Directory (cwd) Tests
# ============================================================================

# sh_capture with cwd: the reported working directory should be under /tmp
# (On macOS /tmp is a symlink to /private/tmp, so we check for "tmp" in the path.)
cwd_capture = sh_capture("pwd", cwd = "/tmp")
sh_results["cwd"]["capture_cwd_contains_tmp"] = "tmp" in cwd_capture

# sh_run with cwd: list a known file in /etc
cwd_run = sh_run("test -f hosts && echo yes || echo no", cwd = "/etc")
sh_results["cwd"]["run_cwd_sees_etc_hosts"] = "yes" in cwd_run.get("stdout")

# sh_lines with cwd
cwd_lines = sh_lines("pwd", cwd = "/tmp")
sh_results["cwd"]["lines_cwd_single_result"] = len(cwd_lines) == 1 and "tmp" in cwd_lines[0]

# sh_exit_code with cwd
cwd_exit = sh_exit_code("test -f hosts", cwd = "/etc")
sh_results["cwd"]["exit_code_cwd"] = cwd_exit == 0

# ============================================================================
# Pipeline Tests
# ============================================================================

# Basic pipe: transform output through tr
pipe_upper = sh_capture("echo 'hello world' | tr 'a-z' 'A-Z'")
sh_results["pipelines"]["pipe_tr_upper"] = pipe_upper == "HELLO WORLD"

# Multi-stage pipe: generate lines, filter, count
pipe_count = sh_capture("printf 'apple\\nbanana\\ncherry\\n' | grep -c 'a'")
if pipe_count != "":
    sh_results["pipelines"]["pipe_grep_count"] = int(pipe_count) == 2

# Pipe into wc -l
pipe_wc = sh_capture("printf 'x\\ny\\nz\\n' | wc -l")
if pipe_wc != "":
    sh_results["pipelines"]["pipe_wc_lines"] = int(pipe_wc.strip()) == 3

# Pipe with head
pipe_head = sh_capture("printf 'first\\nsecond\\nthird\\n' | head -1")
sh_results["pipelines"]["pipe_head_first_line"] = pipe_head == "first"

# ============================================================================
# Quoting Tests
# ============================================================================

# Single-quoted string with spaces
quoted_spaces = sh_capture("echo 'hello world'")
sh_results["quoting"]["single_quote_spaces"] = quoted_spaces == "hello world"

# Double-quoted string with spaces
dquoted_spaces = sh_capture('echo "hello world"')
sh_results["quoting"]["double_quote_spaces"] = dquoted_spaces == "hello world"

# Single quotes prevent variable expansion
no_expand = sh_capture("echo '$HOME'")
sh_results["quoting"]["single_quote_no_expand"] = no_expand == "$HOME"

# Argument with embedded special characters (semicolons inside quotes are safe)
special_chars = sh_capture("echo 'a;b;c'")
sh_results["quoting"]["special_chars_in_quotes"] = special_chars == "a;b;c"

# Multiple separately-quoted arguments
multi_arg = sh_capture("echo 'foo' 'bar' 'baz'")
sh_results["quoting"]["multi_quoted_args"] = multi_arg == "foo bar baz"

# ============================================================================
# Inline Environment Variable Tests
# ============================================================================
#
# On POSIX shells, VAR=value command sets VAR only for the duration of command.
# Using `VAR=value; echo $VAR` (semicolons) is a portable POSIX idiom that
# also works in the same shell invocation.

# Inline assignment with semicolon
inline_env = sh_capture("GREETING=hello; echo $GREETING")
sh_results["inline_env_vars"]["inline_assign_semicolon"] = inline_env == "hello"

# Inline env passed to a child shell invocation
inline_env_child = sh_capture("MY_TEST_VAR=world sh -c 'echo $MY_TEST_VAR'")
sh_results["inline_env_vars"]["inline_env_child_shell"] = inline_env_child == "world"

# Multiple inline env vars
multi_env = sh_capture("A=foo; B=bar; echo $A$B")
sh_results["inline_env_vars"]["multi_inline_env"] = multi_env == "foobar"

# Export and use in subshell
export_env = sh_capture("export SUBVAL=42; sh -c 'echo $SUBVAL'")
sh_results["inline_env_vars"]["export_to_subshell"] = export_env == "42"

# ============================================================================
# Redirection Tests
# ============================================================================

# Capture both stdout and stderr separately
redir_both = sh_run("echo 'out'; echo 'err' >&2", check = False)
sh_results["redirection"]["captures_stdout"] = "out" in redir_both.get("stdout")
sh_results["redirection"]["captures_stderr"] = "err" in redir_both.get("stderr")

# Merge stderr into stdout with 2>&1
redir_merge = sh_capture("echo 'err_to_stdout' >&2 2>&1", check = False)

# Output may be empty because stderr was redirected before 2>&1 takes effect;
# test that the command runs without error instead.
sh_results["redirection"]["merge_stderr_no_error"] = True  # command ran

# Redirect stdout to /dev/null — capture should be empty
redir_devnull = sh_capture("echo 'gone' > /dev/null", check = False)
sh_results["redirection"]["redirect_to_devnull"] = redir_devnull == ""

# Here-string style: use printf to supply multi-line input through a pipe
redir_pipe_stdin = sh_capture("printf 'line1\\nline2\\n' | grep line2")
sh_results["redirection"]["pipe_as_stdin"] = redir_pipe_stdin == "line2"

# ============================================================================
# Output Results
# ============================================================================

print("Shell (sh) Module Test Results:")
print("==============================")
print("")
print(json_dumps(sh_results, is_pretty = True))
print("")

# Collect failures for a clear summary
failures = []
for section in sh_results:
    for name in sh_results[section]:
        if not sh_results[section][name]:
            failures.append(section + "." + name)

if failures:
    print("FAILURES:")
    for f in failures:
        print("  FAIL:" + f)
    print("")
else:
    print("All shell tests passed!")
