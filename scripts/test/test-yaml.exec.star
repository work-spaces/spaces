#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/fs.star",
    "fs_read_text",
    "fs_write_text",
)
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
    "yaml_is_valid",
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
    "invalid_input": {},
    "anchors_aliases": {},
    "multi_document": {},
    "yaml_is_valid": {},
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
yaml_results["basic_decoding"]["simple_parse"] = parsed.get("app").get("name") == "TestApp"
yaml_results["basic_decoding"]["parse_version"] = parsed.get("app").get("version") == "1.0.0"
yaml_results["basic_decoding"]["parse_bool"] = parsed.get("app").get("debug") == False

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
yaml_results["basic_decoding"]["parse_lists"] = list_parsed.get("ports") == [80, 8080, 8443]
yaml_results["basic_decoding"]["parse_string_lists"] = len(list_parsed.get("features")) == 3
yaml_results["basic_decoding"]["list_first_element"] = list_parsed.get("features")[0] == "auth"

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
yaml_results["complex_structures"]["nested_access"] = complex_parsed.get("project").get("database").get("host") == "localhost"
yaml_results["complex_structures"]["deep_nested_access"] = complex_parsed.get("project").get("database").get("connection").get("timeout") == 30
yaml_results["complex_structures"]["multiple_sections"] = complex_parsed.get("project").get("server").get("workers") == 4

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
yaml_results["encoding"]["roundtrip_name"] = decoded_round.get("name") == original.get("name")
yaml_results["encoding"]["roundtrip_bool"] = decoded_round.get("enabled") == original.get("enabled")
yaml_results["encoding"]["roundtrip_number"] = decoded_round.get("count") == 42
yaml_results["encoding"]["roundtrip_list"] = "items" in decoded_round and len(decoded_round.get("items")) == 3

# Test decoding with valid data
valid_yaml_data = """name: Test
value: 123
active: true
"""
safe_decode_valid = yaml_try_decode(valid_yaml_data)
yaml_results["error_handling"]["try_decode_valid_returns_data"] = safe_decode_valid.get("name") == "Test"
yaml_results["error_handling"]["try_decode_with_default_works"] = yaml_try_decode(valid_yaml_data, default = {}).get("name") == "Test"

# ============================================================================
# Invalid input / error handling
# ============================================================================

# yaml_try_decode should return None (default) for invalid YAML
bad_yaml_1 = ": - invalid: {yaml:"
yaml_results["invalid_input"]["try_decode_bad_returns_none"] = yaml_try_decode(bad_yaml_1) == None

# yaml_try_decode with custom default on bad input
yaml_results["invalid_input"]["try_decode_bad_returns_default_dict"] = yaml_try_decode(bad_yaml_1, default = {}) == {}
yaml_results["invalid_input"]["try_decode_bad_returns_default_string"] = yaml_try_decode(bad_yaml_1, default = "fallback") == "fallback"

# yaml_try_decode should return None (not the default) for a valid YAML null document:
# the parse succeeded — the document value is null — so default must NOT be substituted.
yaml_results["invalid_input"]["try_decode_null_doc_returns_none"] = yaml_try_decode("~") == None
yaml_results["invalid_input"]["try_decode_null_doc_ignores_default"] = yaml_try_decode("~", default = {}) == None

# yaml_try_decode returns actual data for valid input even when default is given
yaml_results["invalid_input"]["try_decode_valid_ignores_default"] = yaml_try_decode("x: 1", default = {}).get("x") == 1

# ============================================================================
# Anchors and aliases
# YAML anchors (&name) and aliases (*name) are resolved transparently by
# serde_yaml.  The parsed result is the fully-expanded value; anchors are
# not visible after parsing.
#
# NOTE: The YAML 1.1 merge key "<<:" is NOT supported by serde_yaml 0.9
# (which targets YAML 1.2).  "<<" is treated as a regular string key rather
# than triggering a merge.  Callers who need merge-key behaviour must
# perform the merge manually in Starlark (e.g. with yaml_merge()).
# ============================================================================

# Scalar alias: the alias resolves to the same value as the anchor.
scalar_anchor_yaml = """canonical: &canon "hello world"
copy: *canon
"""
scalar_parsed = yaml_decode(scalar_anchor_yaml)
yaml_results["anchors_aliases"]["scalar_alias_resolved"] = scalar_parsed.get("copy") == "hello world"
yaml_results["anchors_aliases"]["scalar_anchor_original"] = scalar_parsed.get("canonical") == "hello world"

# Mapping alias: *ref resolves to a full copy of the anchored mapping.
mapping_anchor_yaml = """defaults: &defaults
  port: 8080
  debug: false

replica: *defaults
"""
mapping_parsed = yaml_decode(mapping_anchor_yaml)
yaml_results["anchors_aliases"]["mapping_anchor_port"] = mapping_parsed.get("defaults").get("port") == 8080
yaml_results["anchors_aliases"]["mapping_alias_port"] = mapping_parsed.get("replica").get("port") == 8080
yaml_results["anchors_aliases"]["mapping_alias_debug"] = mapping_parsed.get("replica").get("debug") == False

# Sequence alias
seq_anchor_yaml = """base_tags: &tags
  - prod
  - critical

service:
  name: myapp
  tags: *tags
"""
seq_parsed = yaml_decode(seq_anchor_yaml)
yaml_results["anchors_aliases"]["sequence_alias_resolved"] = seq_parsed.get("service").get("tags") == ["prod", "critical"]

