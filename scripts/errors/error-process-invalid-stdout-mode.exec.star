#!/usr/bin/env spaces

load("//@star/prelude/exec/process.star", "process_run")

process_run({"command": "echo", "args": ["hi"], "stdout": "bad-mode"})
