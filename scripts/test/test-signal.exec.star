#!/usr/bin/env spaces

"""Integration-style exec-mode tests for signal.star wrapper behavior."""

load(
    "//@star/prelude/exec/fs.star",
    "fs_write_text",
)
load(
    "//@star/prelude/exec/json.star",
    "json_dumps",
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

signal_results = {
    "timeout_and_state": {},
    "trap_and_wait": {},
    "pending_and_dispatch": {},
    "multi_signal_wrappers": {},
    "error_paths": {},
}

def check(condition, label):
    assert_on(condition, "FAIL [{}]".format(label))

def record(section, key, condition):
    signal_results[section][key] = condition
    check(condition, section + "." + key)

def run_signal_child(source):
    """Execute a temporary child exec script and capture status/stdout/stderr."""
    child_script = tmp_file(suffix = ".exec.star")
    fs_write_text(child_script, source)
    return process_run(process_options(
        command = sys_executable(),
        args = [child_script],
        stdout = process_stdout_capture(),
        stderr = process_stderr_capture(),
    ))

def stdout_contains(result, needle):
    text = result.get("stdout")
    return type(text) == "string" and needle in text

def output_contains(result, needle):
    stdout = result.get("stdout")
    stderr = result.get("stderr")
    return (type(stdout) == "string" and needle in stdout) or (type(stderr) == "string" and needle in stderr)

# ============================================================================
# Child scripts used for positive-path integration checks
# ============================================================================

TIMEOUT_STATE_SCRIPT = """#!/usr/bin/env spaces
load("//@star/prelude/exec/signal.star", "signal_clear", "signal_dispatch", "signal_pending", "signal_wait")

signal_clear()
signal_dispatch()

print("PENDING_TYPE=" + type(signal_pending()))
print("PENDING_LEN=" + str(len(signal_pending())))
print("WAIT_TIMEOUT_IS_NONE=" + str(signal_wait(timeout_ms = 25) == None))
"""

TRAP_REPLACE_WAIT_SCRIPT = """#!/usr/bin/env spaces
load("//@star/prelude/exec/sh.star", "sh_exit_code")
load("//@star/prelude/exec/signal.star", "signal_clear", "signal_dispatch", "signal_pending", "signal_trap", "signal_wait")

signal_clear()
signal_dispatch()

# Register fail first, then replace with print. If replacement fails, wait() would error.
signal_trap("USR1", fail)
signal_trap("SIGUSR1", print)

send_status = sh_exit_code("kill -s USR1 $PPID")
observed = signal_wait(timeout_ms = 1000)

print("SEND_STATUS=" + str(send_status))
print("OBSERVED=" + str(observed))
print("PENDING_AFTER=" + str(len(signal_pending())))
"""

PENDING_DISPATCH_SCRIPT = """#!/usr/bin/env spaces
load("//@star/prelude/exec/sh.star", "sh_exit_code")
load("//@star/prelude/exec/signal.star", "signal_clear", "signal_dispatch", "signal_pending", "signal_trap")
load("//@star/prelude/exec/time.star", "time_sleep_milliseconds")

def _all_strings(items):
    for item in items:
        if type(item) != \"string\":
            return False
    return True

signal_clear()
signal_dispatch()
signal_trap(\"USR2\", print)

send_status = sh_exit_code(\"kill -s USR2 $PPID\")
pending = signal_pending()
for _ in range(50):
    if "USR2" not in pending:
        time_sleep_milliseconds(20)
        pending = signal_pending()

print("SEND_STATUS=" + str(send_status))
print("PENDING_HAS_USR2=" + str("USR2" in pending))
print("PENDING_ALL_STRINGS=" + str(_all_strings(pending)))

dispatched = signal_dispatch()
print("DISPATCHED=" + str(dispatched))
print("PENDING_AFTER=" + str(len(signal_pending())))
"""

MULTI_SIGNAL_WRAPPERS_SCRIPT = """#!/usr/bin/env spaces
load("//@star/prelude/exec/sh.star", "sh_exit_code")
load("//@star/prelude/exec/signal.star", "signal_clear", "signal_dispatch", "signal_pending", "signal_trap", "signal_untrap", "signal_wait")

signal_clear()
signal_dispatch()
signal_trap([\"USR1\", \"SIGUSR2\"], print)

send1 = sh_exit_code(\"kill -s USR1 $PPID\")
obs1 = signal_wait(timeout_ms = 1000)

send2 = sh_exit_code(\"kill -s USR2 $PPID\")
obs2 = signal_wait(timeout_ms = 1000)

signal_untrap([\"USR1\", \"SIGUSR2\"])
signal_clear()

print("SEND1=" + str(send1))
print("OBS1=" + str(obs1))
print("SEND2=" + str(send2))
print("OBS2=" + str(obs2))
print("PENDING_AFTER=" + str(len(signal_pending())))
"""

# ============================================================================
# Child scripts used for error-path checks
# ============================================================================

INVALID_TRAP_TYPE_SCRIPT = """#!/usr/bin/env spaces
load("//@star/prelude/exec/signal.star", "signal_trap")
signal_trap(123, print)
"""

INVALID_TRAP_LIST_ITEM_SCRIPT = """#!/usr/bin/env spaces
load("//@star/prelude/exec/signal.star", "signal_trap")
signal_trap(["USR1", 2], print)
"""

INVALID_SIGNAL_NAME_SCRIPT = """#!/usr/bin/env spaces
load("//@star/prelude/exec/signal.star", "signal_trap")
signal_trap("NOT_A_REAL_SIGNAL", print)
"""

NEGATIVE_TIMEOUT_SCRIPT = """#!/usr/bin/env spaces
load("//@star/prelude/exec/signal.star", "signal_wait")
signal_wait(timeout_ms = -1)
"""

# ============================================================================
# Execute and assert
# ============================================================================

timeout_state = run_signal_child(TIMEOUT_STATE_SCRIPT)
record("timeout_and_state", "script_exits_zero", timeout_state.get("status") == 0)
record("timeout_and_state", "pending_returns_list", stdout_contains(timeout_state, "PENDING_TYPE=list"))
record("timeout_and_state", "queue_empty_after_clear_dispatch", stdout_contains(timeout_state, "PENDING_LEN=0"))
record("timeout_and_state", "wait_timeout_returns_none", stdout_contains(timeout_state, "WAIT_TIMEOUT_IS_NONE=True"))

trap_replace_wait = run_signal_child(TRAP_REPLACE_WAIT_SCRIPT)
record("trap_and_wait", "script_exits_zero", trap_replace_wait.get("status") == 0)
record("trap_and_wait", "send_status_zero", stdout_contains(trap_replace_wait, "SEND_STATUS=0"))
record("trap_and_wait", "wait_returns_usr1", stdout_contains(trap_replace_wait, "OBSERVED=USR1"))
record("trap_and_wait", "handler_printed_usr1", stdout_contains(trap_replace_wait, "USR1"))
record("trap_and_wait", "queue_empty_after_wait", stdout_contains(trap_replace_wait, "PENDING_AFTER=0"))

pending_dispatch = run_signal_child(PENDING_DISPATCH_SCRIPT)
record("pending_and_dispatch", "script_exits_zero", pending_dispatch.get("status") == 0)
record("pending_and_dispatch", "send_status_zero", stdout_contains(pending_dispatch, "SEND_STATUS=0"))
record("pending_and_dispatch", "pending_includes_usr2", stdout_contains(pending_dispatch, "PENDING_HAS_USR2=True"))
record("pending_and_dispatch", "pending_entries_are_strings", stdout_contains(pending_dispatch, "PENDING_ALL_STRINGS=True"))
record("pending_and_dispatch", "dispatch_invokes_one_handler", stdout_contains(pending_dispatch, "DISPATCHED=1"))
record("pending_and_dispatch", "queue_empty_after_dispatch", stdout_contains(pending_dispatch, "PENDING_AFTER=0"))

multi_signal = run_signal_child(MULTI_SIGNAL_WRAPPERS_SCRIPT)
record("multi_signal_wrappers", "script_exits_zero", multi_signal.get("status") == 0)
record("multi_signal_wrappers", "send_usr1_status_zero", stdout_contains(multi_signal, "SEND1=0"))
record("multi_signal_wrappers", "send_usr2_status_zero", stdout_contains(multi_signal, "SEND2=0"))
record("multi_signal_wrappers", "wait_usr1", stdout_contains(multi_signal, "OBS1=USR1"))
record("multi_signal_wrappers", "wait_usr2", stdout_contains(multi_signal, "OBS2=USR2"))
record("multi_signal_wrappers", "handler_printed_usr1", stdout_contains(multi_signal, "USR1"))
record("multi_signal_wrappers", "handler_printed_usr2", stdout_contains(multi_signal, "USR2"))
record("multi_signal_wrappers", "queue_empty_after_waits", stdout_contains(multi_signal, "PENDING_AFTER=0"))

invalid_type = run_signal_child(INVALID_TRAP_TYPE_SCRIPT)
record("error_paths", "trap_rejects_non_string_or_list", invalid_type.get("status") != 0)
record("error_paths", "trap_type_error_message", output_contains(invalid_type, "signal_names"))

invalid_list_item = run_signal_child(INVALID_TRAP_LIST_ITEM_SCRIPT)
record("error_paths", "trap_rejects_non_string_list_item", invalid_list_item.get("status") != 0)
record("error_paths", "trap_list_item_error_message", output_contains(invalid_list_item, "signal_names"))

invalid_signal_name = run_signal_child(INVALID_SIGNAL_NAME_SCRIPT)
record("error_paths", "trap_rejects_unsupported_signal_name", invalid_signal_name.get("status") != 0)
record("error_paths", "trap_unsupported_signal_message", output_contains(invalid_signal_name, "Unsupported signal"))

negative_timeout = run_signal_child(NEGATIVE_TIMEOUT_SCRIPT)
record("error_paths", "wait_rejects_negative_timeout", negative_timeout.get("status") != 0)
record("error_paths", "wait_negative_timeout_message", output_contains(negative_timeout, "timeout_ms must be non-negative"))

# Cleanup temporary child scripts created by tmp_file().
tmp_cleanup_all()

print("Signal Module Test Results:")
print("===========================")
print("")
print(json_dumps(signal_results, is_pretty = True))
print("")
print("All signal functions executed successfully!")
