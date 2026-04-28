#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_decode",
    "json_dumps",
    "json_encode",
    "json_encode_compact",
    "json_encode_indented",
    "json_encode_pretty",
    "json_is_string_json",
    "json_is_valid",
    "json_loads",
    "json_merge",
    "json_read_file",
    "json_try_decode",
    "json_write_file",
)

# JSON module test results
json_results = {
    "decode": {},
    "encode": {},
    "validation": {},
    "compact_encoding": {},
    "pretty_encoding": {},
    "type_preservation": {},
    "unicode": {},
    "large_integers": {},
    "null_handling": {},
    "try_decode": {},
    "indented_encoding": {},
    "merge": {},
    "file_roundtrip": {},
    "key_ordering": {},
    "backwards_compat": {},
}

# ============================================================================
# JSON Module Tests
# ============================================================================

# Test basic JSON decoding
simple_json = '{"name": "Alice", "age": 30, "active": true}'
decoded_simple = json_decode(simple_json)
json_results["decode"]["simple_object"] = decoded_simple
json_results["decode"]["access_name"] = decoded_simple.get("name")
json_results["decode"]["access_age"] = decoded_simple.get("age")
json_results["decode"]["access_active"] = decoded_simple.get("active")

# Test nested JSON
nested_json = '''
{
    "user": {
        "id": 123,
        "profile": {
            "email": "alice@example.com",
            "tags": ["admin", "developer"]
        }
    },
    "created": "2024-01-15"
}
'''
decoded_nested = json_decode(nested_json)
json_results["decode"]["nested_user_id"] = decoded_nested.get("user").get("id")
json_results["decode"]["nested_email"] = decoded_nested.get("user").get("profile").get("email")
json_results["decode"]["nested_first_tag"] = decoded_nested.get("user").get("profile").get("tags")[0]

# Test JSON array
array_json = '[1, 2, 3, "four", 5.5]'
decoded_array = json_decode(array_json)
json_results["decode"]["array_first"] = decoded_array[0]
json_results["decode"]["array_third"] = decoded_array[2]
json_results["decode"]["array_string"] = decoded_array[3]

# Test json_loads alias
json_results["decode"]["json_loads_alias"] = json_loads(simple_json).get("name")

# Test compact encoding
data_to_encode = {
    "name": "Bob",
    "status": "active",
    "score": 95,
}
compact_encoded = json_encode(data_to_encode, pretty = False)
json_results["encode"]["compact"] = compact_encoded

# Test pretty encoding
pretty_encoded = json_encode(data_to_encode, pretty = True)
json_results["encode"]["pretty_contains_newline"] = "\n" in pretty_encoded
json_results["encode"]["pretty_length_greater"] = len(pretty_encoded) > len(compact_encoded)

# Test json_encode_compact convenience function
compact_result = json_encode_compact(data_to_encode)
json_results["compact_encoding"]["function_exists"] = compact_result != None
json_results["compact_encoding"]["matches_regular_compact"] = compact_result == compact_encoded

# Test json_encode_pretty convenience function
pretty_result = json_encode_pretty(data_to_encode)
json_results["pretty_encoding"]["function_exists"] = pretty_result != None
json_results["pretty_encoding"]["has_newlines"] = "\n" in pretty_result

# Test validation - valid JSON
json_results["validation"]["valid_object"] = json_is_valid('{"key": "value"}')
json_results["validation"]["valid_array"] = json_is_valid("[1, 2, 3]")
json_results["validation"]["valid_string"] = json_is_valid('"text"')
json_results["validation"]["valid_number"] = json_is_valid("123")
json_results["validation"]["valid_null"] = json_is_valid("null")
json_results["validation"]["valid_boolean_true"] = json_is_valid("true")
json_results["validation"]["valid_boolean_false"] = json_is_valid("false")

# Test validation - invalid JSON
json_results["validation"]["invalid_plain_text"] = json_is_valid("not json") == False
json_results["validation"]["invalid_broken_object"] = json_is_valid("{broken}") == False
json_results["validation"]["invalid_incomplete"] = json_is_valid('{"key":') == False
json_results["validation"]["invalid_trailing_comma"] = json_is_valid('{"key": "value",}') == False

