"""
Spaces Hash Module

This module provides ergonomic wrappers around cryptographic hashing operations.
It supports common algorithms including MD5, SHA1, SHA256, SHA512, and CRC32,
as well as hex and base64 encoding/decoding utilities.

All hash functions return hexadecimal-encoded strings. Functions are available
for both string and file inputs, allowing you to compute checksums for integrity
verification, deduplication, and data validation.

Examples:
    # Compute SHA256 hash of a file
    checksum = hash_sha256_file("data/model.bin")

    # Compute MD5 hash of a string
    digest = hash_md5("my-data")

    # Verify file integrity
    expected = "abc123def456"
    actual = hash_sha256_file("important.dat")
    if actual == expected:
        print("File is valid")

    # Encode and decode with hex
    encoded = hash_hex_encode("Hello")
    decoded = hash_hex_decode(encoded)
"""

# ============================================================================
# SHA256 - Secure Hash Algorithm 256-bit
# ============================================================================

def hash_sha256(data: str) -> str:
    """
    Compute the SHA256 hash of a string.

    SHA256 is a secure cryptographic hash function that produces a 256-bit
    (64 hexadecimal character) digest. It is commonly used for checksums,
    digital signatures, and integrity verification.

    Args:
        data: The string to hash

    Returns:
        str: The SHA256 hash as a hexadecimal-encoded string (64 characters)

    Raises:
        Error: If hashing fails

    Examples:
        # Hash a simple string
        digest = hash_sha256("password123")
        print(f"Hash: {digest}")

        # Verify data integrity
        original = "important-data"
        checksum = hash_sha256(original)
        # Later, verify:
        if hash_sha256(original) == checksum:
            print("Data is unchanged")
    """
    return hash.sha256_string(data)

def hash_sha256_file(file_path: str) -> str:
    """
    Compute the SHA256 hash of a file.

    Reads the entire file and computes its SHA256 digest. Useful for
    verifying file integrity, detecting changes, or creating checksums.

    Args:
        file_path: Path to the file to hash (relative to workspace root)

    Returns:
        str: The SHA256 hash as a hexadecimal-encoded string (64 characters)

    Raises:
        Error: If the file cannot be read or hashing fails

    Examples:
        # Compute checksum of a downloaded file
        checksum = hash_sha256_file("downloads/archive.tar.gz")
        print(f"File SHA256: {checksum}")

        # Verify file hasn't been corrupted
        expected_checksum = "abc123...def456"
        actual_checksum = hash_sha256_file("backup.zip")
        if actual_checksum == expected_checksum:
            print("Backup is valid")
        else:
            print("Backup may be corrupted!")

        # Create a file manifest
        files = ["src/main.py", "src/utils.py", "config.json"]
        for path in files:
            print(f"{path}: {hash_sha256_file(path)}")
    """
    return hash.sha256_file(file_path)

def hash_compute_sha256_from_string(value: str) -> str:
    """
    Compute SHA256 hash of a string. Backward compatibility alias.

    This function is provided for backward compatibility. Prefer hash_sha256()
    for new code.

    Args:
        value: The string content to hash

    Returns:
        str: Hexadecimal-encoded SHA256 digest (64 characters)

    Examples:
        digest = hash_compute_sha256_from_string("my-unique-identity")
        print(f"String Hash: {digest}")
    """
    return hash.sha256_string(value)

def hash_compute_sha256_from_file(file_path: str) -> str:
    """
    Compute SHA256 hash of a file. Backward compatibility alias.

    This function is provided for backward compatibility. Prefer hash_sha256_file()
    for new code.

    Args:
        file_path: The path to the file to hash

    Returns:
        str: Hexadecimal-encoded SHA256 digest (64 characters)

    Examples:
        checksum = hash_compute_sha256_from_file("data/model.bin")
        print(f"File SHA-256: {checksum}")
    """
    return hash.compute_sha256_from_file(file_path)

# ============================================================================
# SHA1 - Secure Hash Algorithm 160-bit
# ============================================================================

