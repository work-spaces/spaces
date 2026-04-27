#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/yaml.star",
    "yaml_decode",
    "yaml_dumps",
    "yaml_encode",
    "yaml_encode_compact",
    "yaml_encode_pretty",
    "yaml_loads",
    "yaml_merge",
    "yaml_parse_string",
    "yaml_to_string",
    "yaml_try_decode",
)

# YAML module test results
yaml_results = {
    "basic_decoding": {},
    "complex_structures": {},
    "encoding": {},
    "pretty_formatting": {},
    "error_handling": {},
    "merging": {},
    "backward_compatibility": {},
}

# ============================================================================
# YAML Module Tests
# ============================================================================

# Test basic YAML decoding
simple_yaml = """app:
  name: TestApp
  version: "1.0.0"
  debug: false
"""
parsed = yaml_decode(simple_yaml)
yaml_results["basic_decoding"]["simple_parse"] = parsed["app"]["name"] == "TestApp"
yaml_results["basic_decoding"]["parse_version"] = parsed["app"]["version"] == "1.0.0"
yaml_results["basic_decoding"]["parse_bool"] = parsed["app"]["debug"] == False

# Test YAML with lists
list_yaml = """title: Configuration
ports:
  - 80
  - 8080
  - 8443
features:
  - auth
  - logging
  - cache
"""
list_parsed = yaml_decode(list_yaml)
yaml_results["basic_decoding"]["parse_lists"] = list_parsed["ports"] == [80, 8080, 8443]
yaml_results["basic_decoding"]["parse_string_lists"] = len(list_parsed["features"]) == 3
yaml_results["basic_decoding"]["list_first_element"] = list_parsed["features"][0] == "auth"

# Test complex nested YAML
complex_yaml = """project:
  name: MyProject
  version: 2.0
  database:
    host: localhost
    port: 5432
    connection:
      timeout: 30
      max_retries: 3
  server:
    workers: 4
    timeout: 60
"""
complex_parsed = yaml_decode(complex_yaml)
yaml_results["complex_structures"]["nested_access"] = complex_parsed["project"]["database"]["host"] == "localhost"
yaml_results["complex_structures"]["deep_nested_access"] = complex_parsed["project"]["database"]["connection"]["timeout"] == 30
yaml_results["complex_structures"]["multiple_sections"] = complex_parsed["project"]["server"]["workers"] == 4

# Test YAML encoding
data_to_encode = {
    "title": "MyApp",
    "version": "1.2.0",
    "features": ["auth", "logging", "caching"],
}
encoded = yaml_encode(data_to_encode)
yaml_results["encoding"]["basic_encode"] = len(encoded) > 0
yaml_results["encoding"]["encoded_contains_title"] = "title" in encoded
yaml_results["encoding"]["encoded_contains_version"] = "version" in encoded

# Test compact encoding
compact_encoded = yaml_encode_compact(data_to_encode)
yaml_results["encoding"]["compact_encode_returns_string"] = type(compact_encoded) == "string"
yaml_results["encoding"]["compact_contains_key"] = "title" in compact_encoded

# Test pretty encoding
pretty_data = {
    "app": {
        "name": "TestService",
        "version": "1.0.0",
    },
    "server": {
        "host": "localhost",
        "port": 8080,
    },
}
pretty_encoded = yaml_encode_pretty(pretty_data)
yaml_results["pretty_formatting"]["pretty_encode_returns_string"] = type(pretty_encoded) == "string"
yaml_results["pretty_formatting"]["pretty_contains_keys"] = "app" in pretty_encoded and "server" in pretty_encoded
yaml_results["pretty_formatting"]["pretty_encode_longer"] = len(pretty_encoded) > 0

# Test roundtrip encoding/decoding
original = {
    "name": "TestConfig",
    "enabled": True,
    "count": 42,
    "items": ["a", "b", "c"],
}
encoded_round = yaml_encode(original)
decoded_round = yaml_decode(encoded_round)
yaml_results["encoding"]["roundtrip_success"] = type(decoded_round) == "dict"
yaml_results["encoding"]["roundtrip_name"] = decoded_round["name"] == original["name"]
yaml_results["encoding"]["roundtrip_bool"] = decoded_round["enabled"] == original["enabled"]

# Check that fields survive the roundtrip
yaml_results["encoding"]["roundtrip_number"] = "count" in decoded_round and decoded_round["count"] != None
yaml_results["encoding"]["roundtrip_list"] = "items" in decoded_round and len(decoded_round["items"]) == 3

# Test decoding with valid data
valid_yaml_data = """name: Test
value: 123
active: true
"""
safe_decode_valid = yaml_try_decode(valid_yaml_data)
yaml_results["error_handling"]["try_decode_valid_returns_data"] = safe_decode_valid["name"] == "Test"
yaml_results["error_handling"]["try_decode_with_default_works"] = yaml_try_decode(valid_yaml_data, default = {})["name"] == "Test"

# Test merging dictionaries
base_yaml_config = {
    "server": "localhost",
    "port": 8080,
    "debug": False,
    "workers": 4,
}

override_yaml_config = {
    "port": 9000,
    "debug": True,
}

yaml_merged = yaml_merge(base_yaml_config, override_yaml_config)
yaml_results["merging"]["merge_preserves_base"] = yaml_merged["server"] == "localhost"
yaml_results["merging"]["merge_overrides_values"] = yaml_merged["port"] == 9000
yaml_results["merging"]["merge_adds_override"] = yaml_merged["debug"] == True
yaml_results["merging"]["merge_keeps_non_overridden"] = yaml_merged["workers"] == 4

# Test backward compatibility - original function names
compat_yaml = """app: MyApp
version: 1.0
"""
compat_yaml_parsed = yaml_parse_string(compat_yaml)
yaml_results["backward_compatibility"]["parse_string_works"] = compat_yaml_parsed["app"] == "MyApp"

compat_yaml_dict = {"title": "Test", "enabled": True}
compat_yaml_encoded = yaml_to_string(compat_yaml_dict)
yaml_results["backward_compatibility"]["to_string_works"] = "title" in compat_yaml_encoded

# Test yaml_loads and yaml_dumps aliases
loads_yaml = """config:
  name: LoadTest
  port: 3000
"""
loaded = yaml_loads(loads_yaml)
yaml_results["backward_compatibility"]["loads_works"] = loaded["config"]["name"] == "LoadTest"

dumped = yaml_dumps({"test": "value", "number": 99})
yaml_results["backward_compatibility"]["dumps_works"] = "test" in dumped

# ============================================================================
# Output Results
# ============================================================================

print("YAML Module Test Results:")
print("=======================")
print("")
print(json_dumps(yaml_results, is_pretty = True))
print("")
print("All yaml functions executed successfully!")
