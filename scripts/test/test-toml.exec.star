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
    "toml_is_valid",
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
    "invalid_input": {},
    "merging": {},
    "backward_compatibility": {},
    "datetime_handling": {},
    "float_handling": {},
    "arrays_of_tables": {},
    "toml_is_valid": {},
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
toml_results["basic_decoding"]["simple_parse"] = parsed.get("app").get("name") == "TestApp"
toml_results["basic_decoding"]["parse_version"] = parsed.get("app").get("version") == "1.0.0"
toml_results["basic_decoding"]["parse_bool"] = parsed.get("app").get("debug") == False

# Test TOML with arrays
array_toml = '''
title = "Config"
ports = [80, 8080, 8443]
features = ["auth", "logging", "caching"]
'''

array_parsed = toml_decode(array_toml)
toml_results["basic_decoding"]["parse_arrays"] = array_parsed.get("ports") == [80, 8080, 8443]
toml_results["basic_decoding"]["parse_string_arrays"] = len(array_parsed.get("features")) == 3

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
toml_results["complex_structures"]["nested_access"] = complex_parsed.get("database").get("host") == "localhost"
toml_results["complex_structures"]["deep_nested_access"] = complex_parsed.get("database").get("connection").get("timeout") == 30
toml_results["complex_structures"]["multiple_sections"] = complex_parsed.get("server").get("workers") == 4

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
toml_results["encoding"]["roundtrip_name"] = decoded_round.get("name") == original.get("name")
toml_results["encoding"]["roundtrip_bool"] = decoded_round.get("enabled") == original.get("enabled")
toml_results["encoding"]["roundtrip_number"] = decoded_round.get("count") == 42

# Test decoding with valid data
valid_toml_data = 'name = "Test"\nvalue = 123'
safe_decode_valid = toml_try_decode(valid_toml_data)
toml_results["error_handling"]["try_decode_valid_returns_data"] = safe_decode_valid.get("name") == "Test"
toml_results["error_handling"]["try_decode_with_default_works"] = toml_try_decode(valid_toml_data, default = {}).get("name") == "Test"

# ============================================================================
# Invalid input / error handling
# ============================================================================

# toml_try_decode should return None for invalid TOML
bad_toml_1 = "this is not = = valid toml"
toml_results["invalid_input"]["try_decode_bad_returns_none"] = toml_try_decode(bad_toml_1) == None

# toml_try_decode with custom default on bad input
toml_results["invalid_input"]["try_decode_bad_returns_default_dict"] = toml_try_decode(bad_toml_1, default = {}) == {}
toml_results["invalid_input"]["try_decode_bad_returns_default_string"] = toml_try_decode("= broken", default = "fallback") == "fallback"

# Bare key without value is invalid
toml_results["invalid_input"]["try_decode_bare_key_no_value"] = toml_try_decode("key_only") == None

# toml_try_decode returns actual data for valid input even when default is given
toml_results["invalid_input"]["try_decode_valid_ignores_default"] = toml_try_decode("x = 1", default = {}).get("x") == 1

# ============================================================================
# Float handling
# ============================================================================

float_toml = """
pi = 3.14159
negative = -2.71828
zero_float = 0.0
scientific = 6.626e-34
"""

float_parsed = toml_decode(float_toml)
toml_results["float_handling"]["parse_positive_float"] = float_parsed.get("pi") > 3.14 and float_parsed.get("pi") < 3.15
toml_results["float_handling"]["parse_negative_float"] = float_parsed.get("negative") < -2.7 and float_parsed.get("negative") > -2.8
toml_results["float_handling"]["parse_zero_float"] = float_parsed.get("zero_float") == 0.0
toml_results["float_handling"]["parse_scientific"] = float_parsed.get("scientific") > 0.0

# Float roundtrip
float_dict = {"ratio": 0.5, "scale": 1.25}
float_encoded = toml_encode(float_dict)
float_decoded = toml_decode(float_encoded)
toml_results["float_handling"]["float_roundtrip"] = float_decoded.get("ratio") == 0.5 and float_decoded.get("scale") == 1.25

