#!/usr/bin/env spaces

load("//@star/prelude/exec/toml.star", "toml_encode")

toml_encode({"x": None})
