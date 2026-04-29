#!/usr/bin/env spaces

"""Tests for the @star/sdk/star/std/args.star module."""

load(
    "//@star/sdk/star/std/args.star",
    "args_argv",
    "args_flag",
    "args_list",
    "args_opt",
    "args_parse",
    "args_parser",
    "args_pos",
    "args_program",
)

def assert_eq(actual, expected, label):
    """Fails with a descriptive message if actual != expected."""
    if actual != expected:
        fail("FAIL [{}]: expected {!r}, got {!r}".format(label, expected, actual))

def assert_true(condition, label):
    """Fails if condition is not truthy."""
    if not condition:
        log.error("FAIL [{}]: expected truthy, got {!r}".format(label, condition))
        sys.exit(1)

# ============================================================================
# args_argv / args_program
# ============================================================================

argv = args_argv()
assert_true(type(argv) == "list", "argv is a list")
assert_true(len(argv) >= 1, "argv has at least one element (program name)")

prog = args_program()
assert_true(type(prog) == "string", "program is a string")
assert_true(len(prog) > 0, "program name is non-empty")
if len(argv) >= 1:
    assert_eq(prog, argv[0], "program() == argv[0]")

# ============================================================================
# args_flag descriptor
# ============================================================================

flag_no_short = args_flag("--verbose", help = "Enable verbose output")
assert_eq(flag_no_short.get("kind"), "flag", "flag kind")
assert_eq(flag_no_short.get("long"), "--verbose", "flag long")
assert_eq(flag_no_short.get("short"), None, "flag no short")
assert_eq(flag_no_short.get("help"), "Enable verbose output", "flag help")
assert_eq(flag_no_short.get("default"), False, "flag default False")

flag_with_short = args_flag("--dry-run", short = "-d")
assert_eq(flag_with_short.get("short"), "-d", "flag short -d")
assert_eq(flag_with_short.get("default"), False, "flag with short default False")

# ============================================================================
# args_opt descriptor
# ============================================================================

opt_str = args_opt("--env", short = "-e", default = "dev", choices = ["dev", "stg", "prod"])
assert_eq(opt_str.get("kind"), "opt", "opt kind")
assert_eq(opt_str.get("long"), "--env", "opt long")
assert_eq(opt_str.get("short"), "-e", "opt short")
assert_eq(opt_str.get("default"), "dev", "opt str default")
assert_eq(opt_str.get("choices"), ["dev", "stg", "prod"], "opt choices")
assert_eq(opt_str.get("type"), "str", "opt type str")

opt_int = args_opt("--count", type = "int", default = 42)
assert_eq(opt_int.get("kind"), "opt", "int opt kind")
assert_eq(opt_int.get("default"), 42, "int opt default 42")
assert_eq(opt_int.get("type"), "int", "int opt type")

opt_int_no_default = args_opt("--retries", type = "int")
assert_eq(opt_int_no_default.get("default"), 0, "int opt implicit default 0")

opt_bool = args_opt("--flag", type = "bool")
assert_eq(opt_bool.get("default"), False, "bool opt implicit default False")

opt_str_no_default = args_opt("--output")
assert_eq(opt_str_no_default.get("default"), "", "str opt implicit default empty string")

# ============================================================================
# args_list descriptor
# ============================================================================

lst = args_list("--tag", short = "-t", help = "Add a tag")
assert_eq(lst.get("kind"), "list", "list kind")
assert_eq(lst.get("long"), "--tag", "list long")
assert_eq(lst.get("short"), "-t", "list short")
assert_eq(lst.get("help"), "Add a tag", "list help")
assert_eq(lst.get("default"), [], "list default empty")
assert_eq(lst.get("type"), "str", "list type default str")

lst_int = args_list("--port", type = "int")
assert_eq(lst_int.get("type"), "int", "list int type")
assert_eq(lst_int.get("default"), [], "list int default empty")

# ============================================================================
# args_pos descriptor
# ============================================================================

pos_required = args_pos("service", required = True)
assert_eq(pos_required.get("name"), "service", "pos name")
assert_eq(pos_required.get("required"), True, "pos required")
assert_eq(pos_required.get("variadic"), False, "pos not variadic")

pos_variadic = args_pos("targets", variadic = True)
assert_eq(pos_variadic.get("variadic"), True, "pos variadic")
assert_eq(pos_variadic.get("required"), False, "pos variadic not required by default")

pos_optional = args_pos("output")
assert_eq(pos_optional.get("required"), False, "pos optional not required")
assert_eq(pos_optional.get("variadic"), False, "pos optional not variadic")

# ============================================================================
# Key normalisation (--dry-run maps to dry_run, --max-retries to max_retries)
# ============================================================================

spec_keys = args_parser(
    name = "normalise-test",
    description = "",
    options = [
        args_flag("--dry-run"),
        args_opt("--max-retries", type = "int", default = 3),
        args_list("--output-file"),
    ],
)

# Parse with empty argv: all defaults should apply
result_keys = args_parse(spec_keys)
assert_eq(result_keys.get("dry_run"), False, "dry_run default False")
assert_eq(result_keys.get("max_retries"), 3, "max_retries default 3")
assert_eq(result_keys.get("output_file"), [], "output_file default []")

# ============================================================================
# Full parse with all option types (empty argv -> all defaults)
# ============================================================================

spec_full = args_parser(
    name = "test",
    description = "Args module test",
    options = [
        args_flag("--verbose", short = "-v"),
        args_opt("--env", short = "-e", default = "dev", choices = ["dev", "stg", "prod"]),
        args_opt("--timeout", type = "int", default = 30),
        args_opt("--debug", type = "bool"),
        args_list("--tag", short = "-t"),
    ],
    positional = [
        args_pos("output"),  # optional, non-variadic -> None when absent
        args_pos("extras", variadic = True),  # variadic -> [] when absent
    ],
)

result = args_parse(spec_full)

# All flags/opts default correctly
assert_eq(result.get("verbose"), False, "verbose default False")
assert_eq(result.get("env"), "dev", "env default dev")
assert_eq(result.get("timeout"), 30, "timeout default 30")
assert_eq(result.get("debug"), False, "debug default False")
assert_eq(result.get("tag"), [], "tag default []")

# Optional positionals
assert_eq(result.get("output"), None, "optional positional output is None when absent")
assert_eq(result.get("extras"), [], "variadic positional extras is [] when absent")

print("All args module tests passed.")
