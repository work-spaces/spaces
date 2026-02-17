/// Rule cache
/// Cache the outputs of the rule based on the input digest
use crate::age;
use anyhow::Context;
use anyhow_source_location::format_context;
use bincode::{Decode, Encode};
use std::sync::Arc;

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
    let path_in_cache = cache_path.join(&artifact_hash);
    // skip caching if the artifact is already in the cache
    if !path_in_cache.exists() {
        // save the artifact to the cache
        reflink_copy::reflink_or_copy(artifact_path, path_in_cache.join(&artifact_hash))?;

        let artifact_metadata = std::fs::metadata(artifact_path)
            .with_context(|| format_context!("Failed to get metadata for {artifact_path:?}"))?;

        // Update the metadata to be read-only
        let mut read_write_permissions = artifact_metadata.permissions();

        #[allow(clippy::permissions_set_readonly_false)]
        read_write_permissions.set_readonly(false);

        // Set the permissions to read-write
        std::fs::set_permissions(artifact_path, read_write_permissions).context(
            format_context!("Failed to set permissions for {}", artifact_path.display()),
        )?;
    }

    Ok(artifact_hash.into())
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CachedOutput {
    // where does the artifact exist in the cache
    pub path_in_cache: Arc<str>,
    // where should the artifact be restored in the workspace
    pub path_in_workspace: Arc<str>,
}

impl CachedOutput {
    pub fn new_from_workspace_path(
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

    pub fn restore_to_workspace(&self) -> anyhow::Result<()> {
        // hard link to the workspace
        let path_in_cache = std::path::Path::new(self.path_in_cache.as_ref());
        let path_in_workspace = std::path::Path::new(self.path_in_workspace.as_ref());
        std::fs::hard_link(path_in_cache, path_in_workspace).with_context(|| {
            format_context!(
                "Failed to restore artifact to workspace at {}",
                path_in_workspace.display()
            )
        })?;
        Ok(())
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CacheEntry {
    pub last_used: age::LastUsed,
    pub outputs: Vec<CachedOutput>,
}

impl CacheEntry {
    /// loads an entry from cache if it exists
    pub fn new_from_cache(
        cache_path: &std::path::Path,
        rule_digest: &str,
    ) -> anyhow::Result<Option<Self>> {
        let path_in_cache = cache_path.join(rule_digest);
        if path_in_cache.exists() {
            let contents = std::fs::read(&path_in_cache).with_context(|| {
                format_context!(
                    "While trying to open cache file for rule digest {}",
                    rule_digest
                )
            })?;

            let (mut entry, _size) = bincode::decode_from_slice::<
                Self,
                bincode::config::Configuration,
            >(contents.as_slice(), bincode::config::standard())
            .with_context(|| {
                format_context!(
                    "Failed to decode cache entry for rule digest {}",
                    rule_digest
                )
            })?;

            // update last used and save the entry
            entry.last_used.update();
            let encoded = bincode::encode_to_vec(&entry, bincode::config::standard())
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

    pub fn create_cache_entry(
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

        let encoded = bincode::encode_to_vec(&entry, bincode::config::standard())
            .context(format_context!("Failed to encode rcache entry"))?;

        let path_in_cache = cache_path.join(rule_digest);
        std::fs::write(path_in_cache, encoded).context(format_context!(
            "Failed to write cache entry for rule digest {}",
            rule_digest
        ))?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct TaskCache {
    pub cache_path: Arc<std::path::Path>,
}

impl TaskCache {
    /// Checks to see if the input digest exists in the cache.
    ///
    /// if the input digest exists in the cache, populate the workspace outputs with
    /// cached values.
    ///
    /// if the input digest does not exist, the task is executed and the outputs are cached
    /// if the task runs successfully.
    pub fn execute_task<Exec>(
        &mut self,
        rule_digest: Arc<str>,
        outputs: Vec<Arc<std::path::Path>>,
        exec: Exec,
    ) -> anyhow::Result<()>
    where
        Exec: FnOnce() -> anyhow::Result<()>,
    {
        if let Some(entry) = CacheEntry::new_from_cache(&self.cache_path, &rule_digest)
            .context(format_context!("Failed to check for cache entry"))?
        {
            // cache entry exists
            entry
                .restore_to_workspace()
                .with_context(|| format_context!("Failed to restore cached output to workspace"))?;
        } else {
            exec().with_context(|| format_context!("Task failed to execute in rule cacher"))?;

            CacheEntry::create_cache_entry(&self.cache_path, &rule_digest, outputs.as_slice())
                .context(format_context!("Failed to create cache entry"))?;
        }

        Ok(())
    }
}
