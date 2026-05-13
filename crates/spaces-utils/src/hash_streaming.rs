use anyhow::Context;
use anyhow_source_location::format_context;
use std::path::Path;

/// Stream a file through a hasher in 64 KiB chunks.
///
/// - `path`     – the file path to hash
/// - `state`    – the hasher (or any accumulator), owned by this function
/// - `update`   – called for each chunk; receives `&mut state` and `&[u8]`
/// - `finalize` – called once after EOF; receives the owned `state`
///
/// Memory usage is O(chunk-size) regardless of file size.
fn stream_file<S, U, Fi>(
    path: &Path,
    mut state: S,
    mut update: U,
    finalize: Fi,
) -> anyhow::Result<String>
where
    U: FnMut(&mut S, &[u8]),
    Fi: FnOnce(S) -> String,
{
    use std::io::Read;
    let file = std::fs::File::open(path).context(format_context!("{}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = [0u8; 65536]; // 64 KiB read buffer
    loop {
        let n = reader
            .read(&mut buf)
            .context(format_context!("{}", path.display()))?;
        if n == 0 {
            break;
        }
        update(&mut state, &buf[..n]);
    }
    Ok(finalize(state))
}

/// Computes BLAKE3 hash for a file, streaming in chunks.
///
/// Returns a lowercase hex digest string. The file is never fully loaded into
/// memory, making this safe for large files. Uses blake3's optimized update_reader
/// which automatically sizes buffers for maximum SIMD performance.
pub fn stream_blake3_hash(path: &Path) -> anyhow::Result<String> {
    let file = std::fs::File::open(path).context(format_context!("{}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let mut hasher = blake3::Hasher::new();
    hasher
        .update_reader(reader)
        .context(format_context!("{}", path.display()))?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Computes SHA-256 hash for a file, streaming in 64 KiB chunks.
///
/// Returns a lowercase hex digest string. The file is never fully loaded into
/// memory, making this safe for large files.
pub fn stream_sha256_hash(path: &Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    stream_file(
        path,
        Sha256::new(),
        |h, chunk| h.update(chunk),
        |h| format!("{:x}", h.finalize()),
    )
}
