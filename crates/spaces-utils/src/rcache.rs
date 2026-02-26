/// Rule cache
/// Cache the outputs of the rule based on the input digest
use crate::age;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const ARTIFACT_CACHE_DIR: &str = "artifacts";
const RULE_DIGEST_CACHE_DIR: &str = "rule_digests";

fn save_artifact_to_cache(
    cache_path: &std::path::Path,
    artifact_path: &std::path::Path,
) -> anyhow::Result<Arc<str>> {
    // calculate the hash of the artifact
    let contents = std::fs::read(artifact_path).with_context(|| {
        format_context!(
            "Failed to read workspace artifact {}",
            artifact_path.display()
        )
    })?;

    let artifact_hash = blake3::hash(&contents).to_string();
    //path in cache is the hash of the artifact contents
    let path_in_cache = cache_path.join(ARTIFACT_CACHE_DIR).join(&artifact_hash);

    // skip caching if the artifact is already in the cache
    if !path_in_cache.exists() {
        if let Some(parent) = path_in_cache.parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create parent directory for cache entry"
            ))?;
        }

        // save the artifact to the cache
        reflink_copy::reflink_or_copy(artifact_path, path_in_cache)?;

        let artifact_metadata = std::fs::metadata(artifact_path)
            .with_context(|| format_context!("Failed to get metadata for {artifact_path:?}"))?;

        // Update the metadata to be read-only
        let mut read_write_permissions = artifact_metadata.permissions();

        read_write_permissions.set_readonly(true);

        // Set the permissions to read-write
        std::fs::set_permissions(artifact_path, read_write_permissions).context(
            format_context!("Failed to set permissions for {}", artifact_path.display()),
        )?;
    }

    Ok(artifact_hash.into())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CachedOutput {
    // where does the artifact exist in the cache
    path_in_cache: Arc<str>,
    // where should the artifact be restored in the workspace
    path_in_workspace: Arc<str>,
}

impl CachedOutput {
    fn new_from_workspace_path(
        cache_path: &std::path::Path,
        path_in_workspace: &std::path::Path,
    ) -> anyhow::Result<Self> {
        let path_in_cache = save_artifact_to_cache(cache_path, path_in_workspace)
            .with_context(|| format_context!("Failed to save artifact to cache"))?;
        let path_in_workspace = path_in_workspace.to_string_lossy().into();
        Ok(CachedOutput {
            path_in_cache,
            path_in_workspace,
        })
    }

