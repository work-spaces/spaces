"""
Command-line argument parsing and program configuration.

This module provides ergonomic access to command-line arguments and tools for building
sophisticated argument parsers. It supports flags, options, positional arguments, and
automatic usage/help generation.

Core Concepts:
    - argv: The raw command-line arguments
    - flags: Boolean options like --dry-run or -d
    - options: Single-value options like --env=prod or -e prod
    - lists: Repeatable options like --tag=foo --tag=bar
    - positional: Required or variadic positional arguments
    - parser: A specification defining the full argument structure
    - parse: Runs the parser against argv with automatic help/error handling

Example:
    # Simple flag and option parsing
    dry_run = args_flag("--dry-run", short="-d", help="Preview only")
    env_opt = args_opt("--env", short="-e", default="dev", choices=["dev", "stg", "prod"])

    # Define positional arguments
    service = args_pos("service", required=True)
    targets = args_pos("targets", variadic=True)

    # Create a parser and parse argv
    spec = args_parser(
        name="deploy",
        description="Deploy a service to the specified targets",
        options=[dry_run, env_opt],
        positional=[service, targets]
    )

    parsed = args_parse(spec)

    # Access parsed values
    if parsed["dry_run"]:
        print("Dry run mode enabled")
    print("Environment:", parsed["env"])
    print("Service:", parsed["service"])
    print("Targets:", parsed["targets"])
"""

def args_argv() -> list[str]:
    """
    Returns the full command-line arguments (argv) passed to the script.

    This returns the complete argv list including argv[0] (the program name).
    Use args_program() to get just the program name, or args_argv()[1:] to get
    arguments without the program name.

    Returns:
        A list of strings representing command-line arguments.
        First element is typically the program name, followed by arguments.
        Empty list if no arguments provided.

    Examples:
        >>> # If script called as: python script.py --verbose file.txt
        >>> args_argv()  # -> ["script.py", "--verbose", "file.txt"]
        >>> len(args_argv()) > 0  # -> True
        >>> # Get program name and arguments separately
        >>> prog = args_argv()[0] if args_argv() else ""
        >>> rest = args_argv()[1:] if len(args_argv()) > 1 else []
        >>> # Pass argv to other tools
        >>> for arg in args_argv():
        ...     print(f"Argument: {arg}")
    """
    return args.argv()

def args_program() -> str:
    """
    Returns the program name (argv[0]).

    Returns the first element of argv, which is typically the name of the script
    or program being executed. Returns empty string if argv is empty (rare edge case).

    Returns:
        The program name as a string. Usually ends with ".star" for Starlark scripts.
        Returns empty string if no program name available.

    Examples:
        >>> prog = args_program()
        >>> len(prog) > 0  # -> True
        >>> prog  # -> "deploy.star" (or similar)
        >>> # Use for logging or error messages
        >>> print(f"Usage: {args_program()} [OPTIONS] SERVICE")
        >>> # Build help text
        >>> help_prefix = f"{args_program()}: "
    """
    return args.program()

def _validate_short(name: str) -> None:
    """Validates that name is a valid short option format."""
    if not name.startswith("-") or name.startswith("--") or len(name) != 2:
        fail("Short option name must be like `-x`, got `{}`".format(name))

def _validate_type(type_str: str) -> None:
    """Validates that type is one of the allowed values."""
    if type_str not in ("str", "int", "bool"):
        fail("`type` must be one of: str, int, bool (got `{}`)".format(type_str))

def _default_for_opt(type_str: str, user_default = None):
    """Returns the default value for an option based on type.

    Validates that user_default, if supplied, matches the declared type.
    """
    if user_default != None:
        if type_str == "int" and type(user_default) != "int":
            fail("Default for `int` option must be an integer, got: {}".format(type(user_default)))
        if type_str == "bool" and type(user_default) != "bool":
            fail("Default for `bool` option must be a boolean, got: {}".format(type(user_default)))
        if type_str == "str" and type(user_default) != "string":
            fail("Default for `str` option must be a string, got: {}".format(type(user_default)))
        return user_default
    if type_str == "int":
        return 0
    if type_str == "bool":
        return False
    return ""

def args_flag(
        name: str,
        short: str | None = None,
        help: str | None = None) -> dict:
    """
    Creates a boolean flag option descriptor.

    Flags are boolean options that are either present or absent. They don't take values.
    When present on the command line, the flag is set to True. When absent, it defaults to False.

    Args:
        name: The long flag name, must start with "--" and contain only lowercase letters, digits, and hyphens.
              Examples: "--verbose", "--dry-run", "--force"
        short: Optional single-character short flag, must start with "-" if provided.
               Examples: "-v", "-d", "-f". If empty, no short flag is available.
        help: Optional help text describing what the flag does. Will be shown in usage output.
              Keep descriptions concise and clear.

    Returns:
        A flag descriptor (dictionary) to be used in parser specs.
        Internal structure: {"kind": "flag", "long": name, "short": short_value, "help": help_text, "default": False}

    Raises:
        Error: If name doesn't start with "--" or contains invalid characters
        Error: If short is provided but doesn't start with "-"
        Error: If short is not a single character after the dash

    Examples:
        >>> verbose_flag = args_flag("--verbose", short="-v", help="Enable verbose output")
        >>> dry_run_flag = args_flag("--dry-run", help="Preview changes without applying")
        >>> debug_flag = args_flag("--debug")  # Help text is optional
        >>> # Use in parser spec
        >>> spec = args_parser(options=[verbose_flag, dry_run_flag])
        >>> parsed = args_parse(spec)
        >>> if parsed["verbose"]:
        ...     print("Verbose mode enabled")
    """
    if short != None:
        _validate_short(short)

    return {
        "kind": "flag",
        "long": name,
        "short": short,
        "help": help,
        "default": False,
    }

