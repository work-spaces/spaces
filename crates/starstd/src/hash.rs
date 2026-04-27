use anyhow::Context;
use anyhow::anyhow;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;

use crate::is_lsp_mode;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Computes the SHA-256 checksum for the contents of a file.
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
    fn sha256_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        sha256_file_impl(file_path)
    }

    /// Computes SHA-1 for a string.
    fn sha1_string(input: &str) -> anyhow::Result<String> {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(input.as_bytes());
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Computes SHA-1 for a file.
    fn sha1_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let file_contents = std::fs::read(file_path).context(format_context!("{file_path}"))?;
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(&file_contents);
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Computes SHA-512 for a string.
    fn sha512_string(input: &str) -> anyhow::Result<String> {
        use sha2::{Digest, Sha512};
        let mut hasher = Sha512::new();
        hasher.update(input.as_bytes());
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Computes SHA-512 for a file.
    fn sha512_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let file_contents = std::fs::read(file_path).context(format_context!("{file_path}"))?;
        use sha2::{Digest, Sha512};
        let mut hasher = Sha512::new();
        hasher.update(&file_contents);
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Computes MD5 for a string.
    fn md5_string(input: &str) -> anyhow::Result<String> {
        Ok(format!("{:x}", md5::compute(input.as_bytes())))
    }

    /// Computes MD5 for a file.
    fn md5_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let file_contents = std::fs::read(file_path).context(format_context!("{file_path}"))?;
        Ok(format!("{:x}", md5::compute(file_contents)))
    }

    /// Computes CRC32 for a string.
    fn crc32_string(input: &str) -> anyhow::Result<String> {
        let checksum = crc32fast::hash(input.as_bytes());
        Ok(format!("{checksum:08x}"))
    }

    /// Computes CRC32 for a file.
    fn crc32_file(file_path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let file_contents = std::fs::read(file_path).context(format_context!("{file_path}"))?;
        let checksum = crc32fast::hash(&file_contents);
        Ok(format!("{checksum:08x}"))
    }

    /// Hex-encodes raw bytes represented as a string.
    ///
    /// Note: Starlark passes strings, so this treats input as raw byte content of the string.
    fn hex_encode(bytes: &str) -> anyhow::Result<String> {
        Ok(hex::encode(bytes.as_bytes()))
    }

    /// Decodes a hex string into raw bytes and returns them as a UTF-8 string.
    ///
    /// Returns an error if the decoded bytes are not valid UTF-8.
    fn hex_decode(input: &str) -> anyhow::Result<String> {
        let bytes = hex::decode(input).context(format_context!("{input}"))?;
        String::from_utf8(bytes).map_err(|e| anyhow!(format_context!("{e}")))
    }

    /// Base64-encodes raw bytes represented as a string.
    ///
    /// Note: Starlark passes strings, so this treats input as raw byte content of the string.
    fn base64_encode(bytes: &str) -> anyhow::Result<String> {
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes.as_bytes()))
    }

    /// Decodes a base64 string into raw bytes and returns them as a UTF-8 string.
    ///
    /// Returns an error if the decoded bytes are not valid UTF-8.
    fn base64_decode(input: &str) -> anyhow::Result<String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(input)
            .context(format_context!("{input}"))?;
        String::from_utf8(bytes).map_err(|e| anyhow!(format_context!("{e}")))
    }
}

fn sha256_string_impl(input: &str) -> anyhow::Result<String> {
    Ok(sha256::digest(input.as_bytes()))
}

fn sha256_file_impl(file_path: &str) -> anyhow::Result<String> {
    let file_contents = std::fs::read(file_path).context(format_context!("{file_path}"))?;
    Ok(sha256::digest(file_contents))
}
