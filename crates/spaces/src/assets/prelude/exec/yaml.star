"""
Spaces YAML module - Ergonomic wrappers for YAML serialization and deserialization

This module provides clean, well-documented functions for working with YAML data
in Starlark scripts. All functions handle errors gracefully and provide clear
feedback when something goes wrong.

YAML is ideal for configuration files with its human-readable syntax that clearly
represents hierarchical data structures using indentation and minimal punctuation.
This module makes it easy to parse and generate YAML in your Starlark scripts.
"""

# ============================================================================
# Original Functions - Maintained for Backwards Compatibility
# ============================================================================

def yaml_parse_string(content: str):
    """
    Parse a YAML formatted string into a dictionary.

    This function is the original backwards-compatible name for parsing YAML
    strings. New code should use yaml_decode() instead.

    Args:
        content: The YAML-formatted string to parse

    Returns:
        A dictionary representation of the YAML data

    Raises:
        Raises an error if the YAML string is malformed or invalid
    """
    return yaml.string_to_dict(content)

def yaml_to_string(value):
    """
    Convert a dictionary to a YAML-formatted string.

    This function is the original backwards-compatible name for encoding YAML.
    New code should use yaml_encode() instead.

    Args:
        value: The dictionary or Starlark value to serialize

    Returns:
        The YAML string representation of the input value

    Raises:
        Raises an error if the value cannot be serialized to YAML
    """
    return yaml.to_string(value)

# ============================================================================
# New Ergonomic Functions
# ============================================================================

def yaml_decode(yaml_string: str):
    """
    Parse a YAML string into a Starlark dictionary.

    This function takes a YAML-formatted string and converts it into a Starlark
    dictionary that you can easily work with. YAML is a human-readable
    configuration file format that maps well to Starlark data structures.
    The YAML must be valid or this function will raise an error.

    Args:
        yaml_string: A valid YAML-formatted string to parse

    Returns:
        A Starlark dictionary representing the parsed YAML data

    Raises:
        Raises an error if the YAML string is malformed or invalid

    Examples:
        Parse a simple YAML configuration:
        ```starlark
        config_yaml = '''
        app:
          name: MyApp
          version: 1.0.0
          debug: false
        '''
        config = yaml_decode(config_yaml)
        print(config["app"]["name"])  # Output: MyApp
        print(config["app"]["version"])  # Output: 1.0.0
        ```

        Parse YAML with lists:
        ```starlark
        data_yaml = '''
        title: Configuration
        ports:
          - 80
          - 8080
          - 8443

        database:
          host: localhost
          port: 5432
        '''
        data = yaml_decode(data_yaml)
        print(data["ports"])  # Output: [80, 8080, 8443]
        print(data["database"]["host"])  # Output: localhost
        ```

        Parse nested YAML structures:
        ```starlark
        nested_yaml = '''
        project:
          name: MyProject
          version: 2.0
          database:
            url: postgres://localhost
            timeout: 30
          cache:
            enabled: true
            ttl: 3600
        '''
        config = yaml_decode(nested_yaml)
        print(config["project"]["database"]["url"])  # Output: postgres://localhost
        ```
    """
    return yaml.string_to_dict(yaml_string)

def yaml_loads(yaml_string: str):
    """
    Load a YAML string into a Starlark dictionary.

    This is an alias for yaml_decode() following common Python naming conventions.
    It parses a YAML-formatted string and converts it into a Starlark dictionary.

    Args:
        yaml_string: A valid YAML-formatted string to parse

    Returns:
        A Starlark dictionary representing the parsed YAML data

    Raises:
        Raises an error if the YAML string is malformed or invalid

    Examples:
        Load a simple YAML document:
        ```starlark
        yaml_content = '''
        name: John Doe
        age: 30
        city: New York
        '''
        person = yaml_loads(yaml_content)
        print(person["name"])  # Output: John Doe
        print(person["age"])   # Output: 30
        ```
    """
    return yaml.string_to_dict(yaml_string)

def yaml_encode(value):
    """
    Convert a dictionary into a YAML-formatted string.

    This function serializes Starlark dictionaries and other values into
    YAML-formatted strings suitable for configuration files or output.
    By default, YAML is naturally formatted in a human-readable way.

    Args:
        value: The dictionary or Starlark value to encode as YAML

    Returns:
        A YAML-formatted string representation of the input value

    Raises:
        Raises an error if the value cannot be serialized to YAML

    Examples:
        Encode a simple dictionary to YAML:
        ```starlark
        config = {
            "app": {
                "name": "MyApp",
                "version": "1.0.0",
                "debug": False,
            }
        }
        yaml_str = yaml_encode(config)
        print(yaml_str)
        # Output:
        # app:
        #   name: MyApp
        #   version: 1.0.0
        #   debug: false
        ```

        Encode with lists:
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
        yaml_str = yaml_encode(app_config)
        print(yaml_str)
        # Output:
        # title: MyApplication
        # version: 1.2.0
        # features:
        #   - auth
        #   - logging
        #   - caching
        # database:
        #   host: db.example.com
        #   port: 5432
        #   pool_size: 20
        #   timeout: 30
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
        yaml_str = yaml_encode(full_config)
        ```
    """
    return yaml.to_string(value)

