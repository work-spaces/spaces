#!/usr/bin/env spaces

load(
    "//@star/prelude/exec/args.star",
    "args_opt",
    "args_parse",
    "args_parser",
    "args_program",
)
load("//@star/prelude/exec/fs.star", "fs_read_directory")
load("//@star/prelude/exec/log.star", "log_error", "log_info")
load("//@star/prelude/exec/path.star", "path_dirname", "path_join")
load(
    "//@star/prelude/exec/process.star",
    "process_options",
    "process_run",
    "process_stderr_capture",
    "process_stdout_capture",
)
load("//@star/prelude/exec/sys.star", "sys_exit")

parser = args_parser(
    name = "run-all",
    description = "Run all tests",
    options = [
        args_opt(
            name = "spaces",
            help = "Path to the spaces executable",
        ),
    ],
)
args = args_parse(parser)

tests = fs_read_directory(
    path_join([
        path_dirname(args_program()),
        "test",
    ]),
)

# Try to get spaces from args, otherwise use env_which to find it
spaces_program = args.get("spaces")
assert_on(spaces_program != "", "spaces executable not specified")

log_info("Using spaces executable: {}".format(spaces_program))
log_info("Running {} tests...".format(len(tests)))

for test in tests:
    log_info("Running {}".format(test))
    options = process_options(
        command = spaces_program,
        args = [test],
        stdout = process_stdout_capture(),
        stderr = process_stderr_capture(),
    )
    result = process_run(options)
    status = result.get("status", 1)
    if status != 0:
        log_error("{} => {}".format(test, status))
        print("=====================")
        print(result.get("stderr"))
        print("=====================")
        sys_exit(status)
