/// Rule cache
/// Cache the outputs of the rule based on the input digest
use crate::{age, ci, logger, targets};
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum CacheStatus {
    /// No cache status available (legacy metrics entries or cache not used for this rule)
    #[default]
    None,
    /// The rule was skipped (platform, cancelled, optional, unchanged deps); payload is the
    /// rule digest used for caching this rule.
    Skipped(Arc<str>),
    /// The rule was executed (cache miss or no caching); payload is the rule digest used for
    /// caching this rule.
    Executed(Arc<str>),
    /// The rule outputs were restored from the rule cache; payload is the rule digest used for
    /// caching this rule.
    Restored(Arc<str>),
}

impl std::fmt::Display for CacheStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheStatus::None => write!(f, "None"),
            CacheStatus::Skipped(digest) => write!(f, "Skipped({digest})"),
            CacheStatus::Executed(digest) => write!(f, "Executed({digest})"),
            CacheStatus::Restored(digest) => write!(f, "Restored({digest})"),
        }
    }
}

const ARTIFACT_CACHE_DIR: &str = "artifacts";
const RULE_DIGEST_CACHE_DIR: &str = "rule_digests";
const STAGE_CACHE_DIR: &str = "stage";

fn get_artifact_cache_path(cache_path: &std::path::Path, artifact: &str) -> std::path::PathBuf {
    cache_path.join(ARTIFACT_CACHE_DIR).join(artifact)
}

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
    let path_in_cache = get_artifact_cache_path(cache_path, &artifact_hash);

    // skip caching if the artifact is already in the cache
    if !path_in_cache.exists() {
        if let Some(parent) = path_in_cache.parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create parent directory for cache entry"
            ))?;
        }

        // save the artifact to a staged path first
        let stage_dir = cache_path.join(STAGE_CACHE_DIR);
        std::fs::create_dir_all(&stage_dir).context(format_context!(
            "Failed to create stage directory for cache entry"
        ))?;
        let staged_path = stage_dir.join(&artifact_hash);
        reflink_copy::reflink_or_copy(artifact_path, &staged_path).with_context(|| {
            format_context!(
                "Failed to copy artifact to staged cache path {}",
                staged_path.display()
            )
        })?;

        // verify the staged file hash matches the expected hash
        let staged_contents = std::fs::read(&staged_path).with_context(|| {
            format_context!(
                "Failed to read staged cache artifact {}",
                staged_path.display()
            )
        })?;
        let staged_hash = blake3::hash(&staged_contents).to_string();
        if staged_hash != artifact_hash {
            let _ = std::fs::remove_file(&staged_path);
            return Err(anyhow::anyhow!(format_context!(
                "Hash mismatch for staged artifact {}: expected {} but got {}",
                staged_path.display(),
                artifact_hash,
                staged_hash
            )));
        }

        // hash verified - rename staged file to final cache path
        std::fs::rename(&staged_path, &path_in_cache).with_context(|| {
            format_context!(
                "Failed to rename staged cache file {} to {}",
                staged_path.display(),
                path_in_cache.display()
            )
        })?;

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
pub struct CachedTarget {
    // where does the artifact exist in the cache
    path_in_cache: Arc<str>,
    // where should the artifact be restored in the workspace
    path_in_workspace: Arc<str>,
}

impl CachedTarget {
    fn new_from_workspace_path(
        cache_path: &std::path::Path,
        path_in_workspace: &std::path::Path,
    ) -> anyhow::Result<Self> {
        let path_in_cache = save_artifact_to_cache(cache_path, path_in_workspace)
            .with_context(|| format_context!("Failed to save artifact to cache"))?;
        let path_in_workspace = path_in_workspace.to_string_lossy().into();
        Ok(CachedTarget {
            path_in_cache,
            path_in_workspace,
        })
    }