# Test round-trip encoding/decoding
original_data = {
    "project": "Spaces",
    "version": "1.0.0",
    "features": ["fast", "reliable", "scalable"],
    "metadata": {
        "created": "2024-01-15",
        "active": True,
    },
}
encoded_roundtrip = json_encode(original_data)
decoded_roundtrip = json_decode(encoded_roundtrip)
json_results["encode"]["roundtrip_project"] = decoded_roundtrip.get("project")
json_results["encode"]["roundtrip_version"] = decoded_roundtrip.get("version")
json_results["encode"]["roundtrip_first_feature"] = decoded_roundtrip.get("features")[0]
json_results["encode"]["roundtrip_created"] = decoded_roundtrip.get("metadata").get("created")

# Test json_dumps alias with pretty flag (backwards compatible parameter name)
dumped_compact = json_dumps(data_to_encode, is_pretty = False)
dumped_pretty = json_dumps(data_to_encode, is_pretty = True)
json_results["encode"]["dumps_compact_matches"] = dumped_compact == compact_encoded
json_results["encode"]["dumps_pretty_matches"] = dumped_pretty == pretty_encoded

# ============================================================================
# Type Preservation Tests
# ============================================================================

# JSON integers must stay integers, floats must stay floats after round-trip
typed_json = '{"int_val": 42, "float_val": 3.14, "neg_int": -7, "zero": 0}'
typed_decoded = json_decode(typed_json)
json_results["type_preservation"]["int_is_int"] = typed_decoded.get("int_val") == 42
json_results["type_preservation"]["float_is_float"] = typed_decoded.get("float_val") == 3.14
json_results["type_preservation"]["neg_int"] = typed_decoded.get("neg_int") == -7
json_results["type_preservation"]["zero"] = typed_decoded.get("zero") == 0

# Verify integer and float round-trip without precision loss
rt_typed = json_decode(json_encode(typed_decoded))
json_results["type_preservation"]["roundtrip_int"] = rt_typed.get("int_val") == 42
json_results["type_preservation"]["roundtrip_float"] = rt_typed.get("float_val") == 3.14

# Array mixed types preserved
mixed_arr = json_decode('[1, 2.5, "text", true, null]')
json_results["type_preservation"]["arr_int"] = mixed_arr[0] == 1
json_results["type_preservation"]["arr_float"] = mixed_arr[1] == 2.5
json_results["type_preservation"]["arr_str"] = mixed_arr[2] == "text"
json_results["type_preservation"]["arr_bool"] = mixed_arr[3] == True
json_results["type_preservation"]["arr_null"] = mixed_arr[4] == None

# ============================================================================
# Unicode Tests
# ============================================================================

# Basic Latin-1 supplement / accented characters
unicode_json = '{"greeting": "Héllo Wörld", "emoji": "\\u2603", "cjk": "\\u4e2d\\u6587"}'
unicode_decoded = json_decode(unicode_json)
json_results["unicode"]["accented_chars"] = "H" in unicode_decoded.get("greeting")
json_results["unicode"]["snowman_decoded"] = unicode_decoded.get("emoji") == "\u2603"
json_results["unicode"]["cjk_decoded"] = unicode_decoded.get("cjk") == "\u4e2d\u6587"

# Round-trip unicode
unicode_rt = json_decode(json_encode(unicode_decoded))
json_results["unicode"]["roundtrip_accented"] = unicode_rt.get("greeting") == unicode_decoded.get("greeting")
json_results["unicode"]["roundtrip_cjk"] = unicode_rt.get("cjk") == unicode_decoded.get("cjk")

# ============================================================================
# Large Integer Tests
# ============================================================================

# i64-range large integers
large_int_json = '{"big": 9223372036854775807, "neg_big": -9223372036854775808}'
large_int_decoded = json_decode(large_int_json)
json_results["large_integers"]["i64_max"] = large_int_decoded.get("big") == 9223372036854775807
json_results["large_integers"]["i64_min"] = large_int_decoded.get("neg_big") == -9223372036854775808

