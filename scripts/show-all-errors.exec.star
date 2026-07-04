#!/usr/bin/env spaces

"""Run all scripts in spaces/scripts/errors and collect their visible output."""

load("//@star/prelude/exec/args.star", "args_program")
load(
    "//@star/prelude/exec/fs.star",
    "fs_read_directory",
    "fs_write_text",
)
load(
    "//@star/prelude/exec/path.star",
    "path_basename",
    "path_dirname",
    "path_join",
)
load(
    "//@star/prelude/exec/process.star",
    "process_options",
    "process_run",
    "process_stderr_capture",
    "process_stdout_capture",
)
load("//@star/prelude/exec/sys.star", "sys_executable")

script_dir = path_dirname(args_program())
errors_dir = path_join([script_dir, "errors"])
output_path = "all-errors-output.txt"
spaces_bin = sys_executable()

entries = fs_read_directory(errors_dir)
error_scripts = []
for entry in entries:
    name = path_basename(entry)
    if name.endswith(".exec.star"):
        error_scripts.append(entry)

error_scripts = sorted(error_scripts)

def join_streams(stdout_text, stderr_text):
    if stdout_text == "":
        return stderr_text
    if stderr_text == "":
        return stdout_text
    if stdout_text.endswith("\n"):
        return stdout_text + stderr_text
    return stdout_text + "\n" + stderr_text

report_lines = [
    "spaces error showcase",
    "====================",
    "spaces executable: {}".format(spaces_bin),
    "errors directory: {}".format(errors_dir),
    "script count: {}".format(len(error_scripts)),
    "",
]

for script in error_scripts:
    name = path_basename(script)
    result = process_run(process_options(
        command = spaces_bin,
        args = [script],
        stdout = process_stdout_capture(),
        stderr = process_stderr_capture(),
    ))

    status = result.get("status", -1)
    stdout_text = result.get("stdout", "")
    stderr_text = result.get("stderr", "")
    combined = join_streams(stdout_text, stderr_text).strip()

    report_lines.append("================================================================")
    report_lines.append("script: {}".format(name))
    report_lines.append("path: {}".format(script))
    report_lines.append("status: {}".format(status))
    report_lines.append("----------------------------------------------------------------")
    if combined == "":
        report_lines.append("<no output captured>")
    else:
        report_lines.append(combined)
    report_lines.append("")

fs_write_text(output_path, "\n".join(report_lines))

print("Wrote {} error outputs to {}".format(len(error_scripts), output_path))
