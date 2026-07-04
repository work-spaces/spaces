#!/usr/bin/env spaces

load("//@star/prelude/exec/toml.star", "toml_encode")

if "spaces" in sys.executable():
    toml_encode({"x": None})