def args_opt(
        name: str,
        short: str | None = None,
        help: str | None = None,
        default = None,
        choices = None,
        type: str = "str") -> dict:
    """
    Creates a single-value option descriptor.

    Options take a value that can be passed on the command line.
    Each option can appear at most once (unlike lists which can repeat).
    Supports different value types and optional constraints like choice restrictions.

    Args:
        name: The long option name, must start with "--" and contain only lowercase letters, digits, and hyphens.
              Examples: "--env", "--output", "--config-file"
        short: Optional single-character short option. Examples: "-e", "-o", "-c"
        help: Optional help text describing the option. Shown in usage output.
        default: Default value if the option is not provided on command line.
                 Type depends on the `type` parameter. If None, uses type-specific default.
        choices: Optional list of allowed values. If provided, only these values are accepted.
                 Example: ["dev", "staging", "prod"]. If empty list or None, any value is allowed.
        type: The value type: "str" (default), "int", or "bool".
              Determines how the value is parsed and what default is used if not provided.
              When type="bool", the command-line value must be one of: true/1/yes/on (True) or false/0/no/off (False).

    Returns:
        An option descriptor (dictionary) for use in parser specs.
        Internal structure includes all parameters for option validation and parsing.

    Raises:
        Error: If name doesn't start with "--" or contains invalid characters
        Error: If short is provided but doesn't start with "-"
        Error: If type is not one of "str", "int", "bool"
        Error: If default value doesn't match the specified type

    Examples:
        >>> env_opt = args_opt("--env", short="-e", default="dev", choices=["dev", "stg", "prod"])
        >>> output_opt = args_opt("--output", short="-o", help="Output file path")
        >>> count_opt = args_opt("--count", type="int", default=1, help="Number of items")
        >>> debug_opt = args_opt("--debug", type="bool", default=False)
        >>> # Use in parser
        >>> spec = args_parser(options=[env_opt, output_opt, count_opt])
        >>> parsed = args_parse(spec)
        >>> print(f"Env: {parsed['env']}, Output: {parsed['output']}")
    """

    if short != None:
        _validate_short(short)
    _validate_type(type)

    default_value = _default_for_opt(type, default)

    spec = {
        "kind": "opt",
        "long": name,
        "short": short,
        "help": help,
        "default": default_value,
        "choices": choices,
        "type": type,
    }
    return spec

def args_list(
        name: str,
        short: str | None = None,
        help: str | None = None,
        choices = None,
        type: str = "str") -> dict:
    """
    Creates a repeatable option descriptor.

    Lists are options that can appear multiple times on the command line.
    Each occurrence adds a value to the resulting list.
    Useful for collecting multiple values like multiple tags, files, or includes.

    Args:
        name: The long option name, must start with "--" and contain only lowercase letters, digits, and hyphens.
              Examples: "--tag", "--include", "--file"
        short: Optional single-character short option. Examples: "-t", "-i", "-f"
        help: Optional help text. Suggestion: mention that option is repeatable.
              Example: "Include a tag (can be used multiple times)"
        choices: Optional list of allowed values. Each occurrence must use an allowed choice.
                 Note: choices are only supported when type="str" (the default); combining choices with type="int" or type="bool" raises an error.
        type: The value type: "str" (default), "int", or "bool".
              Determines how each value is parsed.

    Returns:
        A list descriptor (dictionary) for use in parser specs.
        Default value is always an empty list [].

    Raises:
        Error: If name doesn't start with "--" or contains invalid characters
        Error: If short is provided but doesn't start with "-"
        Error: If type is not one of "str", "int", "bool"

    Examples:
        >>> tag_opt = args_list("--tag", short="-t", help="Add a tag (repeat for multiple)")
        >>> include_opt = args_list("--include", short="-I", help="Include directory")
        >>> exclude_opt = args_list("--exclude", type="str")
        >>> # Use in parser
        >>> spec = args_parser(options=[tag_opt, include_opt])
        >>> parsed = args_parse(spec)
        >>> print(f"Tags: {parsed['tag']}")  # -> ["prod", "critical"] if called with --tag prod --tag critical
        >>> for tag in parsed["tag"]:
        ...     print(f"Processing tag: {tag}")
    """
    if short != None:
        _validate_short(short)
    _validate_type(type)

    spec = {
        "kind": "list",
        "long": name,
        "short": short,
        "help": help,
        "default": [],
        "choices": choices,
        "type": type,
    }
    return spec

