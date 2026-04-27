#!/usr/bin/env spaces

load("//@star/sdk/star/std/fs.star", "fs_write_text")
load(
    "//@star/sdk/star/std/hash.star",
    "hash_base64_decode",
    "hash_base64_encode",
    "hash_compute_sha256_from_file",
    "hash_compute_sha256_from_string",
    "hash_crc32",
    "hash_crc32_file",
    "hash_hex_decode",
    "hash_hex_encode",
    "hash_md5",
    "hash_md5_file",
    "hash_sha1",
    "hash_sha1_file",
    "hash_sha256",
    "hash_sha256_file",
    "hash_sha512",
    "hash_sha512_file",
)
load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)

# Create a test file for file-based hashing
test_file = "test-hash-data.txt"
test_content = "Hello, World! This is test data for hashing."
fs_write_text(test_file, test_content)

test_string = "hello world"

# Hash module test results
results = {
    "string_hashing": {},
    "file_hashing": {},
    "encoding": {},
    "backward_compatibility": {},
}

# ============================================================================
# String Hashing Tests
# ============================================================================

results["string_hashing"]["sha256"] = hash_sha256(test_string)
results["string_hashing"]["sha512"] = hash_sha512(test_string)
results["string_hashing"]["sha1"] = hash_sha1(test_string)
results["string_hashing"]["md5"] = hash_md5(test_string)
results["string_hashing"]["crc32"] = hash_crc32(test_string)

# ============================================================================
# File Hashing Tests
# ============================================================================

results["file_hashing"]["sha256"] = hash_sha256_file(test_file)
results["file_hashing"]["sha512"] = hash_sha512_file(test_file)
results["file_hashing"]["sha1"] = hash_sha1_file(test_file)
results["file_hashing"]["md5"] = hash_md5_file(test_file)
results["file_hashing"]["crc32"] = hash_crc32_file(test_file)

# ============================================================================
# Encoding/Decoding Tests
# ============================================================================

test_data = "Hello"
hex_encoded = hash_hex_encode(test_data)
hex_decoded = hash_hex_decode(hex_encoded)
results["encoding"]["hex_encode"] = hex_encoded
results["encoding"]["hex_decode"] = hex_decoded
results["encoding"]["hex_roundtrip_match"] = hex_decoded == test_data

b64_encoded = hash_base64_encode(test_data)
b64_decoded = hash_base64_decode(b64_encoded)
results["encoding"]["base64_encode"] = b64_encoded
results["encoding"]["base64_decode"] = b64_decoded
results["encoding"]["base64_roundtrip_match"] = b64_decoded == test_data

# ============================================================================
# Backward Compatibility Tests
# ============================================================================

results["backward_compatibility"]["compute_sha256_from_string"] = hash_compute_sha256_from_string(test_string)
results["backward_compatibility"]["compute_sha256_from_file"] = hash_compute_sha256_from_file(test_file)

# Verify backward compatibility functions match new functions
sha256_match = results["backward_compatibility"]["compute_sha256_from_string"] == results["string_hashing"]["sha256"]
sha256_file_match = results["backward_compatibility"]["compute_sha256_from_file"] == results["file_hashing"]["sha256"]
results["backward_compatibility"]["sha256_string_match"] = sha256_match
results["backward_compatibility"]["sha256_file_match"] = sha256_file_match

# ============================================================================
# Output Results
# ============================================================================

print("Hash Module Test Results:")
print("========================")
print("")
print(json_dumps(results, is_pretty = True))
print("")
print("All hash functions executed successfully!")
