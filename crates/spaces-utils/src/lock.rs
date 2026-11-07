use crate::logger;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub const LOCK_FILE_SUFFIX: &str = "spaces.lock";
const LOCK_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
const MAX_REPLACED_FILE_RETRIES: usize = 3;

#[derive(Debug, Clone)]
pub struct StateLock<ModuleState: std::fmt::Debug> {
    lock: Arc<RwLock<ModuleState>>,
}

impl<ModuleState: std::fmt::Debug> StateLock<ModuleState> {
    pub fn new(state: ModuleState) -> Self {
        Self {
            lock: Arc::new(RwLock::new(state)),
        }
    }

    pub fn read(&self) -> RwLockReadGuard<'_, ModuleState> {
        self.lock.read().unwrap_or_else(|_| {
            panic!(
                "Internal error: failed to get read lock for {:?}",
                self.lock
            )
        })
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, ModuleState> {
        self.lock.write().unwrap_or_else(|_| {
            panic!(
                "Internal error: failed to get write lock for {:?}",
                self.lock
            )
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum LockStatus {
    Locked,
    Busy,
}

/// An exclusive, advisory, cross-process lock backed by `std::fs::File` locking.
///
/// The OS releases the lock automatically if the holding process exits or crashes.
/// Each `FileLock` opens its own file handle, so instances within the same process
/// (e.g. on different threads) also exclude each other.
///
/// The lock file is never deleted on unlock. It may only be deleted while holding
/// the lock (see [`remove_if_unlocked`]). After acquiring the lock, `try_lock`
/// verifies the locked handle still refers to the file at `path`, so a process that
/// opened a file which was subsequently deleted retries instead of "holding" a lock
/// on an unlinked file.
#[derive(Debug)]
pub struct FileLock {
    pub path: Arc<std::path::Path>,
    file: Option<std::fs::File>,
}

impl FileLock {
    pub fn new(path: Arc<std::path::Path>) -> Self {
        Self { path, file: None }
    }

    pub fn try_lock(&mut self) -> anyhow::Result<LockStatus> {
        if self.file.is_some() {
            return Ok(LockStatus::Locked);
        }

        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .context(format_context!("Failed to create {}", parent.display()))?;
        }

        for _ in 0..MAX_REPLACED_FILE_RETRIES {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(self.path.as_ref())
                .context(format_context!(
                    "Failed to open lock file {}",
                    self.path.display()
                ))?;

            match file.try_lock() {
                Ok(()) => {
                    if is_same_file(&file, self.path.as_ref()) {
                        self.file = Some(file);
                        return Ok(LockStatus::Locked);
                    }
                    // The lock file was removed or replaced after it was opened.
                    // Dropping `file` releases the stale lock; retry with the current file.
                }
                Err(std::fs::TryLockError::WouldBlock) => return Ok(LockStatus::Busy),
                Err(std::fs::TryLockError::Error(err)) => {
                    return Err(format_error!(
                        "Failed to lock file '{}': {err:?}",
                        self.path.display()
                    ));
                }
            }
        }

        Ok(LockStatus::Busy)
    }

    pub fn lock(&mut self, console: console::Console) -> anyhow::Result<()> {
        if self
            .try_lock()
            .context(format_context!("Failed to try lock"))?
            == LockStatus::Locked
        {
            return Ok(());
        }

        // Poll rather than block so the progress indicator stays responsive.
        let progress = console::Progress::new(console.clone(), self.path.display(), None, None);
        progress.set_prefix("file locked resource");
        progress.set_message("already started, waiting for it to finish");
        let logger = logger::Logger::new(progress.console.clone(), "lock".into());
        let mut log_count = 0;
        loop {
            progress.increment_progress();
            std::thread::sleep(LOCK_POLL_INTERVAL);

            if self
                .try_lock()
                .context(format_context!("Failed to try lock"))?
                == LockStatus::Locked
            {
                return Ok(());
            }

            log_count += 1;
            if log_count == 10 {
                logger.debug(
                    format!("Still waiting for it to finish at {}", self.path.display()).as_str(),
                );
                log_count = 0;
            }
        }
    }

    pub fn unlock(&mut self) -> anyhow::Result<()> {
        if let Some(file) = self.file.take() {
            file.unlock().context(format_context!(
                "Failed to unlock lock file {}",
                self.path.display()
            ))?;
        }
        Ok(())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.unlock();
    }
}

#[cfg(unix)]
fn is_same_file(file: &std::fs::File, path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(locked), Ok(current)) => locked.dev() == current.dev() && locked.ino() == current.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn is_same_file(_file: &std::fs::File, _path: &std::path::Path) -> bool {
    // Lock files are never deleted on non-unix platforms (see `remove_if_unlocked`).
    true
}

/// Removes the lock file at `path` if no process currently holds it.
///
/// The file is deleted while this function holds the lock, and `FileLock::try_lock`
/// re-validates file identity after locking, so concurrent lockers remain safe.
/// Returns `true` if the file was removed.
///
/// On non-unix platforms file identity can't be verified, so this is a no-op.
pub fn remove_if_unlocked(path: &std::path::Path) -> anyhow::Result<bool> {
    if cfg!(not(unix)) {
        return Ok(false);
    }

    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(format_error!(
                "Failed to open lock file '{}': {err:?}",
                path.display()
            ));
        }
    };

    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Ok(false),
        Err(std::fs::TryLockError::Error(err)) => {
            return Err(format_error!(
                "Failed to lock file '{}': {err:?}",
                path.display()
            ));
        }
    }