    fn restore_to_workspace(&self) -> anyhow::Result<()> {
        // hard link to the workspace
        let path_in_cache = std::path::Path::new(self.path_in_cache.as_ref());
        let path_in_workspace = std::path::Path::new(self.path_in_workspace.as_ref());
        if let Some(parent) = path_in_workspace.parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create parent directory for workspace entry {}",
                path_in_workspace.display()
            ))?;
        }

        if path_in_workspace.exists() {
            std::fs::remove_file(path_in_workspace).context(format_context!(
                "Failed to remove existing workspace entry {}",
                path_in_workspace.display()
            ))?;
        }

        std::fs::hard_link(path_in_cache, path_in_workspace).with_context(|| {
            format_context!(
                "Failed to restore artifact to workspace at {}",
                path_in_workspace.display()
            )
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuleDigestCacheEntry {
    last_used: age::LastUsed,
    outputs: Vec<CachedOutput>,
}

impl RuleDigestCacheEntry {
    fn get_path_in_cache(cache_path: &std::path::Path, rule_digest: &str) -> std::path::PathBuf {
        cache_path.join(RULE_DIGEST_CACHE_DIR).join(rule_digest)
    }

    /// loads an entry from cache if it exists
    fn new_from_cache(
        cache_path: &std::path::Path,
        rule_digest: &str,
    ) -> anyhow::Result<Option<Self>> {
        let path_in_cache = Self::get_path_in_cache(cache_path, rule_digest);
        if path_in_cache.exists() {
            let contents = std::fs::read(&path_in_cache).with_context(|| {
                format_context!(
                    "While trying to open cache file for rule digest {}",
                    rule_digest
                )
            })?;

            let mut entry: Self = postcard::from_bytes(&contents).context(format_context!(
                "Failed to decode cache entry for rule digest {}",
                rule_digest
            ))?;

            // update last used and save the entry
            entry.last_used.update();
            let encoded = postcard::to_stdvec(&entry)
                .context(format_context!("Failed to encode rcache entry"))?;
            std::fs::write(path_in_cache, encoded).context(format_context!(
                "Failed to write cache entry for rule digest {}",
                rule_digest
            ))?;

            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    fn restore_to_workspace(&self) -> anyhow::Result<()> {
        for output in &self.outputs {
            output
                .restore_to_workspace()
                .with_context(|| format_context!("Failed to restore cached output"))?;
        }
        Ok(())
    }

    fn create_cache_entry(
        cache_path: &std::path::Path,
        rule_digest: &str,
        workspace_outputs: &[Arc<std::path::Path>],
    ) -> anyhow::Result<()> {
        let mut outputs = Vec::new();
        for path_in_workspace in workspace_outputs {
            outputs.push(
                CachedOutput::new_from_workspace_path(cache_path, path_in_workspace)
                    .with_context(|| format_context!("Failed to create cached output"))?,
            );
        }

        let entry = Self {
            last_used: age::LastUsed::default(),
            outputs,
        };

        let encoded = postcard::to_stdvec(&entry)
            .context(format_context!("Failed to encode rcache entry"))?;

        let path_in_cache = Self::get_path_in_cache(cache_path, rule_digest);
        if let Some(parent) = path_in_cache.parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create parent for rule digest cache {}",
                path_in_cache.display()
            ))?;
        }
        std::fs::write(path_in_cache, encoded).context(format_context!(
            "Failed to write cache entry for rule digest {}",
            rule_digest
        ))?;

        Ok(())
    }
}

/// Checks to see if the input digest exists in the cache.
///
/// if the input digest exists in the cache, populate the workspace outputs with
/// cached values.
///
/// if the input digest does not exist, the task is executed and the outputs are cached
/// if the task runs successfully.
pub fn execute<Exec>(
    cache_path: &std::path::Path,
    rule_digest: Arc<str>,
    outputs: Vec<Arc<std::path::Path>>,
    exec: Exec,
) -> anyhow::Result<()>
where
    Exec: FnOnce() -> anyhow::Result<()>,
{
    if let Some(entry) = RuleDigestCacheEntry::new_from_cache(cache_path, &rule_digest)
        .context(format_context!("Failed to check for cache entry"))?
    {
        // cache entry exists
        entry
            .restore_to_workspace()
            .with_context(|| format_context!("Failed to restore cached output to workspace"))?;
    } else {
        exec().with_context(|| format_context!("Task failed to execute in rule cacher"))?;

        RuleDigestCacheEntry::create_cache_entry(cache_path, &rule_digest, outputs.as_slice())
            .context(format_context!("Failed to create cache entry"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a unique temporary directory for test isolation.
    /// Returns the path - cleanup left to caller or OS
    fn make_test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("rcache_tests")
            .join(name)
            .join(format!("{}", std::process::id()));
        // ensure clean state
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("failed to create test dir");
        dir
    }

    fn write_test_file(path: &std::path::Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn test_save_artifact_to_cache() {
        let root = make_test_dir("save_artifact");
        let cache_path = root.join("cache");
        let workspace = root.join("workspace");

        let artifact = workspace.join("output.bin");
        let content = b"hello world artifact";
        write_test_file(&artifact, content);

        let expected_hash = blake3::hash(content).to_string();

        // First save
        assert!(artifact.exists());
        let hash1 = save_artifact_to_cache(&cache_path, &artifact).unwrap();
        assert_eq!(hash1.as_ref(), expected_hash.as_str());

        // The artifacts directory should have been created
        assert!(cache_path.join(ARTIFACT_CACHE_DIR).exists());

        // Second save of the same content is idempotent (no error)
        let hash2 = save_artifact_to_cache(&cache_path, &artifact).unwrap();
        assert_eq!(hash1, hash2);

        // Different content produces a different hash
        let artifact2 = workspace.join("output2.bin");
        write_test_file(&artifact2, b"different content");
        let hash3 = save_artifact_to_cache(&cache_path, &artifact2).unwrap();
        assert_ne!(hash1, hash3);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_create_then_load_cache_entry_roundtrip() {
        let root = make_test_dir("roundtrip");
        let cache_path = root.join("cache");
        let workspace = root.join("workspace");

        // Create two workspace artifacts
        let file_a = workspace.join("a.txt");
        let file_b = workspace.join("sub").join("b.txt");
        write_test_file(&file_a, b"content of a");
        write_test_file(&file_b, b"content of b");

        let outputs: Vec<Arc<std::path::Path>> = vec![file_a.clone().into(), file_b.clone().into()];

        let digest = "test_digest_001";

        // Before creation, loading returns None
        let before = RuleDigestCacheEntry::new_from_cache(&cache_path, digest).unwrap();
        assert!(before.is_none());

        // Create the cache entry
        RuleDigestCacheEntry::create_cache_entry(&cache_path, digest, &outputs).unwrap();

        // The rule digest file should now exist on disk
        let digest_path = RuleDigestCacheEntry::get_path_in_cache(&cache_path, digest);
        assert!(digest_path.exists());

        // Load the entry back
        let loaded = RuleDigestCacheEntry::new_from_cache(&cache_path, digest).unwrap();
        assert!(loaded.is_some());

        let entry = loaded.unwrap();
        assert_eq!(entry.outputs.len(), 2);

        // Verify workspace paths are preserved
        assert_eq!(
            entry.outputs[0].path_in_workspace.as_ref(),
            file_a.to_string_lossy().as_ref()
        );
        assert_eq!(
            entry.outputs[1].path_in_workspace.as_ref(),
            file_b.to_string_lossy().as_ref()
        );

        // Verify that path_in_cache holds the blake3 hash of the content
        let expected_hash_a = blake3::hash(b"content of a").to_string();
        let expected_hash_b = blake3::hash(b"content of b").to_string();
        assert_eq!(
            entry.outputs[0].path_in_cache.as_ref(),
            expected_hash_a.as_str()
        );
        assert_eq!(
            entry.outputs[1].path_in_cache.as_ref(),
            expected_hash_b.as_str()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_create_cache_entry_with_multiple_outputs_same_content() {
        let root = make_test_dir("same_content");
        let cache_path = root.join("cache");
        let workspace = root.join("workspace");

        // Two different files with identical content
        let file_x = workspace.join("x.txt");
        let file_y = workspace.join("y.txt");
        let same_content = b"identical bytes";
        write_test_file(&file_x, same_content);
        write_test_file(&file_y, same_content);

        let outputs: Vec<Arc<std::path::Path>> = vec![file_x.clone().into(), file_y.clone().into()];

        RuleDigestCacheEntry::create_cache_entry(&cache_path, "dup_digest", &outputs).unwrap();

        let entry = RuleDigestCacheEntry::new_from_cache(&cache_path, "dup_digest")
            .unwrap()
            .expect("entry should exist");

        // Both outputs should share the same cache hash since content is identical
        assert_eq!(entry.outputs.len(), 2);
        assert_eq!(
            entry.outputs[0].path_in_cache,
            entry.outputs[1].path_in_cache
        );

        // But workspace paths should differ
        assert_ne!(
            entry.outputs[0].path_in_workspace,
            entry.outputs[1].path_in_workspace
        );
        assert_eq!(
            entry.outputs[0].path_in_workspace.as_ref(),
            file_x.to_string_lossy().as_ref()
        );
        assert_eq!(
            entry.outputs[1].path_in_workspace.as_ref(),
            file_y.to_string_lossy().as_ref()
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
