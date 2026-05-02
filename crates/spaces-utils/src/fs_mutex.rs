use crate::lock;
use anyhow::Context;
use anyhow_source_location::format_context;
use std::sync::Arc;

pub struct FsMutex {
    path: Arc<std::path::Path>,
    lock: lock::FileLock,
}

impl FsMutex {
    /// Creates a new `FsMutex` for the given file path.
    ///
    /// The companion lock file is located at `{path}.{LOCK_FILE_SUFFIX}`.
    pub fn new(path: Arc<std::path::Path>) -> Self {
        let lock_path_string = format!("{}.{}", path.display(), lock::LOCK_FILE_SUFFIX);
        let lock_path: Arc<std::path::Path> = std::path::Path::new(&lock_path_string).into();
        let lock = lock::FileLock::new(lock_path);
        Self { path, lock }
    }

    /// Returns a reference to the wrapped file path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn with_lock<F, T>(&mut self, f: F) -> anyhow::Result<T>
    where
        F: FnOnce(&std::path::Path) -> anyhow::Result<T>,
    {
        let console = console::Console::new_null();
        self.lock.lock(console).context(format_context!(
            "Failed to acquire lock for {}",
            self.path.display()
        ))?;

        let result = f(self.path.as_ref());

        let unlock_result = self.lock.unlock().context(format_context!(
            "Failed to release lock for {}",
            self.path.display()
        ));

        match result {
            Ok(value) => {
                unlock_result?;
                Ok(value)
            }
            Err(e) => Err(e),
        }
    }

    pub fn read_to_string(&mut self) -> anyhow::Result<String> {
        self.with_lock(|path| {
            std::fs::read_to_string(path)
                .context(format_context!("Failed to read {}", path.display()))
        })
    }

    pub fn write_bytes(&mut self, content: &[u8]) -> anyhow::Result<()> {
        self.with_lock(|path| {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).context(format_context!(
                    "Failed to create parent directory for {}",
                    path.display()
                ))?;
            }
            std::fs::write(path, content)
                .context(format_context!("Failed to write {}", path.display()))
        })
    }

    pub fn write_str(&mut self, content: &str) -> anyhow::Result<()> {
        self.write_bytes(content.as_bytes())
    }
}
