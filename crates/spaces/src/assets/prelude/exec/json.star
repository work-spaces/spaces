"""
Spaces JSON module - Ergonomic wrappers for JSON serialization, deserialization,
and file I/O

This module provides clean, well-documented functions for working with JSON data
in Starlark scripts. All functions handle errors gracefully and provide clear
feedback when something goes wrong.
"""

# ============================================================================
# Original Functions - Maintained for Backwards Compatibility
# ============================================================================

def json_loads(value: str):
    """
    Load a JSON string

    Args:
        value: The JSON string to load

    Returns:
        The parsed JSON object
    """
    return json.string_to_dict(value)

def json_dumps(value, is_pretty: bool = False):
    """
    Dump a JSON object to a string

    Args:
        value: The JSON object to dump
        is_pretty: Whether to pretty print the JSON

    Returns:
        The JSON string
    """
    if is_pretty:
        return json.to_string_pretty(value)
    else:
        return json.to_string(value)

def json_is_string_json(value: str) -> bool:
    """
    Check if a string is a JSON object

    Args:
        value: The string to check

    Returns:
        True if the string is a JSON object, False otherwise
    """
    return json.is_string_json(value)

# ============================================================================
# New Ergonomic Functions
# ============================================================================

def json_decode(json_string: str):
    """
    Parse a JSON string into a Starlark value.

    This function takes a JSON-formatted string and converts it into a Starlark
    value (dictionary, list, string, number, boolean, or None). The JSON must be
    valid or this function will raise an error.

    Args:
        json_string: A valid JSON-formatted string to parse

    Returns:
        A Starlark value representing the parsed JSON data (dict, list, string, number, bool, or None)

    Raises:
        Raises an error if the JSON string is malformed or invalid

    Examples:
        Parse a simple JSON object:
        ```starlark
        user = json_decode('{"name": "Alice", "age": 30}')
        print(user["name"])  # Output: Alice
        ```

        Parse a JSON array:
        ```starlark
        items = json_decode('[1, 2, 3, 4, 5]')
        print(items[0])  # Output: 1
        ```

        Parse nested JSON structures:
        ```starlark
        config = json_decode('''
        {
            "database": {
                "host": "localhost",
                "port": 5432
            },
            "debug": true
        }
        ''')
        print(config["database"]["host"])  # Output: localhost
        ```
    """
    return json.string_to_dict(json_string)

def json_encode(value, pretty: bool = False):
    """
    Convert a dictionary or value into a JSON string.

    This function serializes Starlark dictionaries and other values into
    JSON-formatted strings suitable for output, file storage, or network
    transmission. By default, the output is compact. Use pretty=True for
    human-readable formatting with indentation.

    Args:
        value: The dictionary or Starlark value to encode as JSON
        pretty: If True, returns formatted JSON with indentation and newlines.
                If False (default), returns compact JSON.

    Returns:
        A JSON-formatted string representation of the input value

    Raises:
        Raises an error if the value cannot be serialized to JSON

    Examples:
        Encode a simple dictionary:
        ```starlark
        data = {"name": "Bob", "active": True}
        json_str = json_encode(data)
        print(json_str)  # Output: {"name":"Bob","active":true}
        ```

        Encode with pretty formatting:
        ```starlark
        config = {
            "server": "api.example.com",
            "port": 8080,
            "features": ["auth", "logging"]
        }
        pretty_json = json_encode(config, pretty=True)
        print(pretty_json)
        # Output:
        # {
        #   "server": "api.example.com",
        #   "port": 8080,
        #   "features": [
        #     "auth",
        #     "logging"
        #   ]
        # }
        ```

        Encode nested structures:
        ```starlark
        project = {
            "name": "MyProject",
            "version": "1.0.0",
            "metadata": {
                "author": "Dev Team",
                "created": "2024-01-15"
            }
        }
        json_str = json_encode(project, pretty=True)
        ```
    """
    if pretty:
        return json.to_string_pretty(value)
    else:
        return json.to_string(value)

