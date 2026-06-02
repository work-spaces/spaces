"""
Spaces TOML module - Ergonomic wrappers for TOML serialization and deserialization

This module provides clean, well-documented functions for working with TOML data
in Starlark scripts. All functions handle errors gracefully and provide clear
feedback when something goes wrong.

TOML is ideal for configuration files with its human-readable syntax that clearly
represents hierarchical data structures. This module makes it easy to parse and
generate TOML in your Starlark scripts.
"""

# ============================================================================
# Original Functions - Maintained for Backwards Compatibility
# ============================================================================

def toml_parse_string(content: str):
    """
    Parse a TOML formatted string into a dictionary.

    This function is the original backwards-compatible name for parsing TOML
    strings. New code should use toml_decode() instead.

    Args:
        content: The TOML-formatted string to parse

    Returns:
        A dictionary representation of the TOML data

    Raises:
        Raises an error if the TOML string is malformed or invalid
    """
    return toml.string_to_dict(content)

def toml_to_string(value):
    """
    Convert a dictionary to a TOML-formatted string.

    This function is the original backwards-compatible name for encoding TOML.
    New code should use toml_encode() instead.

    Args:
        value: The dictionary or Starlark value to serialize

    Returns:
        The TOML string representation of the input value

    Raises:
        Raises an error if the value cannot be serialized to TOML
    """
    return toml.to_string(value)

def toml_to_string_pretty(value):
    """
    Convert a dictionary to a pretty-printed TOML-formatted string.

    This function is the original backwards-compatible name for pretty encoding.
    New code should use toml_encode(value, pretty=True) or toml_encode_pretty() instead.

    Args:
        value: The dictionary or Starlark value to serialize

    Returns:
        The pretty-formatted TOML string representation

    Raises:
        Raises an error if the value cannot be serialized to TOML
    """
    return toml.to_string_pretty(value)

# ============================================================================
# New Ergonomic Functions
# ============================================================================

def toml_decode(toml_string: str):
    """
    Parse a TOML string into a Starlark dictionary.

    This function takes a TOML-formatted string and converts it into a Starlark
    dictionary that you can easily work with. TOML is a human-readable
    configuration file format that maps well to Starlark data structures.
    The TOML must be valid or this function will raise an error.

    Args:
        toml_string: A valid TOML-formatted string to parse

    Returns:
        A Starlark dictionary representing the parsed TOML data

    Raises:
        Raises an error if the TOML string is malformed or invalid

    Examples:
        Parse a simple TOML configuration:
        ```starlark
        config_toml = '''
        [app]
        name = "MyApp"
        version = "1.0.0"
        debug = false
        '''
        config = toml_decode(config_toml)
        print(config["app"]["name"])  # Output: MyApp
        print(config["app"]["version"])  # Output: 1.0.0
        ```

        Parse TOML with arrays:
        ```starlark
        data_toml = '''
        title = "Configuration"
        ports = [80, 8080, 8443]

        [database]
        host = "localhost"
        port = 5432
        '''
        data = toml_decode(data_toml)
        print(data["ports"])  # Output: [80, 8080, 8443]
        print(data["database"]["host"])  # Output: localhost
        ```

        Parse nested TOML structures:
        ```starlark
        nested_toml = '''
        [project]
        name = "MyProject"
        version = "2.0"

        [project.database]
        url = "postgres://localhost"
        timeout = 30

        [project.cache]
        enabled = true
        ttl = 3600
        '''
        config = toml_decode(nested_toml)
        print(config["project"]["database"]["url"])  # Output: postgres://localhost
        ```
    """
    return toml.string_to_dict(toml_string)

def toml_encode(value, pretty: bool = False):
    """
    Convert a dictionary into a TOML-formatted string.

    This function serializes Starlark dictionaries and other values into
    TOML-formatted strings suitable for configuration files or output.
    By default, the output is compact. For human-readable formatting with
    better spacing and organization, use toml_encode_pretty() instead.

    Args:
        value: The dictionary or Starlark value to encode as TOML
        pretty: If True, format with better spacing and organization (default: False)

    Returns:
        A TOML-formatted string representation of the input value

    Raises:
        Raises an error if the value cannot be serialized to TOML

    Examples:
        Encode a simple dictionary to TOML:
        ```starlark
        config = {
            "app": {
                "name": "MyApp",
                "version": "1.0.0",
                "debug": False,
            }
        }
        toml_str = toml_encode(config)
        print(toml_str)
        ```

        Encode with pretty formatting:
        ```starlark
        app_config = {
            "title": "MyApplication",
            "version": "1.2.0",
            "features": ["auth", "logging", "caching"],
            "database": {
                "host": "db.example.com",
                "port": 5432,
                "pool_size": 20,
                "timeout": 30,
            },
        }
        pretty_toml = toml_encode(app_config, pretty=True)
        print(pretty_toml)
        # Output:
        # title = "MyApplication"
        # version = "1.2.0"
        # features = ["auth", "logging", "caching"]
        #
        # [database]
        # host = "db.example.com"
        # port = 5432
        # pool_size = 20
        # timeout = 30
        ```

        Encode nested configuration structures:
        ```starlark
        full_config = {
            "name": "WebServer",
            "version": "3.0",
            "server": {
                "host": "0.0.0.0",
                "port": 8080,
                "ssl": True,
            },
            "logging": {
                "level": "INFO",
                "format": "json",
                "outputs": ["stdout", "file"],
            },
        }
        toml_str = toml_encode(full_config, pretty=True)
        ```
    """
    if pretty:
        return toml.to_string_pretty(value)
    else:
        return toml.to_string(value)