def args_pos(name: str, required: bool = False, variadic: bool = False) -> dict:
    """
    Creates a positional argument descriptor.

    Positional arguments are values provided on the command line after all options.
    They're matched by position, not by flag name.
    Examples: in "command source dest", "source" and "dest" are positional.

    Args:
        name: The name of the positional argument. Used in parsed results and help text.
              Examples: "source", "destination", "targets", "pattern"
        required: If True, the argument must be provided or parsing fails.
                  Default False allows the argument to be optional.
                  When False and the argument is not provided, its value in the parsed result is None.
        variadic: If True, this positional consumes all remaining arguments (like *args).
                  Only one positional can be variadic, and it must be the last one.
                  Default False means the positional takes exactly one value.

    Returns:
        A positional descriptor (dictionary) for use in parser specs.
        Internal structure: {"name": name, "required": required, "variadic": variadic}

    Raises:
        Error: If name is empty or contains only whitespace

    Examples:
        >>> # Single required positional
        >>> service_pos = args_pos("service", required=True)
        >>> # Optional positional
        >>> target_pos = args_pos("target")
        >>> # Variadic positional (consumes remaining arguments)
        >>> files_pos = args_pos("files", variadic=True)
        >>> # Use in parser (positional order matters)
        >>> spec = args_parser(
        ...     name="deploy",
        ...     description="Deploy a service to one or more targets",
        ...     positional=[service_pos, files_pos]
        ... )
        >>> parsed = args_parse(spec)
        >>> print(f"Service: {parsed['service']}")
        >>> print(f"Files: {parsed['files']}")  # -> list of remaining arguments
    """
    if not name or not name.strip():
        fail("Positional name cannot be empty")

    spec = {
        "name": name,
        "required": required,
        "variadic": variadic,
    }
    return spec

def args_parser(
        name: str,
        description: str,
        options: list[dict] | None = None,
        positional: list[dict] | None = None) -> dict:
    """
    Creates a parser specification.

    A parser specification defines the complete structure of command-line arguments,
    including all supported options and positional arguments. Use with args_parse()
    to actually parse command-line arguments.

    Args:
        name: Name of the program/command. Used in help text.
        description: Description of what the program does. Shown at top of help text.
        options: List of option descriptors created with args_flag(), args_opt(), or args_list().
                 Default is empty list (no options).
        positional: List of positional descriptors created with args_pos().
                    Default is empty list (no positional arguments).
                    Variadic positional must come last.

    Returns:
        A parser specification (dictionary) to be passed to args_parse().

    Examples:
        >>> dry_run = args_flag("--dry-run", short="-d", help="Preview only")
        >>> env = args_opt("--env", default="dev", choices=["dev", "stg", "prod"])
        >>> service = args_pos("service", required=True)
        >>> targets = args_pos("targets", variadic=True)
        >>> spec = args_parser(
        ...     name="deploy",
        ...     description="Deploy a service to targets",
        ...     options=[dry_run, env],
        ...     positional=[service, targets]
        ... )
        >>> # Then parse
        >>> parsed = args_parse(spec)
        >>> print(parsed)
    """
    return args.parser({
        "name": name,
        "description": description,
        "options": options,
        "positional": positional,
    })

def args_parse(spec: dict) -> dict:
    """
    Parses command-line arguments according to a parser specification.

    This is the main function that performs argument parsing. It processes argv[1:]
    according to the parser spec, validating values and assigning defaults.

    Special behavior:
        - If --help or -h is encountered, prints usage and exits with code 0
        - If parsing fails (missing required args, invalid values, etc.),
          prints usage message and error details, then exits with code 2
        - Options and positionals are collected into a result dictionary

    Args:
        spec: A parser specification created with args_parser().

    Returns:
        A dictionary mapping argument names to their parsed values.
        - Flag names map to boolean values (True/False)
        - Option names map to their values (as strings, ints, or bools per type)
        - List option names map to lists of values
        - Positional names map to their values or lists (if variadic)

    Raises:
        Error: This function handles errors by printing and exiting rather than raising exceptions.
               Help requests trigger exit 0, parse errors trigger exit 2.

    Examples:
        >>> # Build a parser spec
        >>> spec = args_parser(
        ...     options=[
        ...         args_flag("--verbose", short="-v"),
        ...         args_opt("--config", short="-c", default="config.yaml")
        ...     ],
        ...     positional=[args_pos("input", required=True)]
        ... )
        >>> # Parse and use the results
        >>> parsed = args_parse(spec)
        >>> if parsed["verbose"]:
        ...     print(f"Using config: {parsed['config']}")
        ...     print(f"Processing: {parsed['input']}")
        >>>
        >>> # Accessing list arguments
        >>> spec = args_parser(
        ...     options=[
        ...         args_list("--tag", short="-t", help="Add a tag")
        ...     ]
        ... )
        >>> parsed = args_parse(spec)
        >>> for tag in parsed["tag"]:
        ...     print(f"Tag: {tag}")
    """
    return args.parse(spec)
