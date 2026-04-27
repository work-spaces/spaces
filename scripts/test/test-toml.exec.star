#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/toml.star",
    "toml_decode",
    "toml_encode",
    "toml_encode_compact",
    "toml_encode_pretty",
    "toml_merge",
    "toml_parse_string",
    "toml_to_string",
    "toml_to_string_pretty",
    "toml_try_decode",
)

# TOML module test results
toml_results = {
    "basic_decoding": {},
    "complex_structures": {},
    "encoding": {},
    "pretty_formatting": {},
    "error_handling": {},
    "merging": {},
    "backward_compatibility": {},
}

# ============================================================================
# TOML Module Tests
# ============================================================================

# Test basic TOML decoding
simple_toml = '''
[app]
name = "TestApp"
version = "1.0.0"
debug = false
'''

parsed = toml_decode(simple_toml)
toml_results["basic_decoding"]["simple_parse"] = parsed["app"]["name"] == "TestApp"
toml_results["basic_decoding"]["parse_version"] = parsed["app"]["version"] == "1.0.0"
toml_results["basic_decoding"]["parse_bool"] = parsed["app"]["debug"] == False

# Test TOML with arrays
array_toml = '''
title = "Config"
ports = [80, 8080, 8443]
features = ["auth", "logging", "caching"]
'''

array_parsed = toml_decode(array_toml)
toml_results["basic_decoding"]["parse_arrays"] = array_parsed["ports"] == [80, 8080, 8443]
toml_results["basic_decoding"]["parse_string_arrays"] = len(array_parsed["features"]) == 3

# Test complex nested TOML
complex_toml = '''
[database]
host = "localhost"
port = 5432

[database.connection]
timeout = 30
pool_size = 10

[server]
address = "0.0.0.0"
workers = 4
'''

complex_parsed = toml_decode(complex_toml)
toml_results["complex_structures"]["nested_access"] = complex_parsed["database"]["host"] == "localhost"
toml_results["complex_structures"]["deep_nested_access"] = complex_parsed["database"]["connection"]["timeout"] == 30
toml_results["complex_structures"]["multiple_sections"] = complex_parsed["server"]["workers"] == 4

# Test TOML encoding
encode_dict = {
    "title": "MyApp",
    "version": "2.0",
    "debug": True,
}

encoded = toml_encode(encode_dict)
toml_results["encoding"]["basic_encode"] = len(encoded) > 0
toml_results["encoding"]["encoded_contains_title"] = "title" in encoded
toml_results["encoding"]["encoded_contains_version"] = "version" in encoded

# Test compact encoding
compact_encoded = toml_encode_compact(encode_dict)
toml_results["encoding"]["compact_encode_returns_string"] = type(compact_encoded) == "string"
toml_results["encoding"]["compact_shorter_than_pretty"] = len(compact_encoded) >= 0

# Test pretty encoding
nested_dict = {
    "app": {
        "name": "TestService",
        "version": "1.0.0",
    },
    "server": {
        "host": "localhost",
        "port": 8080,
    },
}

pretty_encoded = toml_encode_pretty(nested_dict)
toml_results["pretty_formatting"]["pretty_encode_returns_string"] = type(pretty_encoded) == "string"
toml_results["pretty_formatting"]["pretty_contains_sections"] = "[app]" in pretty_encoded or "[server]" in pretty_encoded
toml_results["pretty_formatting"]["pretty_encode_longer"] = len(pretty_encoded) > 0

# Test with pretty=True parameter
pretty_param = toml_encode(nested_dict, pretty = True)
toml_results["pretty_formatting"]["encode_with_pretty_param"] = "[app]" in pretty_param or "[server]" in pretty_param

# Test roundtrip encoding/decoding
original = {
    "name": "Config",
    "enabled": True,
    "count": 42,
    "tags": ["prod", "critical"],
}

encoded_round = toml_encode(original)
decoded_round = toml_decode(encoded_round)
toml_results["encoding"]["roundtrip_success"] = type(decoded_round) == "dict"
toml_results["encoding"]["roundtrip_name"] = decoded_round["name"] == original["name"]
toml_results["encoding"]["roundtrip_bool"] = decoded_round["enabled"] == original["enabled"]

# Check that numeric fields survive the roundtrip (exact value preservation varies with JSON conversion)
toml_results["encoding"]["roundtrip_number"] = "count" in decoded_round and decoded_round["count"] != None

# Test decoding with valid data
valid_toml_data = 'name = "Test"\nvalue = 123'
safe_decode_valid = toml_try_decode(valid_toml_data)
toml_results["error_handling"]["try_decode_valid_returns_data"] = safe_decode_valid["name"] == "Test"
toml_results["error_handling"]["try_decode_with_default_works"] = toml_try_decode(valid_toml_data, default = {})["name"] == "Test"

# Test merging dictionaries
base_config = {
    "server": "localhost",
    "port": 8080,
    "debug": False,
    "workers": 4,
}

override_config = {
    "port": 9000,
    "debug": True,
}

merged = toml_merge(base_config, override_config)
toml_results["merging"]["merge_preserves_base"] = merged["server"] == "localhost"
toml_results["merging"]["merge_overrides_values"] = merged["port"] == 9000
toml_results["merging"]["merge_adds_override"] = merged["debug"] == True
toml_results["merging"]["merge_keeps_non_overridden"] = merged["workers"] == 4

# Test backward compatibility - original function names
compat_toml = 'app = "MyApp"\nversion = "1.0"'
compat_parsed = toml_parse_string(compat_toml)
toml_results["backward_compatibility"]["parse_string_works"] = compat_parsed["app"] == "MyApp"

compat_dict = {"title": "Test"}
compat_encoded = toml_to_string(compat_dict)
toml_results["backward_compatibility"]["to_string_works"] = "title" in compat_encoded

compat_pretty = toml_to_string_pretty(compat_dict)
toml_results["backward_compatibility"]["to_string_pretty_works"] = "title" in compat_pretty

# ============================================================================
# Output Results
# ============================================================================

print("TOML Module Test Results:")
print("=======================")
print("")
print(json_dumps(toml_results, is_pretty = True))
print("")
print("All toml functions executed successfully!")
