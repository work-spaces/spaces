#!/usr/bin/env spaces

"""Integration-style exec-mode tests for io.star wrapper behavior."""

load(
    "//@star/prelude/exec/fs.star",
    "fs_write_text",
)
load(
    "//@star/prelude/exec/io.star",
    "io_flush_stderr",
    "io_flush_stdout",
    "io_stdin_is_terminal",
)
load(
    "//@star/prelude/exec/json.star",
    "json_dumps",
    "json_loads",
)
load(
    "//@star/prelude/exec/process.star",
    "process_options",
    "process_run",
    "process_stderr_capture",
    "process_stdout_capture",
)
load(
    "//@star/prelude/exec/sys.star",
    "sys_executable",
)
load(
    "//@star/prelude/exec/tmp.star",
    "tmp_cleanup_all",
    "tmp_file",
)

io_results = {
    "direct_calls": {},
    "stdout_stderr_writes": {},
    "stdin_reads": {},
    "error_paths": {},
}

def check(condition, label):
    assert_on(condition, "FAIL [{}]".format(label))

def record(section, key, condition):
    io_results[section][key] = condition
    check(condition, section + "." + key)

def run_io_child(source, stdin = None):
    """Executes a temporary child script and returns captured process output.

    Args:
      source: Script source to write into a temporary child .exec.star file.
      stdin: Optional stdin string piped to the child process.

    Returns:
      A process result dict from process_run(), including status/stdout/stderr.
    """
    child_script = tmp_file(suffix = ".exec.star")
    fs_write_text(child_script, source)

    if stdin == None:
        options = process_options(
            command = sys_executable(),
            args = [child_script],
            stdout = process_stdout_capture(),
            stderr = process_stderr_capture(),
        )
    else:
        options = process_options(
            command = sys_executable(),
            args = [child_script],
            stdin = stdin,
            stdout = process_stdout_capture(),
            stderr = process_stderr_capture(),
        )

    return process_run(options)

OUTPUT_SCRIPT = """#!/usr/bin/env spaces
load(
    \"//@star/prelude/exec/io.star\",
    \"io_eprint\",
    \"io_print\",
    \"io_write_stderr\",
    \"io_write_stdout\",
)

io_write_stdout(\"a\")
io_write_stdout(\"b\", newline = True, flush = True)
io_print(\"c\")
io_print(\"d\", end = \"\")
io_print(\"e\", end = \"!\", flush = True)

io_write_stderr(\"x\")
io_write_stderr(\"y\", newline = True, flush = True)
io_eprint(\"z\")
io_eprint(\"w\", end = \"\")
io_eprint(\"q\", end = \"?\", flush = True)
"""

READ_SCRIPT = """#!/usr/bin/env spaces
load(\"//@star/prelude/exec/io.star\", \"io_read_stdin\", \"io_read_stdin_lines\", \"io_read_stdin_to_string\")
load(\"//@star/prelude/exec/json.star\", \"json_dumps\")

results = {
    \"to_string\": io_read_stdin_to_string(max_bytes = 128),
    \"alias\": io_read_stdin(max_bytes = 128),
    \"strip\": io_read_stdin_to_string(strip_trailing_newline = True, max_bytes = 128),
    \"lossy\": io_read_stdin_to_string(encoding = \"lossy\", max_bytes = 128),
    \"lines_strip\": io_read_stdin_lines(max_lines = 4, max_bytes = 128),
    \"lines_keep\": io_read_stdin_lines(strip_newline = False, max_lines = 4, max_bytes = 128),
    \"lines_max2\": io_read_stdin_lines(max_lines = 2, max_bytes = 128),
    \"lines_lossy\": io_read_stdin_lines(encoding = \"lossy\", max_lines = 4, max_bytes = 128),
    \"bounded\": io_read_stdin_to_string(max_bytes = 128),
}

print(json_dumps(results))
"""

MAX_BYTES_FAIL_SCRIPT = """#!/usr/bin/env spaces
load(\"//@star/prelude/exec/io.star\", \"io_read_stdin_to_string\")
io_read_stdin_to_string(max_bytes = 3)
"""

MAX_LINES_FAIL_SCRIPT = """#!/usr/bin/env spaces
load(\"//@star/prelude/exec/io.star\", \"io_read_stdin_lines\")
io_read_stdin_lines(max_lines = 1, max_bytes = 128)
"""

INVALID_ENCODING_FAIL_SCRIPT = """#!/usr/bin/env spaces
load(\"//@star/prelude/exec/io.star\", \"io_read_stdin_to_string\")
io_read_stdin_to_string(encoding = \"utf16\", max_bytes = 128)
"""

# ============================================================================
# Direct wrapper calls
# ============================================================================

record("direct_calls", "stdin_is_terminal_returns_bool", type(io_stdin_is_terminal()) == "bool")
io_flush_stdout()
io_flush_stderr()
record("direct_calls", "flush_functions_callable", True)

# ============================================================================
# Stdout / stderr wrapper behavior
# ============================================================================

output_result = run_io_child(OUTPUT_SCRIPT)
record("stdout_stderr_writes", "child_exits_zero", output_result.get("status") == 0)
record("stdout_stderr_writes", "stdout_content", output_result.get("stdout") == "ab\nc\nde!")
record("stdout_stderr_writes", "stderr_content", output_result.get("stderr") == "xy\nz\nwq?")

# ============================================================================
# Stdin reading behavior
# ============================================================================

stdin_payload = "line1\nline2\n"
read_result = run_io_child(READ_SCRIPT, stdin = stdin_payload)
record("stdin_reads", "child_exits_zero", read_result.get("status") == 0)

read_data = json_loads(read_result.get("stdout"))
record("stdin_reads", "to_string", read_data.get("to_string") == stdin_payload)
record("stdin_reads", "alias_matches", read_data.get("alias") == stdin_payload)
record("stdin_reads", "strip_trailing_newline", read_data.get("strip") == "line1\nline2")
record("stdin_reads", "lossy_encoding", read_data.get("lossy") == stdin_payload)
record("stdin_reads", "lines_strip", read_data.get("lines_strip") == ["line1", "line2"])
record("stdin_reads", "lines_keep", read_data.get("lines_keep") == ["line1\n", "line2\n"])
record("stdin_reads", "lines_max2", read_data.get("lines_max2") == ["line1", "line2"])
record("stdin_reads", "lines_lossy", read_data.get("lines_lossy") == ["line1", "line2"])
record("stdin_reads", "max_bytes_ok", read_data.get("bounded") == stdin_payload)

# ============================================================================
# Error-path behavior
# ============================================================================

max_bytes_fail = run_io_child(MAX_BYTES_FAIL_SCRIPT, stdin = "abcdef")
record("error_paths", "max_bytes_exceeded_fails", max_bytes_fail.get("status") != 0)

max_lines_fail = run_io_child(MAX_LINES_FAIL_SCRIPT, stdin = "a\nb\n")
record("error_paths", "max_lines_exceeded_fails", max_lines_fail.get("status") != 0)

invalid_encoding_fail = run_io_child(INVALID_ENCODING_FAIL_SCRIPT, stdin = "abc")
record("error_paths", "invalid_encoding_fails", invalid_encoding_fail.get("status") != 0)

# Cleanup tracked temp files created by tmp_file()
tmp_cleanup_all()

print("IO Module Test Results:")
print("=======================")
print("")
print(json_dumps(io_results, is_pretty = True))
print("")
print("All io functions executed successfully!")
