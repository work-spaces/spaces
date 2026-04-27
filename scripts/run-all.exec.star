#!/usr/bin/env spaces

load("//@star/sdk/star/std/args.star", "args_program")
load("//@star/sdk/star/std/env.star", "env_cwd")
load("//@star/sdk/star/std/fs.star", "fs_read_directory")
load("//@star/sdk/star/std/log.star", "log_error", "log_info")
load("//@star/sdk/star/std/path.star", "path_dirname", "path_join")
load(
    "//@star/sdk/star/std/process.star",
    "process_options",
    "process_run",
    "process_stderr_capture",
    "process_stdout_capture",
)
load("//@star/sdk/star/std/sys.star", "sys_exit")

tests = fs_read_directory(
    path_join([
        path_dirname(args_program()),
        "test",
    ]),
)

log_info("Running {} tests...".format(len(tests)))

for test in tests:
    log_info("Running {}".format(test))
    options = process_options(
        command = test,
        stdout = process_stdout_capture(),
        stderr = process_stdout_capture(),
    )
    result = process_run(options)
    status = result.get("status")
    if status != 0:
        log_error("{} => {}".format(test, status))
        print("=====================")
        print(result.get("stderr"))
        print("=====================")
        sys_exit(status)