    if !is_same_file(&file, path) {
        return Ok(false);
    }

    std::fs::remove_file(path).context(format_context!(
        "Failed to remove lock file {}",
        path.display()
    ))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_path(dir: &tempfile::TempDir) -> Arc<std::path::Path> {
        dir.path()
            .join("nested")
            .join(format!("test.{LOCK_FILE_SUFFIX}"))
            .into()
    }

    #[test]
    fn test_try_lock_excludes_other_instances() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(&dir);

        let mut first = FileLock::new(path.clone());
        let mut second = FileLock::new(path.clone());

        assert_eq!(first.try_lock().unwrap(), LockStatus::Locked);
        assert_eq!(first.try_lock().unwrap(), LockStatus::Locked);
        assert_eq!(second.try_lock().unwrap(), LockStatus::Busy);

        first.unlock().unwrap();
        first.unlock().unwrap();
        assert!(path.exists());
        assert_eq!(second.try_lock().unwrap(), LockStatus::Locked);
    }

    #[test]
    fn test_drop_releases_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(&dir);

        {
            let mut first = FileLock::new(path.clone());
            assert_eq!(first.try_lock().unwrap(), LockStatus::Locked);
        }

        let mut second = FileLock::new(path);
        assert_eq!(second.try_lock().unwrap(), LockStatus::Locked);
    }

    #[test]
    fn test_legacy_lock_file_does_not_block() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"expiration":340282366920938463463374607431768211455}"#,
        )
        .unwrap();

        let mut lock = FileLock::new(path);
        assert_eq!(lock.try_lock().unwrap(), LockStatus::Locked);
    }

    #[test]
    fn test_lock_waits_for_release() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(&dir);

        let mut holder = FileLock::new(path.clone());
        assert_eq!(holder.try_lock().unwrap(), LockStatus::Locked);

        let waiter_path = path.clone();
        let waiter = std::thread::spawn(move || {
            let mut waiter = FileLock::new(waiter_path);
            waiter.lock(console::Console::new_null()).unwrap();
            std::time::Instant::now()
        });

        std::thread::sleep(std::time::Duration::from_millis(200));
        let released_at = std::time::Instant::now();
        holder.unlock().unwrap();

        let acquired_at = waiter.join().unwrap();
        assert!(acquired_at >= released_at);
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_if_unlocked() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(&dir);

        assert!(!remove_if_unlocked(&path).unwrap());

        let mut holder = FileLock::new(path.clone());
        assert_eq!(holder.try_lock().unwrap(), LockStatus::Locked);
        assert!(!remove_if_unlocked(&path).unwrap());
        assert!(path.exists());

        holder.unlock().unwrap();
        assert!(remove_if_unlocked(&path).unwrap());
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_try_lock_rejects_removed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = lock_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();

        // Simulates a waiter that opened the old file before it was deleted
        let stale = std::fs::File::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        stale.try_lock().unwrap();
        assert!(!is_same_file(&stale, &path));

        let mut lock = FileLock::new(path.clone());
        assert_eq!(lock.try_lock().unwrap(), LockStatus::Locked);
        assert!(is_same_file(lock.file.as_ref().unwrap(), &path));
    }
}