def json_encode_compact(value):
    """
    Convert a value into a compact JSON string without whitespace.

    This is a convenience function for encoding with minimal whitespace,
    useful for efficient network transmission or compact file storage.
    Equivalent to calling json_encode(value, pretty=False).

    Args:
        value: The dictionary or Starlark value to encode

    Returns:
        A compact JSON string with no extra whitespace

    Examples:
        ```starlark
        data = {"x": 1, "y": 2, "z": 3}
        compact = json_encode_compact(data)
        print(compact)  # Output: {"x":1,"y":2,"z":3}
        ```
    """
    return json.to_string(value)

def json_encode_pretty(value):
    """
    Convert a value into a formatted JSON string with indentation.

    This is a convenience function for creating human-readable JSON output
    with proper indentation and newlines. Equivalent to calling
    json_encode(value, pretty=True).  Always uses 2-space indentation; use
    `json_encode_indented` if you need a different width.

    Args:
        value: The dictionary or Starlark value to encode

    Returns:
        A formatted JSON string with 2-space indentation and newlines

    Examples:
        ```starlark
        app_config = {
            "name": "MyApp",
            "settings": {
                "debug": False,
                "timeout": 30
            },
            "plugins": ["auth", "cache"]
        }
        print(json_encode_pretty(app_config))
        ```
    """
    return json.to_string_pretty(value)

def json_encode_indented(value, indent: int = 2):
    """
    Convert a value into a formatted JSON string with a configurable indent width.

    This is the flexible alternative to `json_encode_pretty`, giving you direct
    control over how many spaces are used for each indentation level.  The
    `indent` must be between 0 and 16 (inclusive).

    Args:
        value: The dictionary or Starlark value to encode
        indent: Number of spaces per indentation level (0–16, default 2).
                0 adds newlines without indentation; 4 is a common alternative
                to the default 2-space style.

    Returns:
        A formatted JSON string using `indent` spaces per level

    Raises:
        Raises an error if `indent` is outside the range 0–16, or if the
        value cannot be serialized to JSON (e.g. NaN, Infinity).

    Examples:
        4-space indentation:
        ```starlark
        data = {"key": "value", "nums": [1, 2, 3]}
        wide = json_encode_indented(data, indent=4)
        print(wide)
        # {
        #     "key": "value",
        #     "nums": [
        #         1,
        #         2,
        #         3
        #     ]
        # }
        ```

        1-space indentation (compact but readable):
        ```starlark
        narrow = json_encode_indented(data, indent=1)
        ```

        Default (same as json_encode_pretty):
        ```starlark
        two_space = json_encode_indented(data)
        ```
    """
    return json.to_string_indented(value, indent = indent)

def json_is_valid(json_string: str):
    """
    Check whether a string is valid JSON.

    This function validates a string without parsing it, useful for checking
    JSON validity before attempting to decode it. Returns True if the string
    is valid JSON, False otherwise. Does not raise an error for invalid JSON.

    Args:
        json_string: The string to validate

    Returns:
        True if the string is valid JSON, False otherwise

    Examples:
        Validate user input before processing:
        ```starlark
        user_input = '{"id": 123}'
        if json_is_valid(user_input):
            data = json_decode(user_input)
            print("Successfully parsed:", data)
        else:
            print("Invalid JSON provided")
        ```

        Check various strings:
        ```starlark
        print(json_is_valid('{"key": "value"}'))  # True
        print(json_is_valid('[1, 2, 3]'))         # True
        print(json_is_valid('"text"'))            # True
        print(json_is_valid('123'))               # True
        print(json_is_valid('not json'))          # False
        print(json_is_valid('{broken}'))          # False
        ```

        Safe JSON parsing with validation:
        ```starlark
        def safe_json_decode(text, default=None):
            # Safely decode JSON, returning default on error
            if json_is_valid(text):
                return json_decode(text)
            return default

        result = safe_json_decode('{"x": 1}', {})
        print(result)  # {"x": 1}
        ```
    """
    return json.is_string_json(json_string)