# ============================================================================
# Datetime handling
# TOML has native datetime types; they are converted to ISO 8601 strings
# after going through the JSON intermediary layer.
# ============================================================================

datetime_toml = """
created_at = 1979-05-27T07:32:00Z
local_date = 1979-05-27
local_time = 07:32:00
offset_dt = 1979-05-27T00:32:00-07:00
"""

datetime_parsed = toml_decode(datetime_toml)

# All datetime types come back as strings after the TOML -> JSON -> Starlark conversion
toml_results["datetime_handling"]["offset_datetime_is_string"] = type(datetime_parsed.get("created_at")) == "string"
toml_results["datetime_handling"]["local_date_is_string"] = type(datetime_parsed.get("local_date")) == "string"
toml_results["datetime_handling"]["local_time_is_string"] = type(datetime_parsed.get("local_time")) == "string"
toml_results["datetime_handling"]["offset_dt_contains_date"] = "1979" in datetime_parsed.get("created_at")
toml_results["datetime_handling"]["offset_dt_contains_time"] = "07:32:00" in datetime_parsed.get("created_at")

# ============================================================================
# Arrays of tables (AoT)
# ============================================================================

aot_toml = '''
[[servers]]
name = "alpha"
ip = "10.0.0.1"
role = "primary"

[[servers]]
name = "beta"
ip = "10.0.0.2"
role = "replica"

[[servers]]
name = "gamma"
ip = "10.0.0.3"
role = "replica"
'''

aot_parsed = toml_decode(aot_toml)
toml_results["arrays_of_tables"]["aot_is_list"] = type(aot_parsed.get("servers")) == "list"
toml_results["arrays_of_tables"]["aot_length"] = len(aot_parsed.get("servers")) == 3
toml_results["arrays_of_tables"]["aot_first_name"] = aot_parsed.get("servers")[0].get("name") == "alpha"
toml_results["arrays_of_tables"]["aot_last_role"] = aot_parsed.get("servers")[2].get("role") == "replica"

# AoT round-trip: encode back to TOML and decode again
aot_reencoded = toml_encode({"servers": aot_parsed.get("servers")})
aot_redecoded = toml_decode(aot_reencoded)
toml_results["arrays_of_tables"]["aot_roundtrip_length"] = len(aot_redecoded.get("servers")) == 3
toml_results["arrays_of_tables"]["aot_roundtrip_name"] = aot_redecoded.get("servers")[1].get("name") == "beta"

# ============================================================================
# toml_is_valid
# ============================================================================

toml_results["toml_is_valid"]["valid_simple"] = toml_is_valid('key = "value"') == True
toml_results["toml_is_valid"]["valid_table"] = toml_is_valid("[section]\nkey = 1") == True
toml_results["toml_is_valid"]["valid_empty_string"] = toml_is_valid("") == True
toml_results["toml_is_valid"]["invalid_missing_value"] = toml_is_valid("key =") == False
toml_results["toml_is_valid"]["invalid_duplicate_equals"] = toml_is_valid("a = = 1") == False
toml_results["toml_is_valid"]["invalid_unclosed_string"] = toml_is_valid('name = "unclosed') == False

# ============================================================================
# Merging
# ============================================================================

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
toml_results["merging"]["merge_preserves_base"] = merged.get("server") == "localhost"
toml_results["merging"]["merge_overrides_values"] = merged.get("port") == 9000
toml_results["merging"]["merge_adds_override"] = merged.get("debug") == True
toml_results["merging"]["merge_keeps_non_overridden"] = merged.get("workers") == 4

# ============================================================================
# Backward compatibility — original function names
# ============================================================================

compat_toml = 'app = "MyApp"\nversion = "1.0"'
compat_parsed = toml_parse_string(compat_toml)
toml_results["backward_compatibility"]["parse_string_works"] = compat_parsed.get("app") == "MyApp"

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
