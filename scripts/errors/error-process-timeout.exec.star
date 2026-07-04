#!/usr/bin/env spaces

load("//@star/prelude/exec/process.star", "process_options", "process_run")

process_run(process_options("sleep", args = ["1"], timeout_ms = 10))
