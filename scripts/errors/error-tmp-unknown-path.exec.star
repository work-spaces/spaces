#!/usr/bin/env spaces

load("//@star/prelude/exec/tmp.star", "tmp_cleanup")

tmp_cleanup("/tmp/not-tracked-by-tmp-module")