def yaml_dumps(value):
    """
    Dump a dictionary into a YAML-formatted string.

    This is an alias for yaml_encode() following common Python naming conventions.
    It converts Starlark dictionaries and other values into YAML-formatted strings
    suitable for configuration files or output.

    Args:
        value: The dictionary or Starlark value to dump as YAML

    Returns:
        A YAML-formatted string representation of the input value

    Raises:
        Raises an error if the value cannot be serialized to YAML

    Examples:
        Dump a configuration structure:
        ```starlark
        settings = {
            "db": {
                "host": "localhost",
                "port": 5432,
            },
            "features": ["auth", "api"],
        }
        yaml_str = yaml_dumps(settings)
        ```
    """
    return yaml.to_string(value)

def yaml_try_decode(yaml_string: str, default = None):
    """
    Attempt to decode a YAML string, returning a default value on failure.

    This function provides safe YAML parsing with graceful error handling.
    If the string is not valid YAML, it returns the default value instead
    of raising an exception.

    Args:
        yaml_string: The YAML string to decode.
        default: The value to return if decoding fails. Default is None.

    Returns:
        The decoded dictionary if successful, or the default value if decoding fails.

    Examples:
        Safe parsing with default value:
        ```starlark
        result = yaml_try_decode('invalid: yaml: content:', default={})
        print(result)  # Output: {}

        Safe parsing with successful decode:
        ```starlark
        data = yaml_try_decode('''
        name: MyApp
        version: 1.0
        ''', default={})
        print(data["name"])  # Output: MyApp
        ```
    """
    return yaml.try_string_to_dict(yaml_string, default = default)

def yaml_merge(base_dict, override_dict):
    """
    Merge two dictionaries, with values from override_dict replacing base_dict values.

    This utility function performs a shallow merge of two dictionaries, commonly
    used when combining default configuration with user-provided YAML overrides.
    Values from override_dict take precedence over those in base_dict.
    For deep merging of nested structures, consider applying this function recursively.

    Args:
        base_dict: The base/default dictionary.
        override_dict: The dictionary with overrides. Its values replace base_dict's values.

    Returns:
        A new dictionary with merged values where override_dict values take precedence.

    Examples:
        Merge default settings with YAML overrides:
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

        final_config = yaml_merge(defaults, user_settings)
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

        prod_config = yaml_merge(base_config, prod_overrides)
        staging_config = yaml_merge(base_config, staging_overrides)
        ```

        Combine parsed YAML documents:
        ```starlark
        load("//@star/sdk/star/std/fs.star", "fs_read_text")

        default_config = yaml_decode(fs_read_text("config.yaml"))
        local_overrides = yaml_decode(fs_read_text("config.local.yaml"))

        final_config = yaml_merge(default_config, local_overrides)
        ```
    """
    result = dict(base_dict)
    result.update(override_dict)
    return result

def yaml_encode_compact(value):
    """
    Convert a value into a compact YAML string.

    This is a convenience function for encoding with minimal whitespace when needed,
    though YAML is typically compact by default. Equivalent to calling yaml_encode(value).

    Args:
        value: The dictionary or Starlark value to encode

    Returns:
        A compact YAML string

    Examples:
        ```starlark
        config = {
            "name": "App",
            "version": "1.0",
            "features": ["f1", "f2"],
        }
        compact = yaml_encode_compact(config)
        ```
    """
    return yaml.to_string(value)

def yaml_encode_pretty(value):
    """
    Convert a value into a formatted YAML string with proper indentation.

    This is a convenience function for creating human-readable YAML output.
    Since YAML is inherently readable, this currently produces the same
    formatted output as yaml_encode().

    Args:
        value: The dictionary or Starlark value to encode

    Returns:
        A formatted YAML string with proper indentation

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
        print(yaml_encode_pretty(app_config))
        # Output:
        # app:
        #   name: MyService
        #   version: 1.0.0
        # server:
        #   host: localhost
        #   port: 8080
        #   workers: 4
        # database:
        #   url: postgres://localhost/db
        #   pool: 10
        #   timeout: 30
        ```

        Save configuration to a file:
        ```starlark
        load("//@star/sdk/star/std/fs.star", "fs_write_text")

        config = {"app": {"name": "MyApp"}}
        fs_write_text("config.yaml", yaml_encode_pretty(config))
        ```
    """
    return yaml.to_string(value)

def yaml_is_valid(yaml_string: str):
    """
    Check whether a string is valid YAML without raising an error.

    This is a quick validation helper: it returns True if the string parses
    as YAML (single-document) and False otherwise. No decoded data is returned.

    Args:
        yaml_string: The string to validate.

    Returns:
        True if the string is valid YAML, False otherwise.

    Examples:
        ```starlark
        if yaml_is_valid(raw):
            config = yaml_decode(raw)
        else:
            print("invalid YAML, using defaults")
        ```
    """
    return yaml.is_string_yaml(yaml_string)
