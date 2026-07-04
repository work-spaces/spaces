#!/usr/bin/env spaces

load("//@star/prelude/exec/time.star", "time_parse_datetime")

time_parse_datetime("not-a-date", "%Y-%m-%d")
