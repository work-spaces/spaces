#!/usr/bin/env spaces

load(
    "//@star/prelude/exec/process.star",
    "process_options",
    "process_run",
    "process_stderr_capture",
    "process_stdout_capture",
)

process_run(process_options(
    "sh",
    args = ["-c", "echo process failed >&2; exit 7"],
    cwd = "/tmp",
    stdout = process_stdout_capture(),
    stderr = process_stderr_capture(),
    check = True,
))