def hash_sha1(data: str) -> str:
    """
    Compute the SHA1 hash of a string.

    SHA1 produces a 160-bit (40 hexadecimal character) digest. While still
    widely used, SHA1 is considered cryptographically weak. Prefer SHA256
    for new applications requiring security guarantees.

    Args:
        data: The string to hash

    Returns:
        str: The SHA1 hash as a hexadecimal-encoded string (40 characters)

    Raises:
        Error: If hashing fails

    Examples:
        # Compute SHA1 of a string
        digest = hash_sha1("example")

        # Generate a commit-like identifier
        commit_id = hash_sha1("version-1.0.0")
    """
    return hash.sha1_string(data)

def hash_sha1_file(file_path: str) -> str:
    """
    Compute the SHA1 hash of a file.

    Reads the entire file and computes its SHA1 digest. Commonly used for
    Git-like identifiers and legacy systems.

    Args:
        file_path: Path to the file to hash (relative to workspace root)

    Returns:
        str: The SHA1 hash as a hexadecimal-encoded string (40 characters)

    Raises:
        Error: If the file cannot be read or hashing fails

    Examples:
        # Compute SHA1 of a file
        digest = hash_sha1_file("data.bin")

        # Check against known SHA1
        known_sha1 = "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        if hash_sha1_file("file.dat") == known_sha1:
            print("File matches known version")
    """
    return hash.sha1_file(file_path)

# ============================================================================
# SHA512 - Secure Hash Algorithm 512-bit
# ============================================================================

def hash_sha512(data: str) -> str:
    """
    Compute the SHA512 hash of a string.

    SHA512 produces a 512-bit (128 hexadecimal character) digest. It provides
    stronger security guarantees than SHA256 but is slower and produces larger
    hashes. Use when maximum security is required.

    Args:
        data: The string to hash

    Returns:
        str: The SHA512 hash as a hexadecimal-encoded string (128 characters)

    Raises:
        Error: If hashing fails

    Examples:
        # Compute SHA512 of sensitive data
        digest = hash_sha512("secure-password")

        # Create a strong fingerprint
        fingerprint = hash_sha512("user-session-token-12345")
    """
    return hash.sha512_string(data)

def hash_sha512_file(file_path: str) -> str:
    """
    Compute the SHA512 hash of a file.

    Reads the entire file and computes its SHA512 digest. Provides the
    strongest standard hash-based integrity verification among common algorithms.

    Args:
        file_path: Path to the file to hash (relative to workspace root)

    Returns:
        str: The SHA512 hash as a hexadecimal-encoded string (128 characters)

    Raises:
        Error: If the file cannot be read or hashing fails

    Examples:
        # Compute SHA512 of a critical file
        digest = hash_sha512_file("secret-keys.enc")

        # Create a strong checksum for archival
        archive_checksum = hash_sha512_file("backup-2024.tar.gz")
        print(f"Archive verified with: {archive_checksum}")
    """
    return hash.sha512_file(file_path)

# ============================================================================
# BLAKE3 - Fast cryptographic hash function
# ============================================================================

def hash_blake3(data: str) -> str:
    """
    Compute the BLAKE3 hash of a string.

    BLAKE3 is a modern cryptographic hash function that is significantly faster
    than SHA-256/SHA-512 while providing equivalent security guarantees. It
    produces a 256-bit (64 hexadecimal character) digest by default.

    Args:
        data: The string to hash

    Returns:
        str: The BLAKE3 hash as a hexadecimal-encoded string (64 characters)

    Raises:
        Error: If hashing fails

    Examples:
        # Hash a simple string
        digest = hash_blake3("my-data")
        print(f"BLAKE3: {digest}")

        # Use as a fast integrity fingerprint
        fingerprint = hash_blake3("artifact-v1.2.3")
    """
    return hash.blake3_string(data)

