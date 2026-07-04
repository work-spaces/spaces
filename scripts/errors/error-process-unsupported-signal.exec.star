#!/usr/bin/env spaces

load(
    "//@star/prelude/exec/process.star",
    "process_kill",
    "process_options",
    "process_spawn",
)

h = process_spawn(process_options("sleep", args = ["1"]))
process_kill(h, "SIGUSR1")
