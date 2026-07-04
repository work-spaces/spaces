#!/usr/bin/env spaces

load("//@star/prelude/exec/fs.star", "fs_remove")

fs_remove("./_definitely_missing_path_for_error_script", missing_ok = False)
