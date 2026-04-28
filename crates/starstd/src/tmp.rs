use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;

use crate::is_lsp_mode;
use starlark::values::none::NoneType;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
enum TempKind {
    File,
    Dir,
}

#[derive(Debug, Clone)]
struct TempEntry {
    path: PathBuf,
    kind: TempKind,
    keep: bool,
}

// Registry is keyed by the canonical path string so that cleanup(path) works
// without ever exposing an opaque integer handle to callers.
static TMP_REGISTRY: OnceLock<Mutex<HashMap<String, TempEntry>>> = OnceLock::new();

fn tmp_registry() -> &'static Mutex<HashMap<String, TempEntry>> {
    TMP_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_temp(path: PathBuf, kind: TempKind, keep: bool) -> anyhow::Result<()> {
    let key = path.to_string_lossy().to_string();
    let entry = TempEntry { path, kind, keep };
    tmp_registry()
        .lock()
        .map_err(|_| anyhow::anyhow!("temp registry lock poisoned"))?
        .insert(key, entry);
    Ok(())
}

fn cleanup_entry(entry: &TempEntry) -> anyhow::Result<()> {
    match entry.kind {
        TempKind::File => {
            if entry.path.exists() {
                std::fs::remove_file(&entry.path).context(format_context!(
                    "Failed to remove temporary file {}",
                    entry.path.display()
                ))?;
            }
        }
        TempKind::Dir => {
            if entry.path.exists() {
                std::fs::remove_dir_all(&entry.path).context(format_context!(
                    "Failed to remove temporary directory {}",
                    entry.path.display()
                ))?;
            }
        }
    }
    Ok(())
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Create a temporary directory and register it for later cleanup.
    ///
    /// The directory is created in the system's default temp location using
    /// cryptographically random bytes for the unique part of its name, so
    /// collisions are not possible in practice.  On Unix the directory is
    /// created with mode 0700 (owner-only access).
    ///
    /// Returns the created directory path as a string.
    ///
    /// Example:
    ///   d = tmp.dir(prefix = "build-")
    fn dir(
        #[starlark(require = named, default = "tmp-".to_owned())] prefix: String,
    ) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }

        // tempfile::Builder creates with mode 0700 on Unix, using OS-level
        // random bytes, giving far stronger uniqueness than a counter.
        let tmp_dir =
            tempfile::Builder::new()
                .prefix(&prefix)
                .tempdir()
                .context(format_context!(
                    "Failed to create temporary directory with prefix {:?}",
                    prefix
                ))?;

        // Cancel RAII cleanup — our registry owns cleanup from here on.
        let path = tmp_dir.keep();
        register_temp(path.clone(), TempKind::Dir, false)?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Create a temporary directory that will NOT be automatically cleaned up.
    ///
    /// Identical to `tmp.dir` except that `tmp.cleanup_all()` will leave this
    /// directory in place.  Useful for caches or artefacts that must survive
    /// the script.
    ///
    /// Returns the created directory path as a string.
    ///
    /// Example:
    ///   d = tmp.dir_keep(prefix = "cache-")
    fn dir_keep(
        #[starlark(require = named, default = "tmp-".to_owned())] prefix: String,
    ) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }

        let tmp_dir =
            tempfile::Builder::new()
                .prefix(&prefix)
                .tempdir()
                .context(format_context!(
                    "Failed to create temporary directory with prefix {:?}",
                    prefix
                ))?;

        let path = tmp_dir.keep();
        register_temp(path.clone(), TempKind::Dir, true)?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Create a temporary file and register it for later cleanup.
    ///
    /// The file is created in the system's default temp location with a
    /// randomly generated name.  On Unix the file is created with mode 0600
    /// (owner-only access).  The file starts empty.
    ///
    /// Returns the created file path as a string.
    ///
    /// Example:
    ///   f = tmp.file(suffix = ".log")
    fn file(
        #[starlark(require = named, default = "".to_owned())] suffix: String,
    ) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }

        // tempfile creates with mode 0600 on Unix and uses random bytes for
        // the name, eliminating both the permission and collision concerns.
        let named = tempfile::Builder::new()
            .prefix("tmp-")
            .suffix(&suffix)
            .tempfile()
            .context(format_context!(
                "Failed to create temporary file with suffix {:?}",
                suffix
            ))?;

        // Persist the file so it survives beyond the NamedTempFile drop;
        // our registry is responsible for cleanup from this point on.
        let (_, path) = named.keep().map_err(|e| {
            anyhow::anyhow!(
                "Failed to persist temporary file with suffix {:?}: {}",
                suffix,
                e
            )
        })?;

        register_temp(path.clone(), TempKind::File, false)?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Immediately clean up a single tracked temporary resource.
    ///
    /// Pass the path string that was returned by `tmp.dir`, `tmp.dir_keep`,
    /// or `tmp.file`.  Raises an error if the path is not tracked.
    ///
    /// Example:
    ///   f = tmp.file(suffix = ".txt")
    ///   # ... use f ...
    ///   tmp.cleanup(f)
    fn cleanup(path: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        let entry = {
            let mut map = tmp_registry()
                .lock()
                .map_err(|_| anyhow::anyhow!("temp registry lock poisoned"))?;
            map.remove(path)
                .context(format_context!("Unknown temp path (not tracked): {}", path))?
        };

        cleanup_entry(&entry)?;
        Ok(NoneType)
    }

    /// Clean up all tracked temporary resources that are not marked keep=true.
    ///
    /// Resources created with `tmp.dir_keep` are skipped.  All other tracked
    /// resources are removed from the registry and deleted from disk before
    /// this function returns.
    ///
    /// Unlike a fail-fast approach, cleanup continues for every entry even if
    /// an individual deletion fails; all errors are then reported together so
    /// that no entry is silently leaked.
    ///
    /// Example:
    ///   tmp.cleanup_all()
    fn cleanup_all() -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        // Atomically drain all non-keep entries from the registry in a single
        // lock acquisition, then clean up without holding the lock so that
        // Starlark callbacks (if any) can still reach the registry.
        let to_clean: Vec<TempEntry> = {
            let mut map = tmp_registry()
                .lock()
                .map_err(|_| anyhow::anyhow!("temp registry lock poisoned"))?;
            let to_clean: Vec<TempEntry> = map.values().filter(|v| !v.keep).cloned().collect();
            map.retain(|_, v| v.keep);
            to_clean
        };

        // Best-effort: attempt every deletion and collect all failures.
        let mut errors: Vec<String> = Vec::new();
        for entry in &to_clean {
            if let Err(e) = cleanup_entry(entry) {
                errors.push(format!("{}: {}", entry.path.display(), e));
            }
        }

        if errors.is_empty() {
            Ok(NoneType)
        } else {
            Err(anyhow::anyhow!(
                "cleanup_all encountered {} error(s):\n{}",
                errors.len(),
                errors.join("\n")
            ))
        }
    }
}