def json_try_decode(json_string: str, default = None):
    """
    Attempt to decode a JSON string, returning a default value on failure.

    This function provides safe JSON parsing with graceful error handling.
    If the string is not valid JSON, it returns the default value instead
    of raising an exception.

    Internally this calls `json.try_string_to_dict` with the supplied
    `default`, so the input is parsed only once.  A non-None default lets
    callers distinguish a successfully decoded JSON `null` (returns `None`)
    from a parse failure (returns the custom default):

    Args:
        json_string: The JSON string to decode.
        default: The value to return if decoding fails. Default is None.

    Returns:
        The decoded value if successful, or the default value if decoding fails.

    Examples:
        Safe parsing with default:
        ```starlark
        result = json_try_decode('invalid json', default={})
        print(result)  # Output: {}
        ```

        Safe parsing with successful decode:
        ```starlark
        data = json_try_decode('{"key": "value"}', default={})
        print(data["key"])  # Output: value
        ```

        Distinguishing JSON null from parse failure:
        ```starlark
        MISSING = "PARSE_FAILED"
        result = json_try_decode("null", default=MISSING)
        # result is None  (valid JSON null, not a failure)

        result = json_try_decode("{bad}", default=MISSING)
        # result is "PARSE_FAILED"  (parse failed, default returned)
        ```
    """
    return json.try_string_to_dict(json_string, default = default)

def json_read_file(path: str):
    """
    Read and parse a JSON file into a Starlark value.

    This is a convenience wrapper around fs.read_json_to_dict. It reads the
    file at the given path (relative to the workspace root), parses its contents
    as JSON, and returns the resulting Starlark value. I/O errors and JSON parse
    errors are both propagated as exceptions with descriptive messages.

    Args:
        path: Path to the JSON file, relative to the workspace root

    Returns:
        The parsed JSON value (dict, list, string, number, bool, or None)

    Raises:
        Error: If the file cannot be read (I/O error) or contains invalid JSON

    Examples:
        ```starlark
        config = json_read_file("config/settings.json")
        print(config["version"])
        ```
    """
    return fs.read_json_to_dict(path)

def json_write_file(path: str, value, pretty: bool = True):
    """
    Serialize a Starlark value to JSON and write it to a file.

    This is a convenience wrapper around fs.write_json_from_dict. It converts
    the given value to JSON and writes it to the specified file path (relative
    to the workspace root). By default the output is pretty-printed with
    indentation. I/O errors and serialization errors are propagated as
    exceptions with descriptive messages.

    Args:
        path: Destination file path, relative to the workspace root
        value: The Starlark value (dict, list, etc.) to serialize
        pretty: If True (default), write formatted JSON with indentation.
                If False, write compact JSON with no extra whitespace.

    Returns:
        None

    Raises:
        Error: If the value cannot be serialized or the file cannot be written

    Examples:
        ```starlark
        # Write pretty JSON (default)
        json_write_file("output/result.json", {"status": "ok", "count": 42})

        # Write compact JSON
        json_write_file("output/compact.json", {"x": 1}, pretty=False)
        ```
    """
    return fs.write_json_from_dict(path, value, pretty = pretty)

def json_merge(dict1, dict2):
    """
    Merge two dictionaries, with values from dict2 overwriting dict1.

    This utility function performs a shallow merge of two dictionaries.
    Values from dict2 override those in dict1. For deep merging of nested
    structures, consider using json_merge recursively.

    Args:
        dict1: The base dictionary.
        dict2: The dictionary to merge in. Its values override dict1's values.

    Returns:
        dict: A new dictionary with merged values.

    Example:
        ```starlark
        defaults = {
            "host": "localhost",
            "port": 8080,
            "debug": False,
        }

        user_config = {
            "port": 9000,
            "debug": True,
        }

        final_config = json_merge(defaults, user_config)
        print(final_config)
        # Output:
        # {
        #     "host": "localhost",
        #     "port": 9000,
        #     "debug": true
        # }
        ```
    """
    result = dict(dict1)
    result.update(dict2)
    return result
