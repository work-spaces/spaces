use anyhow::Context;
use anyhow::anyhow;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;

use crate::is_lsp_mode;

// ── shared streaming helper ───────────────────────────────────────────────────

/// Stream `file_path` through a hasher in 64 KiB chunks.
///
/// - `state`    – the hasher (or any accumulator), owned by this function.
/// - `update`   – called for each chunk; receives `&mut state` and `&[u8]`.
/// - `finalize` – called once after EOF; receives the owned `state`.
///
/// Memory usage is O(chunk-size) regardless of file size.
fn stream_file<S, U, Fi>(
    file_path: &str,
    mut state: S,
    mut update: U,
    finalize: Fi,
) -> anyhow::Result<String>
where
    U: FnMut(&mut S, &[u8]),
    Fi: FnOnce(S) -> String,
{
    use std::io::Read;
    let file = std::fs::File::open(file_path).context(format_context!("{file_path}"))?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = [0u8; 65536]; // 64 KiB read buffer
    loop {
        let n = reader
            .read(&mut buf)
            .context(format_context!("{file_path}"))?;
        if n == 0 {
            break;
        }
        update(&mut state, &buf[..n]);
    }
    Ok(finalize(state))
}

// ── sha256 helpers (use sha2::Sha256 directly – no heavyweight wrapper crate) ─

fn sha256_string_impl(input: &str) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(input.as_bytes());
    Ok(format!("{:x}", h.finalize()))
}

fn sha256_file_impl(file_path: &str) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    stream_file(
        file_path,
        Sha256::new(),
        |h, chunk| h.update(chunk),
        |h| format!("{:x}", h.finalize()),
    )
}

// ── starlark module ───────────────────────────────────────────────────────────