def toml_encode_compact(value):
    """
    Convert a value into a compact TOML string without extra whitespace.

    This is a convenience function for encoding with minimal whitespace,
    useful for efficient file storage or network transmission of configuration data.
    Equivalent to calling toml_encode(value, pretty=False).

    Args:
        value: The dictionary or Starlark value to encode

    Returns:
        A compact TOML string with minimal whitespace

    Examples:
        ```starlark
        config = {
            "name": "App",
            "version": "1.0",
            "features": ["f1", "f2"],
        }
        compact = toml_encode_compact(config)
        # Produces minimal whitespace TOML
        print(compact)
        ```

        Useful for storing configuration in minimal space:
        ```starlark
        settings = {"debug": False, "timeout": 60}
        compact_toml = toml_encode_compact(settings)
        # Store in database or transmit over network
        ```
    """
    return toml.to_string(value)

def toml_encode_pretty(value):
    """
    Convert a value into a formatted TOML string with proper indentation.

    This is a convenience function for creating human-readable TOML output
    with proper indentation and section organization. This is ideal for
    configuration files that need to be read and edited by humans.
    Equivalent to calling toml_encode(value, pretty=True).

    Args:
        value: The dictionary or Starlark value to encode

    Returns:
        A formatted TOML string with indentation and newlines

    Examples:
        Create a human-readable configuration file:
        ```starlark
        app_config = {
            "app": {
                "name": "MyService",
                "version": "1.0.0",
            },
            "server": {
                "host": "localhost",
                "port": 8080,
                "workers": 4,
            },
            "database": {
                "url": "postgres://localhost/db",
                "pool": 10,
                "timeout": 30,
            },
        }
        print(toml_encode_pretty(app_config))
        # Output:
        # [app]
        # name = "MyService"
        # version = "1.0.0"
        #
        # [server]
        # host = "localhost"
        # port = 8080
        # workers = 4
        #
        # [database]
        # url = "postgres://localhost/db"
        # pool = 10
        # timeout = 30
        ```

        Save configuration to a file:
        ```starlark
        load("//@star/sdk/star/std/fs.star", "fs_write_text")

        config = {"app": {"name": "MyApp"}}
        fs_write_text("config.toml", toml_encode_pretty(config))
        ```
    """
    return toml.to_string_pretty(value)

def toml_try_decode(toml_string: str, default = None):
    """
    Attempt to decode a TOML string, returning a default value on failure.

    This function provides safe TOML parsing with graceful error handling.
    If the string is not valid TOML, it returns the default value instead
    of raising an exception.

    Args:
        toml_string: The TOML string to decode.
        default: The value to return if decoding fails. Default is None.

    Returns:
        The decoded dictionary if successful, or the default value if decoding fails.

    Examples:
        Safe parsing with default value:
        ```starlark
        result = toml_try_decode('invalid: toml: content:', default={})
        print(result)  # Output: {}

        Safe parsing with successful decode:
        ```starlark
        data = toml_try_decode('''
        name = "MyApp"
        version = "1.0"
        ''', default={})
        print(data["name"])  # Output: MyApp
        ```
    """
    return toml.try_string_to_dict(toml_string, default = default)

def toml_is_valid(toml_string: str):
    """
    Check whether a string is valid TOML without raising an error.

    This is a quick validation helper: it returns True if the string parses
    as TOML and False otherwise. No decoded data is returned.

    Args:
        toml_string: The string to validate.

    Returns:
        True if the string is valid TOML, False otherwise.

    Examples:
        ```starlark
        if toml_is_valid(raw):
            config = toml_decode(raw)
        else:
            print("invalid TOML, using defaults")
        ```
    """
    return toml.is_string_toml(toml_string)

def toml_merge(base_dict, override_dict):
    """
    Merge two dictionaries, with values from override_dict replacing base_dict values.

    This utility function performs a shallow merge of two dictionaries, commonly
    used when combining default configuration with user-provided overrides.
    Values from override_dict take precedence over those in base_dict.
    For deep merging of nested structures, consider applying this function recursively.

    Args:
        base_dict: The base/default dictionary.
        override_dict: The dictionary with overrides. Its values replace base_dict's values.

    Returns:
        A new dictionary with merged values where override_dict values take precedence.

    Examples:
        Merge default settings with user overrides:
        ```starlark
        defaults = {
            "server": "localhost",
            "port": 8080,
            "debug": False,
            "workers": 4,
        }

        user_settings = {
            "port": 9000,
            "debug": True,
        }

        final_config = toml_merge(defaults, user_settings)
        print(final_config)
        # Output:
        # {
        #     "server": "localhost",
        #     "port": 9000,
        #     "debug": true,
        #     "workers": 4
        # }
        ```

        Stack multiple configurations:
        ```starlark
        base_config = {"feature_a": False, "feature_b": False}
        prod_overrides = {"feature_a": True}
        staging_overrides = {"feature_b": True}

        prod_config = toml_merge(base_config, prod_overrides)
        staging_config = toml_merge(base_config, staging_overrides)
        ```

        Combine parsed TOML files:
        ```starlark
        load("//@star/sdk/star/std/fs.star", "fs_read_text")

        default_config = toml_decode(fs_read_text("config.toml"))
        local_overrides = toml_decode(fs_read_text("config.local.toml"))

        final_config = toml_merge(default_config, local_overrides)
        ```
    """
    result = dict(base_dict)
    result.update(override_dict)
    return result
