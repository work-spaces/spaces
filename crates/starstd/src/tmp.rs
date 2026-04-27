use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;

use crate::is_lsp_mode;
use starlark::values::none::NoneType;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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

static TMP_REGISTRY: OnceLock<Mutex<HashMap<u64, TempEntry>>> = OnceLock::new();
static NEXT_TMP_ID: AtomicU64 = AtomicU64::new(1);

fn tmp_registry() -> &'static Mutex<HashMap<u64, TempEntry>> {
    TMP_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_temp(path: PathBuf, kind: TempKind, keep: bool) -> anyhow::Result<u64> {
    let id = NEXT_TMP_ID.fetch_add(1, Ordering::Relaxed);
    let entry = TempEntry { path, kind, keep };

    let mut map = tmp_registry()
        .lock()
        .map_err(|_| anyhow::anyhow!("temp registry lock poisoned"))?;
    map.insert(id, entry);
    Ok(id)
}

fn cleanup_path(path: &PathBuf, kind: &TempKind) -> anyhow::Result<NoneType> {
    match kind {
        TempKind::File => {
            if path.exists() {
                std::fs::remove_file(path).context(format_context!(
                    "Failed to remove temporary file {}",
                    path.display()
                ))?;
            }
        }
        TempKind::Dir => {
            if path.exists() {
                std::fs::remove_dir_all(path).context(format_context!(
                    "Failed to remove temporary directory {}",
                    path.display()
                ))?;
            }
        }
    }
    Ok(NoneType)
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Create a temporary directory path and register it for later cleanup.
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

        let base = std::env::temp_dir();
        let unique = NEXT_TMP_ID.load(Ordering::Relaxed);
        let dirname = format!("{}{}", prefix, unique);
        let path = base.join(dirname);

        std::fs::create_dir_all(&path).context(format_context!(
            "Failed to create temporary directory {}",
            path.display()
        ))?;

        let _ = register_temp(path.clone(), TempKind::Dir, false)?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Create a temporary directory path and do not register cleanup.
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

        let base = std::env::temp_dir();
        let unique = NEXT_TMP_ID.fetch_add(1, Ordering::Relaxed);
        let dirname = format!("{}{}", prefix, unique);
        let path = base.join(dirname);

        std::fs::create_dir_all(&path).context(format_context!(
            "Failed to create temporary directory {}",
            path.display()
        ))?;

        let _ = register_temp(path.clone(), TempKind::Dir, true)?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Create a temporary file and register it for later cleanup.
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

        let base = std::env::temp_dir();
        let unique = NEXT_TMP_ID.fetch_add(1, Ordering::Relaxed);
        let filename = format!("tmp-{}{}", unique, suffix);
        let path = base.join(filename);

        let _f = std::fs::File::create(&path).context(format_context!(
            "Failed to create temporary file {}",
            path.display()
        ))?;

        let _ = register_temp(path.clone(), TempKind::File, false)?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Cleanup a tracked temp resource by id.
    ///
    /// Useful for explicit cleanup of long-lived temps.
    fn cleanup(id: u64) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        let entry = {
            let mut map = tmp_registry()
                .lock()
                .map_err(|_| anyhow::anyhow!("temp registry lock poisoned"))?;
            map.remove(&id)
                .context(format_context!("Invalid temp handle: {}", id))?
        };

        cleanup_path(&entry.path, &entry.kind)
    }

    /// Cleanup all tracked temporary resources that are not marked keep=true.
    ///
    /// Call this at script end to emulate "auto cleanup at exit".
    fn cleanup_all() -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        let entries = {
            let mut map = tmp_registry()
                .lock()
                .map_err(|_| anyhow::anyhow!("temp registry lock poisoned"))?;
            let values: Vec<TempEntry> = map.values().cloned().collect();
            map.retain(|_, v| v.keep);
            values
        };

        for entry in entries {
            if !entry.keep {
                cleanup_path(&entry.path, &entry.kind)?;
            }
        }

        Ok(NoneType)
    }
}
