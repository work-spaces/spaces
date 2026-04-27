#!/usr/bin/env spaces

load("//@star/sdk/star/std/fs.star", "fs_write_text")
load(
    "//@star/sdk/star/std/hash.star",
    "hash_base64_decode",
    "hash_base64_encode",
    "hash_blake3",
    "hash_blake3_file",
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

# ============================================================================
# Assertion helper
# ============================================================================

def assert_eq(label, actual, expected):
    """Fail loudly if actual != expected."""
    if actual != expected:
        fail("ASSERTION FAILED [{}]\n  expected: {}\n  actual:   {}".format(
            label,
            expected,
            actual,
        ))

# ============================================================================
# Fixture setup
# ============================================================================

# The fixture file is written fresh every run so the expected hashes are always
# tied to exactly this content string.
test_file = "test-hash-data.txt"
test_content = "Hello, World! This is test data for hashing."
fs_write_text(test_file, test_content)

test_string = "hello world"

# ============================================================================
# Known-good reference digests
#
# Verified by running the implementation against canonical test vectors and
# cross-checking with standard command-line tools:
#
#   echo -n "hello world"  | sha256sum
#   echo -n "hello world"  | sha512sum
#   echo -n "hello world"  | sha1sum
#   echo -n "hello world"  | md5sum
#   printf '%s' "Hello, World! This is test data for hashing." | sha256sum
#   ...etc
# ============================================================================

KNOWN = {
    # --- string: "hello world" ---
    "str_sha256": "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
    "str_sha512": "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f",
    "str_sha1": "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed",
    "str_md5": "5eb63bbbe01eeed093cb22bb8f5acdc3",
    "str_crc32": "0d4a1185",
    "str_blake3": "d74981efa70a0c880b8d8c1985d075dbcbf679b99a5f9914e5aaf96b831a9e24",

    # --- file: "Hello, World! This is test data for hashing." ---
    "file_sha256": "d2ab706bae601b2e78edd844770e845e8a3dc78259232a38e5dfe7aff415eeb3",
    "file_sha512": "e4eea5fe509f4b0eb9fb32d5aecf0feb3d01aa1a75745f4275e6343d055de0acf75bc536de72c922c934dcd3e29009cfaf0fe25adc2622433e541cb84f1eb935",
    "file_sha1": "22fe0f8275b4659315a2f5bb708297a5ede1a2b3",
    "file_md5": "ac6a34604c4ff93a6c99fe946e64f1da",
    "file_crc32": "6bfdf9ce",
    "file_blake3": "877e7036104d4dda1b1bc1b1705ff1580f906044aa843869b20ed0e2045313e6",

    # --- hex encode of "Hello" ---
    "hex_hello": "48656c6c6f",

    # --- base64 encode of "Hello" ---
    "b64_hello": "SGVsbG8=",
}

# ============================================================================
# String hashing — assert against known values
# ============================================================================

assert_eq("sha256(string)", hash_sha256(test_string), KNOWN["str_sha256"])
assert_eq("sha512(string)", hash_sha512(test_string), KNOWN["str_sha512"])
assert_eq("sha1(string)", hash_sha1(test_string), KNOWN["str_sha1"])
assert_eq("md5(string)", hash_md5(test_string), KNOWN["str_md5"])
assert_eq("crc32(string)", hash_crc32(test_string), KNOWN["str_crc32"])

# ============================================================================
# File hashing — assert against known values
# ============================================================================

assert_eq("sha256(file)", hash_sha256_file(test_file), KNOWN["file_sha256"])
assert_eq("sha512(file)", hash_sha512_file(test_file), KNOWN["file_sha512"])
assert_eq("sha1(file)", hash_sha1_file(test_file), KNOWN["file_sha1"])
assert_eq("md5(file)", hash_md5_file(test_file), KNOWN["file_md5"])
assert_eq("crc32(file)", hash_crc32_file(test_file), KNOWN["file_crc32"])

# ============================================================================
# BLAKE3 — string and file
#
# BLAKE3 digests are 64 hex characters (256-bit output).
# The exact values are captured from the reference implementation and stored
# below after the first verified run.
# ============================================================================

blake3_str = hash_blake3(test_string)
blake3_file = hash_blake3_file(test_file)

assert_eq("blake3(string)", blake3_str, KNOWN["str_blake3"])
assert_eq("blake3(file)", blake3_file, KNOWN["file_blake3"])

# Structural checks independent of exact digest value (length + uniqueness):
if len(blake3_str) != 64:
    fail("BLAKE3 string digest should be 64 hex chars, got: {}".format(len(blake3_str)))
if len(blake3_file) != 64:
    fail("BLAKE3 file digest should be 64 hex chars, got: {}".format(len(blake3_file)))
if blake3_str == blake3_file:
    fail("BLAKE3 digest for string and file should differ (different inputs)")

# ============================================================================
# Encoding / decoding — roundtrip and known-value checks
# ============================================================================

hex_encoded = hash_hex_encode("Hello")
hex_decoded = hash_hex_decode(hex_encoded)

assert_eq("hex_encode(Hello)", hex_encoded, KNOWN["hex_hello"])
assert_eq("hex_decode roundtrip", hex_decoded, "Hello")

b64_encoded = hash_base64_encode("Hello")
b64_decoded = hash_base64_decode(b64_encoded)

assert_eq("base64_encode(Hello)", b64_encoded, KNOWN["b64_hello"])
assert_eq("base64_decode roundtrip", b64_decoded, "Hello")

# Additional roundtrip over a richer string to catch off-by-one padding issues.
roundtrip_input = "spaces hash module roundtrip check 1234"
assert_eq(
    "hex roundtrip (long string)",
    hash_hex_decode(hash_hex_encode(roundtrip_input)),
    roundtrip_input,
)
assert_eq(
    "base64 roundtrip (long string)",
    hash_base64_decode(hash_base64_encode(roundtrip_input)),
    roundtrip_input,
)

# ============================================================================
# Backward-compatibility aliases
# ============================================================================

assert_eq(
    "compute_sha256_from_string == sha256(string)",
    hash_compute_sha256_from_string(test_string),
    KNOWN["str_sha256"],
)
assert_eq(
    "compute_sha256_from_file == sha256(file)",
    hash_compute_sha256_from_file(test_file),
    KNOWN["file_sha256"],
)

# ============================================================================
# Output summary
# ============================================================================

results = {
    "string_hashing": {
        "sha256": hash_sha256(test_string),
        "sha512": hash_sha512(test_string),
        "sha1": hash_sha1(test_string),
        "md5": hash_md5(test_string),
        "crc32": hash_crc32(test_string),
        "blake3": blake3_str,
    },
    "file_hashing": {
        "sha256": hash_sha256_file(test_file),
        "sha512": hash_sha512_file(test_file),
        "sha1": hash_sha1_file(test_file),
        "md5": hash_md5_file(test_file),
        "crc32": hash_crc32_file(test_file),
        "blake3": blake3_file,
    },
    "encoding": {
        "hex_encode": hex_encoded,
        "hex_decode": hex_decoded,
        "hex_roundtrip_match": hex_decoded == "Hello",
        "base64_encode": b64_encoded,
        "base64_decode": b64_decoded,
        "base64_roundtrip_match": b64_decoded == "Hello",
    },
    "backward_compatibility": {
        "compute_sha256_from_string": hash_compute_sha256_from_string(test_string),
        "compute_sha256_from_file": hash_compute_sha256_from_file(test_file),
        "sha256_string_match": hash_compute_sha256_from_string(test_string) == hash_sha256(test_string),
        "sha256_file_match": hash_compute_sha256_from_file(test_file) == hash_sha256_file(test_file),
    },
}

print("Hash Module Test Results:")
print("========================")
print("")
print(json_dumps(results, is_pretty = True))
print("")
print("All assertions passed!")