def hash_blake3_file(file_path: str) -> str:
    """
    Compute the BLAKE3 hash of a file.

    Reads the file in 64 KiB streaming chunks so large files never cause
    excessive memory consumption.  Returns a 256-bit (64 hexadecimal
    character) digest.

    Args:
        file_path: Path to the file to hash (relative to workspace root)

    Returns:
        str: The BLAKE3 hash as a hexadecimal-encoded string (64 characters)

    Raises:
        Error: If the file cannot be read or hashing fails

    Examples:
        # Compute BLAKE3 checksum of a large artifact
        digest = hash_blake3_file("downloads/toolchain.tar.gz")
        print(f"BLAKE3: {digest}")

        # Verify file integrity
        expected = "d74981efa70a0c880b8d8c1985d075dbcbf679b99a5f9914e5aaf96b831a9e24"
        if hash_blake3_file("release.zip") == expected:
            print("File is valid")
    """
    return hash.blake3_file(file_path)

# ============================================================================
# MD5 - Message Digest Algorithm 5
# ============================================================================

def hash_md5(data: str) -> str:
    """
    Compute the MD5 hash of a string.

    MD5 produces a 128-bit (32 hexadecimal character) digest. MD5 is
    cryptographically broken and should NOT be used for security purposes.
    It is provided for legacy compatibility and non-critical checksums only.

    Args:
        data: The string to hash

    Returns:
        str: The MD5 hash as a hexadecimal-encoded string (32 characters)

    Raises:
        Error: If hashing fails

    Examples:
        # Generate a simple checksum (not for security)
        checksum = hash_md5("data")

        # Create a cache key (non-critical)
        cache_key = hash_md5("user-123-profile")

    Note:
        MD5 is deprecated for cryptographic use. For security purposes,
        use hash_sha256() or hash_sha512() instead.
    """
    return hash.md5_string(data)

def hash_md5_file(file_path: str) -> str:
    """
    Compute the MD5 hash of a file.

    Reads the entire file and computes its MD5 digest. MD5 is provided for
    legacy compatibility but should not be used for security-critical applications.

    Args:
        file_path: Path to the file to hash (relative to workspace root)

    Returns:
        str: The MD5 hash as a hexadecimal-encoded string (32 characters)

    Raises:
        Error: If the file cannot be read or hashing fails

    Examples:
        # Compute MD5 of a file (for legacy systems)
        digest = hash_md5_file("legacy-data.bin")

        # Quick non-critical checksum
        quick_check = hash_md5_file("temp-file.txt")

    Note:
        MD5 is deprecated for cryptographic use. For security purposes,
        use hash_sha256_file() or hash_sha512_file() instead.
    """
    return hash.md5_file(file_path)

# ============================================================================
# CRC32 - Cyclic Redundancy Check 32-bit
# ============================================================================

def hash_crc32(data: str) -> str:
    """
    Compute the CRC32 checksum of a string.

    CRC32 produces a 32-bit (8 hexadecimal character) checksum. It is a
    simple error-detection mechanism, NOT cryptographically secure. Use it
    only for detecting accidental data corruption, not malicious tampering.

    Args:
        data: The string to checksum

    Returns:
        str: The CRC32 checksum as a zero-padded hexadecimal string (8 characters)

    Raises:
        Error: If checksumming fails

    Examples:
        # Quick data corruption check
        checksum = hash_crc32("important-data")

        # Create a simple integrity marker
        marker = hash_crc32("config-version-1")

    Note:
        CRC32 is NOT suitable for security purposes. Use SHA-based functions
        for integrity verification against tampering.
    """
    return hash.crc32_string(data)

def hash_crc32_file(file_path: str) -> str:
    """
    Compute the CRC32 checksum of a file.

    Reads the entire file and computes its CRC32 checksum. Useful for
    detecting accidental corruption but not for security-critical integrity checks.

    Args:
        file_path: Path to the file to checksum (relative to workspace root)

    Returns:
        str: The CRC32 checksum as a zero-padded hexadecimal string (8 characters)

    Raises:
        Error: If the file cannot be read or checksumming fails

    Examples:
        # Check for file corruption during transfer
        original_crc = hash_crc32_file("data-backup.bin")
        # After transfer...
        transferred_crc = hash_crc32_file("data-transferred.bin")
        if original_crc == transferred_crc:
            print("Transfer completed without corruption")

    Note:
        CRC32 is fast but NOT cryptographically secure. For integrity
        verification against tampering, use hash_sha256_file() instead.
    """
    return hash.crc32_file(file_path)