    fn restore_to_workspace(&self, path_to_cache: &std::path::Path) -> anyhow::Result<()> {
        // hard link to the workspace
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

        let path_in_cache = get_artifact_cache_path(path_to_cache, &self.path_in_cache);
        std::fs::hard_link(&path_in_cache, path_in_workspace).with_context(|| {
            format_context!(
                "Failed to restore artifact to workspace at {} from {}",
                path_in_workspace.display(),
                path_in_cache.display()
            )
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuleDigestCacheEntry {
    last_used: age::LastUsed,
    outputs: Vec<CachedTarget>,
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

    fn restore_to_workspace(&self, path_to_cache: &std::path::Path) -> anyhow::Result<()> {
        for output in &self.outputs {
            output
                .restore_to_workspace(path_to_cache)
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
                CachedTarget::new_from_workspace_path(cache_path, path_in_workspace)
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

fn logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "rcache".into())
}

pub fn prune(
    cache_path: &std::path::Path,
    age: u16,
    is_dry_run: bool,
    console: console::Console,
    is_ci: ci::IsCi,
) -> anyhow::Result<()> {
    let group = ci::GithubLogGroup::new_group(console.clone(), is_ci, "Spaces RCache Prune")?;

    // Phase 1: find and remove stale rule_digest entries
    let rule_digests_path = cache_path.join(RULE_DIGEST_CACHE_DIR);
    let mut stale_digests: Vec<(String, u128, std::path::PathBuf)> = Vec::new();

    if rule_digests_path.exists() {
        let entries = std::fs::read_dir(&rule_digests_path)
            .context(format_context!("Failed to read rule digests directory"))?;

        for dir_entry in entries.filter_map(|e| e.ok()) {
            let path = dir_entry.path();
            if !path.is_file() {
                continue;
            }
            let digest = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let entry = std::fs::read(&path)
                .ok()
                .and_then(|contents| postcard::from_bytes::<RuleDigestCacheEntry>(&contents).ok());

            match entry {
                Some(entry) => {
                    let entry_age = entry.last_used.get_current_age();
                    if entry_age > age as u128 {
                        stale_digests.push((digest, entry_age, path));
                    }
                }
                None => {
                    // unreadable or corrupted — prune it
                    stale_digests.push((digest, u128::MAX, path));
                }
            }
        }
    }

    let mut total_size_removed = bytesize::ByteSize(0);
    let mut progress = console::Progress::new(
        console.clone(),
        "rcache-prune",
        Some(stale_digests.len() as u64),
        None,
    );

    let mut successfully_removed_paths: std::collections::HashSet<std::path::PathBuf> =
        std::collections::HashSet::new();

    for (digest, entry_age, path) in &stale_digests {
        let short_digest = &digest[..digest.len().min(8)];
        let age_display = if *entry_age == u128::MAX {
            "corrupted".to_string()
        } else {
            format!("{entry_age} days")
        };
        let digest_size = bytesize::ByteSize(std::fs::metadata(path).map(|m| m.len()).unwrap_or(0));
        total_size_removed += digest_size.0;
        logger(console.clone()).info(
            format!("Pruning rule digest {short_digest}: age {age_display} ({digest_size})")
                .as_str(),
        );
        progress.set_message(&format!("pruning digest {short_digest}"));
        if !is_dry_run {
            if let Err(e) = std::fs::remove_file(path) {
                logger(console.clone())
                    .error(format!("Failed to remove rule digest {short_digest}: {e}").as_str());
            } else {
                logger(console.clone()).info("- Removed.");
                successfully_removed_paths.insert(path.clone());
            }
        } else {
            logger(console.clone()).info("- Dry run. Not removed.");
        }
        progress.increment(1);
    }

    // Phase 2: GC unreferenced artifacts
    let artifacts_path = cache_path.join(ARTIFACT_CACHE_DIR);
    if artifacts_path.exists() {
        // Build the set of digest paths to skip when computing live artifact references.
        // In dry-run mode the stale files still exist on disk but should be treated as
        // already removed, so we skip all of them.  In non-dry-run mode we only skip
        // paths that were actually deleted in Phase 1; a digest whose removal failed
        // is still present on disk and may be the sole reference to some artifacts, so
        // we must not treat those artifacts as unreferenced.
        let paths_to_skip: std::collections::HashSet<std::path::PathBuf> = if is_dry_run {
            stale_digests.iter().map(|(_, _, p)| p.clone()).collect()
        } else {
            successfully_removed_paths
        };

        // Collect artifact hashes still referenced by live rule_digest entries
        let mut live_hashes: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Ok(entries) = std::fs::read_dir(&rule_digests_path) {
            for dir_entry in entries.filter_map(|e| e.ok()) {
                // Skip entries that were already pruned (or marked for pruning in dry-run)
                if paths_to_skip.contains(&dir_entry.path()) {
                    continue;
                }
                if let Ok(contents) = std::fs::read(dir_entry.path())
                    && let Ok(entry) = postcard::from_bytes::<RuleDigestCacheEntry>(&contents)
                {
                    for output in &entry.outputs {
                        live_hashes.insert(output.path_in_cache.to_string());
                    }
                }
            }
        }

        if let Ok(artifact_entries) = std::fs::read_dir(&artifacts_path) {
            for dir_entry in artifact_entries.filter_map(|e| e.ok()) {
                let path = dir_entry.path();
                if !path.is_file() {
                    continue;
                }
                let hash = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !live_hashes.contains(&hash) {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    total_size_removed += size;
                    let short_hash = &hash[..hash.len().min(8)];
                    logger(console.clone()).info(
                        format!(
                            "Removing unreferenced artifact {short_hash} ({})",
                            bytesize::ByteSize(size)
                        )
                        .as_str(),
                    );
                    if !is_dry_run && let Err(e) = std::fs::remove_file(&path) {
                        logger(console.clone())
                            .error(format!("Failed to remove artifact {short_hash}: {e}").as_str());
                    }
                }
            }
        }
    }

    // Sweep leftover staged files
    if !is_dry_run {
        let stage_path = cache_path.join(STAGE_CACHE_DIR);
        if stage_path.exists()
            && let Ok(stage_entries) = std::fs::read_dir(&stage_path)
        {
            for dir_entry in stage_entries.filter_map(|e| e.ok()) {
                let path = dir_entry.path();
                if path.is_file() {
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    total_size_removed += size;
                    logger(console.clone()).info(
                        format!(
                            "Removing leftover staged file {}",
                            path.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        )
                        .as_str(),
                    );
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    let total_removed_message = if is_dry_run {
        format!("Total size to prune in dry run: {total_size_removed}")
    } else {
        format!("Total removed: {total_size_removed}")
    };
    logger(console.clone()).info(total_removed_message.as_str());
    let finalize_message = if is_dry_run {
        format!("dry run: would prune {total_size_removed}")
    } else {
        format!("pruned {total_size_removed}")
    };
    progress.set_finalize_lines(logger::make_finalize_line(
        logger::FinalType::Finished,
        progress.elapsed(),
        finalize_message.as_str(),
    ));

    group.end_group(console.clone(), is_ci)?;
    Ok(())
}

fn get_size_of_path(path: &std::path::Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

pub fn show_info(
    cache_path: &std::path::Path,
    console: console::Console,
    is_ci: ci::IsCi,
) -> anyhow::Result<()> {
    if !cache_path.exists() {
        return Ok(());
    }

    let group = ci::GithubLogGroup::new_group(console.clone(), is_ci, "Spaces RCache Info")?;

    let artifacts_path = cache_path.join(ARTIFACT_CACHE_DIR);
    let rule_digests_path = cache_path.join(RULE_DIGEST_CACHE_DIR);

    let artifacts_size = if artifacts_path.exists() {
        get_size_of_path(&artifacts_path)
    } else {
        0
    };
    let rule_digests_size = if rule_digests_path.exists() {
        get_size_of_path(&rule_digests_path)
    } else {
        0
    };
    let total_size = artifacts_size + rule_digests_size;

    logger(console.clone()).info("Path: rcache");
    logger(console.clone()).info(
        format!(
            "  {}: {}",
            ARTIFACT_CACHE_DIR,
            bytesize::ByteSize(artifacts_size).display()
        )
        .as_str(),
    );
    logger(console.clone()).info(
        format!(
            "  {}: {}",
            RULE_DIGEST_CACHE_DIR,
            bytesize::ByteSize(rule_digests_size).display()
        )
        .as_str(),
    );
    logger(console.clone())
        .info(format!("  Total Size: {}", bytesize::ByteSize(total_size).display()).as_str());

    group.end_group(console.clone(), is_ci)?;
    Ok(())
}

fn remove_targets(targets: &[targets::Target]) -> anyhow::Result<()> {
    for target in targets {
        target
            .remove()
            .with_context(|| format_context!("Failed to remove target"))?;
    }
    Ok(())
}

/// Checks to see if the input digest exists in the cache.
///
/// if the input digest exists in the cache, populate the workspace outputs with
/// cached values.
///
/// if the input digest does not exist, the task is executed and the outputs are cached
/// if the task runs successfully.
pub fn execute<Exec, ExecSuccess, GetTargetPaths>(
    cache_path: &std::path::Path,
    rule_digest: Arc<str>,
    targets: &[targets::Target],
    exec: Exec,
    get_target_paths: GetTargetPaths,
) -> Option<anyhow::Result<ExecSuccess>>
where
    Exec: FnOnce() -> anyhow::Result<ExecSuccess>,
    GetTargetPaths: FnOnce() -> Vec<Arc<std::path::Path>>,
{
    let remove_result =
        remove_targets(targets).with_context(|| format_context!("while removing targets"));

    if let Err(e) = remove_result {
        return Some(Err(e));
    }

    let new_from_cache_result = RuleDigestCacheEntry::new_from_cache(cache_path, &rule_digest)
        .with_context(|| format_context!("Failed to check for cache entry for {rule_digest}"));

    match new_from_cache_result {
        Err(e) => Some(Err(e)),
        Ok(Some(entry)) => {
            // cache entry exists
            let result = entry
                .restore_to_workspace(cache_path)
                .with_context(|| format_context!("Failed to restore cached output to workspace"));
            if let Err(e) = result {
                return Some(Err(e));
            }

            // no result and no error, item restored from cache
            None
        }
        Ok(None) => {
            let exec_result = exec();

            if exec_result.is_ok() {
                let result = RuleDigestCacheEntry::create_cache_entry(
                    cache_path,
                    &rule_digest,
                    get_target_paths().as_slice(),
                )
                .with_context(|| format_context!("Failed to create cache entry for {rule_digest}"));

                if let Err(e) = result {
                    return Some(Err(e));
                }
            }

            Some(exec_result)
        }
    }
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

    // ── helpers for prune tests ─────────────────────────────────────────

    fn null_console() -> console::Console {
        console::Console::new_null()
    }

    /// Write a rule-digest cache entry directly to disk with a controlled
    /// `last_used` timestamp so we can deterministically make it "stale" or
    /// "fresh".
    fn write_digest_entry(
        cache_path: &std::path::Path,
        digest_name: &str,
        artifact_hashes: &[&str],
        last_used: age::LastUsed,
    ) {
        let outputs: Vec<CachedTarget> = artifact_hashes
            .iter()
            .map(|h| CachedTarget {
                path_in_cache: Arc::from(*h),
                path_in_workspace: Arc::from(format!("/fake/{h}")),
            })
            .collect();
        let entry = RuleDigestCacheEntry {
            last_used,
            outputs,
        };
        let encoded = postcard::to_stdvec(&entry).unwrap();
        let dir = cache_path.join(RULE_DIGEST_CACHE_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(digest_name), encoded).unwrap();
    }

    /// Place a fake artifact file in the artifacts/ subdirectory.
    fn write_artifact(cache_path: &std::path::Path, hash: &str, contents: &[u8]) {
        let dir = cache_path.join(ARTIFACT_CACHE_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(hash), contents).unwrap();
    }

    /// Place a fake staged file in the stage/ subdirectory.
    fn write_staged_file(cache_path: &std::path::Path, name: &str, contents: &[u8]) {
        let dir = cache_path.join(STAGE_CACHE_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), contents).unwrap();
    }

    /// Return a `LastUsed` that will report as `days_old` days old when
    /// `get_current_age()` is called.
    fn last_used_days_ago(days_old: u128) -> age::LastUsed {
        let now_millis = age::get_now();
        let millis_ago = days_old * 24 * 60 * 60 * 1000;
        age::LastUsed::new(now_millis.saturating_sub(millis_ago))
    }

    // ── prune tests ─────────────────────────────────────────────────────

    #[test]
    fn test_prune_empty_cache_is_noop() {
        let root = make_test_dir("prune_empty");
        let cache_path = root.join("cache");
        std::fs::create_dir_all(&cache_path).unwrap();

        let result = prune(&cache_path, 30, false, null_console(), ci::IsCi::No);
        assert!(result.is_ok());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_removes_stale_digest_and_leaves_fresh() {
        let root = make_test_dir("prune_stale_fresh");
        let cache_path = root.join("cache");

        // "stale_d" is 60 days old -> should be pruned with age=30
        write_digest_entry(&cache_path, "stale_d", &["art1"], last_used_days_ago(60));
        // "fresh_d" is 5 days old -> should survive
        write_digest_entry(&cache_path, "fresh_d", &["art2"], last_used_days_ago(5));

        write_artifact(&cache_path, "art1", b"artifact one");
        write_artifact(&cache_path, "art2", b"artifact two");

        prune(&cache_path, 30, false, null_console(), ci::IsCi::No).unwrap();

        let digests_dir = cache_path.join(RULE_DIGEST_CACHE_DIR);
        assert!(!digests_dir.join("stale_d").exists(), "stale digest should be removed");
        assert!(digests_dir.join("fresh_d").exists(), "fresh digest should remain");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_gc_removes_unreferenced_artifacts() {
        let root = make_test_dir("prune_gc");
        let cache_path = root.join("cache");

        // One fresh entry referencing "art_live"
        write_digest_entry(&cache_path, "fresh_d", &["art_live"], last_used_days_ago(1));

        // One stale entry referencing "art_stale"
        write_digest_entry(&cache_path, "stale_d", &["art_stale"], last_used_days_ago(90));

        // "art_orphan" is not referenced by any digest
        write_artifact(&cache_path, "art_live", b"live");
        write_artifact(&cache_path, "art_stale", b"stale");
        write_artifact(&cache_path, "art_orphan", b"orphan");

        prune(&cache_path, 30, false, null_console(), ci::IsCi::No).unwrap();

        let artifacts_dir = cache_path.join(ARTIFACT_CACHE_DIR);
        assert!(artifacts_dir.join("art_live").exists(), "live artifact should remain");
        assert!(!artifacts_dir.join("art_stale").exists(), "artifact only referenced by stale digest should be GC'd");
        assert!(!artifacts_dir.join("art_orphan").exists(), "orphan artifact should be GC'd");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_gc_keeps_artifact_referenced_by_any_live_digest() {
        let root = make_test_dir("prune_gc_shared");
        let cache_path = root.join("cache");

        // Both a stale and a fresh entry reference the same artifact
        write_digest_entry(&cache_path, "stale_d", &["shared_art"], last_used_days_ago(60));
        write_digest_entry(&cache_path, "fresh_d", &["shared_art"], last_used_days_ago(2));

        write_artifact(&cache_path, "shared_art", b"shared");

        prune(&cache_path, 30, false, null_console(), ci::IsCi::No).unwrap();

        let artifacts_dir = cache_path.join(ARTIFACT_CACHE_DIR);
        assert!(artifacts_dir.join("shared_art").exists(), "shared artifact should remain because a live digest still references it");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_dry_run_does_not_remove_anything() {
        let root = make_test_dir("prune_dry_run");
        let cache_path = root.join("cache");

        write_digest_entry(&cache_path, "stale_d", &["art1"], last_used_days_ago(90));
        write_artifact(&cache_path, "art1", b"artifact");
        write_artifact(&cache_path, "art_orphan", b"orphan");
        write_staged_file(&cache_path, "leftover", b"staged");

        prune(&cache_path, 30, true, null_console(), ci::IsCi::No).unwrap();

        let digests_dir = cache_path.join(RULE_DIGEST_CACHE_DIR);
        let artifacts_dir = cache_path.join(ARTIFACT_CACHE_DIR);
        let stage_dir = cache_path.join(STAGE_CACHE_DIR);

        assert!(digests_dir.join("stale_d").exists(), "dry run should not remove digest");
        assert!(artifacts_dir.join("art1").exists(), "dry run should not remove artifact");
        assert!(artifacts_dir.join("art_orphan").exists(), "dry run should not remove orphan artifact");
        assert!(stage_dir.join("leftover").exists(), "dry run should not remove staged file");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_dry_run_gc_treats_stale_digests_as_removed() {
        // After a dry-run prune, if we then do a real prune, the orphaned artifacts should be removed.
        // This test verifies the dry-run correctly identifies what *would* be orphaned
        // by checking that a subsequent real prune actually removes them.
        let root = make_test_dir("prune_dry_then_real");
        let cache_path = root.join("cache");

        // stale digest references "art_only_stale"
        write_digest_entry(&cache_path, "stale_d", &["art_only_stale"], last_used_days_ago(60));
        write_artifact(&cache_path, "art_only_stale", b"stale only");

        // Dry run: nothing removed
        prune(&cache_path, 30, true, null_console(), ci::IsCi::No).unwrap();
        assert!(cache_path.join(ARTIFACT_CACHE_DIR).join("art_only_stale").exists());

        // Real run: both digest and artifact removed
        prune(&cache_path, 30, false, null_console(), ci::IsCi::No).unwrap();
        assert!(!cache_path.join(RULE_DIGEST_CACHE_DIR).join("stale_d").exists());
        assert!(!cache_path.join(ARTIFACT_CACHE_DIR).join("art_only_stale").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_removes_corrupted_digest_entries() {
        let root = make_test_dir("prune_corrupted");
        let cache_path = root.join("cache");

        // Write a corrupted (non-postcard) digest entry
        let digests_dir = cache_path.join(RULE_DIGEST_CACHE_DIR);
        std::fs::create_dir_all(&digests_dir).unwrap();
        std::fs::write(digests_dir.join("corrupt_d"), b"not valid postcard").unwrap();

        // Also a fresh entry that should survive
        write_digest_entry(&cache_path, "fresh_d", &["art1"], last_used_days_ago(1));
        write_artifact(&cache_path, "art1", b"live artifact");

        prune(&cache_path, 30, false, null_console(), ci::IsCi::No).unwrap();

        assert!(!digests_dir.join("corrupt_d").exists(), "corrupted digest should be removed");
        assert!(digests_dir.join("fresh_d").exists(), "fresh digest should survive");
        assert!(cache_path.join(ARTIFACT_CACHE_DIR).join("art1").exists(), "artifact referenced by fresh digest should survive");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_sweeps_leftover_staged_files() {
        let root = make_test_dir("prune_stage_sweep");
        let cache_path = root.join("cache");

        // Create some staged files (simulating interrupted operations)
        write_staged_file(&cache_path, "incomplete_1", b"partial");
        write_staged_file(&cache_path, "incomplete_2", b"partial too");

        prune(&cache_path, 30, false, null_console(), ci::IsCi::No).unwrap();

        let stage_dir = cache_path.join(STAGE_CACHE_DIR);
        assert!(!stage_dir.join("incomplete_1").exists(), "staged file should be swept");
        assert!(!stage_dir.join("incomplete_2").exists(), "staged file should be swept");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_all_stale_leaves_empty_dirs() {
        let root = make_test_dir("prune_all_stale");
        let cache_path = root.join("cache");

        write_digest_entry(&cache_path, "d1", &["a1", "a2"], last_used_days_ago(100));
        write_digest_entry(&cache_path, "d2", &["a3"], last_used_days_ago(200));

        write_artifact(&cache_path, "a1", b"one");
        write_artifact(&cache_path, "a2", b"two");
        write_artifact(&cache_path, "a3", b"three");

        prune(&cache_path, 30, false, null_console(), ci::IsCi::No).unwrap();

        // All digests removed
        let digests_dir = cache_path.join(RULE_DIGEST_CACHE_DIR);
        let remaining_digests: Vec<_> = std::fs::read_dir(&digests_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(remaining_digests.is_empty(), "all stale digests should be removed");

        // All artifacts removed
        let artifacts_dir = cache_path.join(ARTIFACT_CACHE_DIR);
        let remaining_artifacts: Vec<_> = std::fs::read_dir(&artifacts_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(remaining_artifacts.is_empty(), "all artifacts should be GC'd when no digests remain");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_nonexistent_cache_path_is_noop() {
        let root = make_test_dir("prune_nonexist");
        let cache_path = root.join("no_such_dir");

        // prune should succeed even if the cache directory doesn't exist
        let result = prune(&cache_path, 30, false, null_console(), ci::IsCi::No);
        assert!(result.is_ok());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_prune_age_boundary() {
        let root = make_test_dir("prune_boundary");
        let cache_path = root.join("cache");

        // Exactly at the boundary: age == threshold (30 days).
        // get_current_age returns days as integer division, so 30 * 86400000 ms ago
        // should report age == 30. The check is entry_age > age, so 30 > 30 is false
        // → entry should survive.
        write_digest_entry(&cache_path, "boundary_d", &["art_b"], last_used_days_ago(30));
        // One day over the threshold → should be pruned
        write_digest_entry(&cache_path, "over_d", &["art_o"], last_used_days_ago(31));

        write_artifact(&cache_path, "art_b", b"boundary");
        write_artifact(&cache_path, "art_o", b"over");

        prune(&cache_path, 30, false, null_console(), ci::IsCi::No).unwrap();

        let digests_dir = cache_path.join(RULE_DIGEST_CACHE_DIR);
        assert!(digests_dir.join("boundary_d").exists(), "entry at exactly the age threshold should survive (not strictly greater)");
        assert!(!digests_dir.join("over_d").exists(), "entry one day over the threshold should be pruned");

        let artifacts_dir = cache_path.join(ARTIFACT_CACHE_DIR);
        assert!(artifacts_dir.join("art_b").exists(), "boundary artifact should survive");
        assert!(!artifacts_dir.join("art_o").exists(), "over-threshold artifact should be GC'd");

        let _ = std::fs::remove_dir_all(&root);
    }
}
