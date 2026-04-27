#!/usr/bin/env spaces

load("//@star/sdk/star/std/env.star", "env_get")
load(
    "//@star/sdk/star/std/process.star",
    "process_options",
    "process_pipeline",
    "process_run",
    "process_stdout_file",
)

ENV = {
    "PATH": "{}/sysroot/bin".format(env_get("PWD")),
}

run_rg = process_options(
    command = "rg",
    args = [
        "--iglob=**/*.exec.star",
        "--line-number",
        "log",
    ],
    env = ENV,
)

run_bat = process_options(
    command = "bat",
    args = ["--paging=never"],
    stdout = process_stdout_file("bat_out.txt"),
    stderr = "inherit",
    env = ENV,
)

rg_status = process_pipeline(
    [run_rg, run_bat],
)