# ============================================================================
# Hexadecimal Encoding/Decoding
# ============================================================================

def hash_hex_encode(data: str) -> str:
    """
    Encode raw bytes as hexadecimal.

    Converts the byte representation of a string into a hexadecimal-encoded
    format. Useful for displaying binary data in human-readable form.

    Args:
        data: The string to hex-encode

    Returns:
        str: The hexadecimal-encoded representation

    Raises:
        Error: If encoding fails

    Examples:
        # Encode binary data
        encoded = hash_hex_encode("Hello")
        print(encoded)  # Output: 48656c6c6f

        # Display raw bytes
        raw = "\\x00\\x01\\x02\\x03"
        print(hash_hex_encode(raw))

        # Encode and decode roundtrip
        original = "data"
        encoded = hash_hex_encode(original)
        decoded = hash_hex_decode(encoded)
        assert original == decoded
    """
    return hash.hex_encode(data)

def hash_hex_decode(hex_string: str) -> str:
    """
    Decode hexadecimal-encoded bytes.

    Converts a hexadecimal string back into raw bytes (as a UTF-8 string).
    Inverse of hash_hex_encode().

    Args:
        hex_string: The hexadecimal string to decode

    Returns:
        str: The decoded bytes as a UTF-8 string

    Raises:
        Error: If the hex string is invalid or decoded bytes are not valid UTF-8

    Examples:
        # Decode hex string
        decoded = hash_hex_decode("48656c6c6f")
        print(decoded)  # Output: Hello

        # Roundtrip encode/decode
        original = "test-data"
        encoded = hash_hex_encode(original)
        recovered = hash_hex_decode(encoded)
        assert original == recovered

    Note:
        The decoded bytes must be valid UTF-8. If your data contains
        non-UTF-8 bytes, decoding will fail.
    """
    return hash.hex_decode(hex_string)

# ============================================================================
# Base64 Encoding/Decoding
# ============================================================================

def hash_base64_encode(data: str) -> str:
    """
    Encode raw bytes as base64.

    Converts the byte representation of a string into base64 encoding.
    Base64 is commonly used for transmitting binary data over text-based
    protocols like email and HTTP.

    Args:
        data: The string to base64-encode

    Returns:
        str: The base64-encoded representation

    Raises:
        Error: If encoding fails

    Examples:
        # Encode binary data
        encoded = hash_base64_encode("Hello, World!")
        print(encoded)  # Output: SGVsbG8sIFdvcmxkIQ==

        # Encode for transmission
        credentials = "username:password"
        encoded_creds = hash_base64_encode(credentials)

        # Roundtrip encode/decode
        original = "data"
        encoded = hash_base64_encode(original)
        decoded = hash_base64_decode(encoded)
        assert original == decoded
    """
    return hash.base64_encode(data)

def hash_base64_decode(base64_string: str) -> str:
    """
    Decode base64-encoded bytes.

    Converts a base64 string back into raw bytes (as a UTF-8 string).
    Inverse of hash_base64_encode().

    Args:
        base64_string: The base64 string to decode

    Returns:
        str: The decoded bytes as a UTF-8 string

    Raises:
        Error: If the base64 string is invalid or decoded bytes are not valid UTF-8

    Examples:
        # Decode base64 string
        decoded = hash_base64_decode("SGVsbG8sIFdvcmxkIQ==")
        print(decoded)  # Output: Hello, World!

        # Decode credentials
        encoded_creds = "dXNlcm5hbWU6cGFzc3dvcmQ="
        credentials = hash_base64_decode(encoded_creds)
        username, password = credentials.split(":")

        # Roundtrip encode/decode
        original = "test-data"
        encoded = hash_base64_encode(original)
        recovered = hash_base64_decode(encoded)
        assert original == recovered

    Note:
        The decoded bytes must be valid UTF-8. If your data contains
        non-UTF-8 bytes, decoding will fail.
    """
    return hash.base64_decode(base64_string)
