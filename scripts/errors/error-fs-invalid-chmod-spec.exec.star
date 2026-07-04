#!/usr/bin/env spaces

load("//@star/prelude/exec/fs.star", "fs_chmod")

fs_chmod("spaces/scripts/test/test-args.exec.star", "bad")
