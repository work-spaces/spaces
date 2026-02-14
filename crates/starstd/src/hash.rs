use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Computes the SHA-256 checksum for the contents of a file.
    ///
    /// ```python
    /// checksum = hash.compute_sha256_from_file("data/model.bin")
    /// print(f"File SHA-256: {checksum}")
    /// ```
    ///
    /// # Arguments
    /// * `file_path`: The path to the file to be hashed.
    ///
    /// # Returns
    /// * `str`: The hex-encoded SHA-256 digest of the file.
    fn compute_sha256_from_file(file_path: &str) -> anyhow::Result<String> {
        let file_contents = std::fs::read(file_path).context(format_context!("{file_path}"))?;
        let digest = sha256::digest(file_contents);
        Ok(digest)
    }

    /// Computes the SHA-256 checksum for a given string.
    ///
    /// ```python
    /// text_hash = hash.compute_sha256_from_string("my-unique-identity")
    /// print(f"String Hash: {text_hash}")
    /// ```
    ///
    /// # Arguments
    /// * `input`: The raw string to be hashed.
    ///
    /// # Returns
    /// * `str`: The hex-encoded SHA-256 digest of the input string.
    fn compute_sha256_from_string(input: &str) -> anyhow::Result<String> {
        Ok(sha256::digest(input.as_bytes()))
    }
}
