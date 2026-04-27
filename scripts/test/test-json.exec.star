#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_decode",
    "json_dumps",
    "json_encode",
    "json_encode_compact",
    "json_encode_pretty",
    "json_is_valid",
    "json_loads",
)

# JSON module test results
json_results = {
    "decode": {},
    "encode": {},
    "validation": {},
    "compact_encoding": {},
    "pretty_encoding": {},
}

# ============================================================================
# JSON Module Tests
# ============================================================================

# Test basic JSON decoding
simple_json = '{"name": "Alice", "age": 30, "active": true}'
decoded_simple = json_decode(simple_json)
json_results["decode"]["simple_object"] = decoded_simple
json_results["decode"]["access_name"] = decoded_simple["name"]
json_results["decode"]["access_age"] = decoded_simple["age"]
json_results["decode"]["access_active"] = decoded_simple["active"]

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
json_results["decode"]["nested_user_id"] = decoded_nested["user"]["id"]
json_results["decode"]["nested_email"] = decoded_nested["user"]["profile"]["email"]
json_results["decode"]["nested_first_tag"] = decoded_nested["user"]["profile"]["tags"][0]

# Test JSON array
array_json = '[1, 2, 3, "four", 5.5]'
decoded_array = json_decode(array_json)
json_results["decode"]["array_first"] = decoded_array[0]
json_results["decode"]["array_third"] = decoded_array[2]
json_results["decode"]["array_string"] = decoded_array[3]

# Test json_loads alias
json_results["decode"]["json_loads_alias"] = json_loads(simple_json)["name"]

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
json_results["encode"]["roundtrip_project"] = decoded_roundtrip["project"]
json_results["encode"]["roundtrip_version"] = decoded_roundtrip["version"]
json_results["encode"]["roundtrip_first_feature"] = decoded_roundtrip["features"][0]
json_results["encode"]["roundtrip_created"] = decoded_roundtrip["metadata"]["created"]

# Test json_dumps alias with pretty flag (backwards compatible parameter name)
dumped_compact = json_dumps(data_to_encode, is_pretty = False)
dumped_pretty = json_dumps(data_to_encode, is_pretty = True)
json_results["encode"]["dumps_compact_matches"] = dumped_compact == compact_encoded
json_results["encode"]["dumps_pretty_matches"] = dumped_pretty == pretty_encoded

# ============================================================================
# Output Results
# ============================================================================

print("JSON Module Test Results:")
print("========================")
print("")
print(json_dumps(json_results, is_pretty = True))
print("")
print("All JSON functions executed successfully!")