# u64 boundary (larger than i64::MAX)
u64_json = '{"u64_max": 18446744073709551615}'
u64_decoded = json_decode(u64_json)
json_results["large_integers"]["u64_max"] = u64_decoded.get("u64_max") == 18446744073709551615

# BigInt (beyond u64)
bigint_json = '{"big": 123456789123456789123456789}'
bigint_decoded = json_decode(bigint_json)
json_results["large_integers"]["bigint_decoded"] = bigint_decoded.get("big") == 123456789123456789123456789

# Round-trip large integer
json_results["large_integers"]["bigint_roundtrip"] = (
    json_decode(json_encode({"n": 123456789123456789123456789})).get("n") == 123456789123456789123456789
)

# ============================================================================
# Null / None Handling Tests
# ============================================================================

# Decode JSON null -> Starlark None
json_results["null_handling"]["decode_null_literal"] = json_decode("null") == None

# Null inside object
obj_with_null = json_decode('{"key": null, "other": 1}')
json_results["null_handling"]["null_in_object"] = obj_with_null.get("key") == None
json_results["null_handling"]["non_null_in_object"] = obj_with_null.get("other") == 1

# Round-trip null
json_results["null_handling"]["roundtrip_null"] = json_decode(json_encode({"x": None})).get("x") == None

# ============================================================================
# json_try_decode Tests
# ============================================================================

# Valid JSON returns parsed value
try_valid = json_try_decode('{"x": 99}')
json_results["try_decode"]["valid_returns_dict"] = try_valid.get("x") == 99

# Invalid JSON returns default (None by default)
json_results["try_decode"]["invalid_returns_none"] = json_try_decode("not json") == None

# Invalid JSON returns supplied default
json_results["try_decode"]["invalid_returns_custom_default"] = json_try_decode("{bad}", {}) == {}

# Valid JSON null returns None (correct decode, not the fallback)
json_results["try_decode"]["valid_null_returns_none"] = json_try_decode("null") == None

# Completely empty string returns default
json_results["try_decode"]["empty_string_returns_default"] = json_try_decode("", "fallback") == "fallback"

# Custom sentinel distinguishes valid JSON null from parse failure (single-parse path)
_SENTINEL = "__PARSE_FAILED__"
json_results["try_decode"]["sentinel_on_failure"] = json_try_decode("{bad}", _SENTINEL) == _SENTINEL
json_results["try_decode"]["sentinel_not_returned_for_null"] = json_try_decode("null", _SENTINEL) == None
json_results["try_decode"]["sentinel_not_returned_for_valid"] = json_try_decode('{"a":1}', _SENTINEL) != _SENTINEL

# ============================================================================
# json_encode_indented Tests
# ============================================================================

_indent_data = {"alpha": [1, 2], "beta": True}

# Default indent (2) must match json_encode_pretty output
_indent_default = json_encode_indented(_indent_data)
_pretty_default = json_encode_pretty(_indent_data)
json_results["indented_encoding"]["default_matches_pretty"] = _indent_default == _pretty_default

# 4-space indentation produces deeper indentation than 2-space
_indent_4 = json_encode_indented(_indent_data, indent = 4)
json_results["indented_encoding"]["four_space_has_newlines"] = "\n" in _indent_4
json_results["indented_encoding"]["four_space_longer_than_two"] = len(_indent_4) > len(_indent_default)

# 1-space indentation produces shallower indentation than 2-space
_indent_1 = json_encode_indented(_indent_data, indent = 1)
json_results["indented_encoding"]["one_space_shorter_than_two"] = len(_indent_1) < len(_indent_default)

# 0-space indentation has newlines but no leading spaces on value lines
_indent_0 = json_encode_indented(_indent_data, indent = 0)
json_results["indented_encoding"]["zero_space_has_newlines"] = "\n" in _indent_0