# <<: merge key is treated as a literal key (YAML 1.1 feature, not supported in YAML 1.2 / serde_yaml 0.9)
merge_key_yaml = """defaults: &defaults
  port: 8080

server:
  <<: *defaults
  host: localhost
"""
merge_key_parsed = yaml_decode(merge_key_yaml)

# "<<" is a literal key — port is NOT merged into server
yaml_results["anchors_aliases"]["merge_key_not_expanded"] = "port" not in merge_key_parsed.get("server")
yaml_results["anchors_aliases"]["merge_key_literal_key_present"] = "<<" in merge_key_parsed.get("server")
yaml_results["anchors_aliases"]["merge_key_own_key_preserved"] = merge_key_parsed.get("server").get("host") == "localhost"

# ============================================================================
# Multi-document YAML
# serde_yaml 0.9 (targeting YAML 1.2) does NOT support multi-document streams.
# A string containing multiple "---"-separated documents causes a parse error.
# yaml_decode() will raise; yaml_try_decode() returns the default value.
# To process multiple documents, split the string on "---" first.
# ============================================================================

multi_doc_yaml = """key: first_doc
value: 1
---
key: second_doc
value: 2
"""

# yaml_try_decode returns None (parse failed) for multi-document input
multi_doc_try = yaml_try_decode(multi_doc_yaml)
yaml_results["multi_document"]["multi_doc_causes_parse_error"] = multi_doc_try == None

# yaml_try_decode with a custom default also returns that default
multi_doc_try_default = yaml_try_decode(multi_doc_yaml, default = {"error": True})
yaml_results["multi_document"]["multi_doc_returns_custom_default"] = multi_doc_try_default.get("error") == True

# yaml_is_valid returns False for multi-document input
yaml_results["multi_document"]["multi_doc_is_not_valid"] = yaml_is_valid(multi_doc_yaml) == False

# Single-document YAML with a leading "---" marker IS valid
single_doc_with_marker = """---
key: only_doc
value: 42
"""
single_doc_parsed = yaml_decode(single_doc_with_marker)
yaml_results["multi_document"]["single_doc_with_marker_ok"] = single_doc_parsed.get("key") == "only_doc"
yaml_results["multi_document"]["single_doc_with_marker_value"] = single_doc_parsed.get("value") == 42

# ============================================================================
# yaml_is_valid
# ============================================================================

yaml_results["yaml_is_valid"]["valid_simple"] = yaml_is_valid("key: value") == True
yaml_results["yaml_is_valid"]["valid_list"] = yaml_is_valid("- a\n- b\n- c") == True
yaml_results["yaml_is_valid"]["valid_empty_string"] = yaml_is_valid("") == True
yaml_results["yaml_is_valid"]["valid_null"] = yaml_is_valid("~") == True
yaml_results["yaml_is_valid"]["invalid_bad_indent"] = yaml_is_valid(": - {broken:") == False

# ============================================================================
# File round-trip
# Encode a dict to YAML, write to a temp file, read back, decode, and verify.
# ============================================================================

file_roundtrip_data = {
    "service": "yaml-test",
    "version": "2.0",
    "enabled": True,
    "replicas": 3,
    "tags": ["prod", "critical"],
}

roundtrip_yaml_str = yaml_encode(file_roundtrip_data)
fs_write_text("build/yaml-roundtrip-test.yaml", roundtrip_yaml_str)
roundtrip_read_back = fs_read_text("build/yaml-roundtrip-test.yaml")
roundtrip_decoded = yaml_decode(roundtrip_read_back)

if roundtrip_decoded != None:
    yaml_results["encoding"]["file_roundtrip_service"] = roundtrip_decoded.get("service") == "yaml-test"
    yaml_results["encoding"]["file_roundtrip_version"] = roundtrip_decoded.get("version") == "2.0"
    yaml_results["encoding"]["file_roundtrip_bool"] = roundtrip_decoded.get("enabled") == True
    yaml_results["encoding"]["file_roundtrip_int"] = roundtrip_decoded.get("replicas") == 3
    yaml_results["encoding"]["file_roundtrip_list"] = roundtrip_decoded.get("tags") == ["prod", "critical"]

# ============================================================================
# Merging
# ============================================================================

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
yaml_results["merging"]["merge_preserves_base"] = yaml_merged.get("server") == "localhost"
yaml_results["merging"]["merge_overrides_values"] = yaml_merged.get("port") == 9000
yaml_results["merging"]["merge_adds_override"] = yaml_merged.get("debug") == True
yaml_results["merging"]["merge_keeps_non_overridden"] = yaml_merged.get("workers") == 4

# ============================================================================
# Backward compatibility - original function names
# ============================================================================

compat_yaml = """app: MyApp
version: 1.0
"""
compat_yaml_parsed = yaml_parse_string(compat_yaml)
yaml_results["backward_compatibility"]["parse_string_works"] = compat_yaml_parsed.get("app") == "MyApp"

compat_yaml_dict = {"title": "Test", "enabled": True}
compat_yaml_encoded = yaml_to_string(compat_yaml_dict)
yaml_results["backward_compatibility"]["to_string_works"] = "title" in compat_yaml_encoded

# Test yaml_loads and yaml_dumps aliases
loads_yaml = """config:
  name: LoadTest
  port: 3000
"""
loaded = yaml_loads(loads_yaml)
yaml_results["backward_compatibility"]["loads_works"] = loaded.get("config").get("name") == "LoadTest"

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