// This defines the functions that are visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    // ── SHA-256 ───────────────────────────────────────────────────────────────

    /// Computes the SHA-256 checksum for the contents of a file.
    ///
    /// The file is read in 64 KiB streaming chunks so large files do not cause
    /// excessive memory consumption.
    ///
    /// ```python
    /// checksum = hash.compute_sha256_from_file("data/model.bin")
    /// print(f"File SHA-256: {checksum}")
    /// ```
    fn compute_sha256_from_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        sha256_file_impl(file_path)
    }

    /// Computes the SHA-256 checksum for a given string.
    ///
    /// ```python
    /// text_hash = hash.compute_sha256_from_string("my-unique-identity")
    /// print(f"String Hash: {text_hash}")
    /// ```
    fn compute_sha256_from_string(input: &str) -> anyhow::Result<String> {
        sha256_string_impl(input)
    }

    /// Alias of `compute_sha256_from_string`.
    fn sha256_string(input: &str) -> anyhow::Result<String> {
        sha256_string_impl(input)
    }

    /// Alias of `compute_sha256_from_file`.
    ///
    /// The file is read in 64 KiB streaming chunks.
    fn sha256_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        sha256_file_impl(file_path)
    }

    // ── SHA-1 ─────────────────────────────────────────────────────────────────

    /// Computes SHA-1 for a string, returning a 40-character hex digest.
    ///
    /// SHA-1 is cryptographically weak; prefer SHA-256 for new applications.
    fn sha1_string(input: &str) -> anyhow::Result<String> {
        use sha1::{Digest, Sha1};
        let mut h = Sha1::new();
        h.update(input.as_bytes());
        Ok(format!("{:x}", h.finalize()))
    }

    /// Computes SHA-1 for a file, streaming in 64 KiB chunks.
    ///
    /// SHA-1 is cryptographically weak; prefer SHA-256 for new applications.
    fn sha1_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        use sha1::{Digest, Sha1};
        stream_file(
            file_path,
            Sha1::new(),
            |h, chunk| h.update(chunk),
            |h| format!("{:x}", h.finalize()),
        )
    }

    // ── SHA-512 ───────────────────────────────────────────────────────────────

    /// Computes SHA-512 for a string, returning a 128-character hex digest.
    fn sha512_string(input: &str) -> anyhow::Result<String> {
        use sha2::{Digest, Sha512};
        let mut h = Sha512::new();
        h.update(input.as_bytes());
        Ok(format!("{:x}", h.finalize()))
    }

    /// Computes SHA-512 for a file, streaming in 64 KiB chunks.
    fn sha512_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        use sha2::{Digest, Sha512};
        stream_file(
            file_path,
            Sha512::new(),
            |h, chunk| h.update(chunk),
            |h| format!("{:x}", h.finalize()),
        )
    }

    // ── BLAKE3 ────────────────────────────────────────────────────────────────

    /// Computes BLAKE3 for a string, returning a 64-character hex digest.
    ///
    /// BLAKE3 is a modern cryptographic hash function that is significantly
    /// faster than SHA-256/SHA-512 while offering equivalent security.  It
    /// supports arbitrary-length output; this function returns the default
    /// 256-bit (64 hex character) digest.
    ///
    /// ```python
    /// digest = hash.blake3_string("my-data")
    /// print(f"BLAKE3: {digest}")
    /// ```
    fn blake3_string(input: &str) -> anyhow::Result<String> {
        Ok(blake3::hash(input.as_bytes()).to_hex().to_string())
    }

    /// Computes BLAKE3 for a file, streaming in 64 KiB chunks.
    ///
    /// Returns a 64-character lowercase hex digest.  The file is never fully
    /// loaded into memory, making this safe for large files.
    ///
    /// ```python
    /// digest = hash.blake3_file("data/model.bin")
    /// print(f"BLAKE3: {digest}")
    /// ```
    fn blake3_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        stream_file(
            file_path,
            blake3::Hasher::new(),
            |h, chunk| {
                h.update(chunk);
            },
            |h| h.finalize().to_hex().to_string(),
        )
    }

    // ── MD5 ───────────────────────────────────────────────────────────────────

    /// Computes MD5 for a string, returning a 32-character hex digest.
    ///
    /// MD5 is cryptographically broken; do **not** use for security purposes.
    /// Provided for legacy compatibility and non-critical checksums only.
    fn md5_string(input: &str) -> anyhow::Result<String> {
        Ok(format!("{:x}", md5::compute(input.as_bytes())))
    }

    /// Computes MD5 for a file, streaming in 64 KiB chunks.
    ///
    /// MD5 is cryptographically broken; do **not** use for security purposes.
    fn md5_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        stream_file(
            file_path,
            md5::Context::new(),
            |ctx, chunk| ctx.consume(chunk),
            |ctx| format!("{:x}", ctx.compute()),
        )
    }

    // ── CRC32 ─────────────────────────────────────────────────────────────────

    /// Computes CRC32 for a string, returning a zero-padded 8-character hex string.
    ///
    /// CRC32 is **not** a cryptographic hash; use only for accidental-corruption
    /// detection, never for security.
    fn crc32_string(input: &str) -> anyhow::Result<String> {
        let checksum = crc32fast::hash(input.as_bytes());
        Ok(format!("{checksum:08x}"))
    }

    /// Computes CRC32 for a file, streaming in 64 KiB chunks.
    ///
    /// Returns a zero-padded 8-character hex string.  CRC32 is **not** a
    /// cryptographic hash; use only for accidental-corruption detection.
    fn crc32_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        stream_file(
            file_path,
            crc32fast::Hasher::new(),
            |h, chunk| h.update(chunk),
            |h| format!("{:08x}", h.finalize()),
        )
    }

    // ── Hex encoding / decoding ───────────────────────────────────────────────

    /// Hex-encodes the raw bytes of a string.
    ///
    /// ```python
    /// encoded = hash.hex_encode("Hello")  # "48656c6c6f"
    /// ```
    fn hex_encode(bytes: &str) -> anyhow::Result<String> {
        Ok(hex::encode(bytes.as_bytes()))
    }

    /// Decodes a hex string into raw bytes and returns them as a UTF-8 string.
    ///
    /// Returns an error if the hex is malformed or the decoded bytes are not
    /// valid UTF-8.
    ///
    /// ```python
    /// decoded = hash.hex_decode("48656c6c6f")  # "Hello"
    /// ```
    fn hex_decode(input: &str) -> anyhow::Result<String> {
        let bytes = hex::decode(input).context(format_context!("{input}"))?;
        String::from_utf8(bytes).map_err(|e| anyhow!(format_context!("{e}")))
    }

    // ── Base64 encoding / decoding ────────────────────────────────────────────

    /// Base64-encodes the raw bytes of a string (standard alphabet, with padding).
    ///
    /// ```python
    /// encoded = hash.base64_encode("Hello, World!")  # "SGVsbG8sIFdvcmxkIQ=="
    /// ```
    fn base64_encode(bytes: &str) -> anyhow::Result<String> {
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes.as_bytes()))
    }

    /// Decodes a standard-alphabet base64 string into raw bytes and returns
    /// them as a UTF-8 string.
    ///
    /// Returns an error if the base64 is malformed or the decoded bytes are not
    /// valid UTF-8.
    ///
    /// ```python
    /// decoded = hash.base64_decode("SGVsbG8sIFdvcmxkIQ==")  # "Hello, World!"
    /// ```
    fn base64_decode(input: &str) -> anyhow::Result<String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(input)
            .context(format_context!("{input}"))?;
        String::from_utf8(bytes).map_err(|e| anyhow!(format_context!("{e}")))
    }
}
