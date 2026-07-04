#!/usr/bin/env spaces

load("//@star/prelude/exec/sh.star", "sh_capture")

sh_capture("echo sh failed 1>&2; exit 3", check = True, cwd = "/tmp")