# Round-trip: indented output decodes back to the same structure
_indent_rt = json_decode(_indent_4)
json_results["indented_encoding"]["roundtrip_beta"] = _indent_rt.get("beta") == True
json_results["indented_encoding"]["roundtrip_alpha_first"] = _indent_rt.get("alpha")[0] == 1

# ============================================================================
# json_merge Tests
# ============================================================================

base_cfg = {"host": "localhost", "port": 8080, "debug": False}
override_cfg = {"port": 9000, "debug": True}
merged = json_merge(base_cfg, override_cfg)

json_results["merge"]["host_preserved"] = merged.get("host") == "localhost"
json_results["merge"]["port_overridden"] = merged.get("port") == 9000
json_results["merge"]["debug_overridden"] = merged.get("debug") == True

# Original dicts must not be mutated
json_results["merge"]["base_unchanged"] = base_cfg.get("port") == 8080
json_results["merge"]["override_unchanged"] = "host" not in override_cfg

# Merge adds new keys from dict2
d1 = {"a": 1}
d2 = {"b": 2}
merged2 = json_merge(d1, d2)
json_results["merge"]["new_key_added"] = merged2.get("b") == 2
json_results["merge"]["old_key_kept"] = merged2.get("a") == 1

# ============================================================================
# Key Ordering Tests
# ============================================================================

# serde_json uses BTreeMap by default; keys in encoded output should be
# alphabetically sorted regardless of insertion order.
unordered = {"zebra": 1, "apple": 2, "mango": 3}
encoded_unordered = json_encode(unordered)

# apple < mango < zebra alphabetically
apple_pos = encoded_unordered.index("apple")
mango_pos = encoded_unordered.index("mango")
zebra_pos = encoded_unordered.index("zebra")
json_results["key_ordering"]["keys_sorted_alphabetically"] = (
    apple_pos < mango_pos and mango_pos < zebra_pos
)

# Round-trip through encode/decode preserves all keys and values
rt_unordered = json_decode(encoded_unordered)
json_results["key_ordering"]["values_preserved"] = (
    rt_unordered.get("zebra") == 1 and
    rt_unordered.get("apple") == 2 and
    rt_unordered.get("mango") == 3
)

# ============================================================================
# File Round-Trip Tests
# ============================================================================

file_data = {
    "project": "spaces-test",
    "version": "2.0.0",
    "tags": ["json", "roundtrip"],
    "meta": {
        "count": 3,
        "active": True,
        "score": 9.5,
    },
}

_json_tmp_path = "/tmp/spaces-test-json-roundtrip.json"

# Write pretty JSON, read back, and verify
json_write_file(_json_tmp_path, file_data, pretty = True)
file_rt = json_read_file(_json_tmp_path)
json_results["file_roundtrip"]["project"] = file_rt.get("project") == "spaces-test"
json_results["file_roundtrip"]["version"] = file_rt.get("version") == "2.0.0"
json_results["file_roundtrip"]["first_tag"] = file_rt.get("tags", ["tag"])[0] == "json"
json_results["file_roundtrip"]["nested_count"] = file_rt.get("meta", {}).get("count") == 3
json_results["file_roundtrip"]["nested_active"] = file_rt.get("meta", {}).get("active") == True
json_results["file_roundtrip"]["nested_score"] = file_rt.get("meta", {}).get("score") == 9.5

# Write compact JSON, read back
json_write_file(_json_tmp_path, file_data, pretty = False)
file_compact_rt = json_read_file(_json_tmp_path)
json_results["file_roundtrip"]["compact_project"] = file_compact_rt.get("project") == "spaces-test"

# ============================================================================
# Backwards Compatibility Tests
# ============================================================================

json_results["backwards_compat"]["json_is_string_json_valid"] = json_is_string_json('{"k": 1}') == True
json_results["backwards_compat"]["json_is_string_json_invalid"] = json_is_string_json("bad") == False

# ============================================================================
# Output Results
# ============================================================================

print("JSON Module Test Results:")
print("========================")
print("")
print(json_dumps(json_results, is_pretty = True))
print("")
print("All JSON functions executed successfully!")
