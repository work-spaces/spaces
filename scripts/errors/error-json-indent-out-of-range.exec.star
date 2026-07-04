#!/usr/bin/env spaces

load("//@star/prelude/exec/json.star", "json_encode_indented")

json_encode_indented({"ok": True}, indent = 99)
