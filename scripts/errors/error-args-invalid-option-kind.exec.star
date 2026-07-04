#!/usr/bin/env spaces

load("//@star/prelude/exec/args.star", "args_parse")

args_parse({
    "name": "error-args-invalid-option-kind",
    "description": "Triggers args.rs invalid option kind error",
    "options": [
        {"kind": "nope", "long": "--bad"},
    ],
})
