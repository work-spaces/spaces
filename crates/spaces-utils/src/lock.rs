use crate::logger;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::{
    io::{Seek, Write},
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

pub const LOCK_FILE_SUFFIX: &str = "spaces.lock";
const DEFAULT_EXPIRATION_SECONDS: u16 = 10;

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

fn get_millis_now() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn get_expiration() -> u128 {
    get_millis_now() + (DEFAULT_EXPIRATION_SECONDS as u128 * 1000)
}

fn is_expired(expiration: u128) -> bool {
    get_millis_now() > expiration
}

#[derive(Debug, Serialize, Deserialize)]
struct LockFileContents {
    expiration: u128,
}

impl Default for LockFileContents {
    fn default() -> Self {
        Self {
            expiration: get_expiration(),
        }
    }
}

impl LockFileContents {
    fn is_expired(&self) -> bool {
        is_expired(self.expiration)
    }

    fn serialize(&self) -> anyhow::Result<String> {
        serde_json::to_string(&self).context("Failed to serialize lock file contents")
    }
}

#[derive(Debug, PartialEq)]
pub enum LockStatus {
    Locked,
    Busy,
}

#[derive(Debug)]
pub struct FileLock {
    pub path: Arc<std::path::Path>,
    is_locked: StateLock<bool>,
    renew_handle: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
}

impl FileLock {
    pub fn new(path: Arc<std::path::Path>) -> Self {
        Self {
            path,
            is_locked: StateLock::new(false),
            renew_handle: None,
        }
    }

    pub fn try_lock(&mut self) -> anyhow::Result<LockStatus> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .context(format_context!("Failed to create {}", parent.display()))?;
        }

        match std::fs::OpenOptions::new()
            .write(true) // Open for writing
            .create_new(true) // Create only if it does NOT exist
            .open(self.path.as_ref())
        {
            Ok(mut file) => {
                let contents = LockFileContents::default()
                    .serialize()
                    .context(format_context!("Failed to serialize lock file contents"))?;
                file.write_all(contents.as_bytes())
                    .context(format_context!("Failed to write {}", self.path.display()))?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                // wait in case the file was just created but not written yet
                std::thread::sleep(std::time::Duration::from_millis(100));
                let contents_result = std::fs::read_to_string(self.path.as_ref())
                    .context(format_context!("Failed to read {}", self.path.display()));

                if let Ok(contents) = contents_result {
                    let existing_info_result = serde_json::from_str::<LockFileContents>(&contents);
                    let is_remove = match existing_info_result {
                        Ok(existing_info) => existing_info.is_expired(),
                        Err(_) => true,
                    };

                    if is_remove {
                        std::fs::remove_file(self.path.as_ref()).context(format_context!(
                            "Failed to remove expired lock file {}",
                            self.path.display()
                        ))?;
                    }

                    return Ok(LockStatus::Busy);
                } else {
                    // an error reading the file contents means the file may have been deleted
                    // call it busy and try again later
                    return Ok(LockStatus::Busy);
                }
            }
            Err(err) => {
                return Err(format_error!(
                    "Failed to create file '{}': {err:?} - delete the file and try again",
                    self.path.display()
                ));
            }
        }
        {
            let mut write_lock = self.is_locked.write();
            *write_lock = true;
        }

        let renew_path = self.path.clone();
        let is_locked = self.is_locked.clone();
        self.renew_handle = Some(std::thread::spawn(move || {
            // periodically updates the expiration time until the lock is released
            renew_lock(renew_path, is_locked)
        }));

        Ok(LockStatus::Locked)
    }

    pub fn lock(&mut self, progress: &mut printer::MultiProgressBar) -> anyhow::Result<()> {
        while self
            .try_lock()
            .context(format_context!("Failed to try lock"))?
            == LockStatus::Busy
        {
            self.wait(progress)
                .context(format_context!("failed to wait for lock"))?;
        }
        Ok(())
    }

    pub fn unlock(&mut self) -> anyhow::Result<()> {
        {
            let mut is_locked_write = self.is_locked.write();
            *is_locked_write = false;
        }

        if let Some(handle) = self.renew_handle.take() {
            let _ = handle.join();
        }

        std::fs::remove_file(self.path.as_ref()).context(format_context!(
            "Failed to remove lock file {}",
            self.path.display()
        ))?;
        Ok(())
    }

    fn wait(&self, progress: &mut printer::MultiProgressBar) -> anyhow::Result<()> {
        progress.set_message("already started, waiting for it to finish");
        let lock_file_path = std::path::Path::new(self.path.as_ref());
        let mut log_count = 0;
        while lock_file_path.exists() {
            // another process may have just created this file.
            // This delay gives the other process time to finish creating the file.
            std::thread::sleep(std::time::Duration::from_millis(100));

            // the holding process may have deleted the file by now.
            let contents_result = std::fs::read_to_string(lock_file_path);

            if let Ok(contents) = contents_result {
                let lock_info: LockFileContents =
                    serde_json::from_str(&contents).context(format_context!(
                        "failed to parse {} - delete the file and try again",
                        self.path.display()
                    ))?;

                if lock_info.is_expired() {
                    return Ok(());
                }
            }

            progress.increment(1);
            std::thread::sleep(std::time::Duration::from_millis(500));
            log_count += 1;
            if log_count == 10 {
                logger::Logger::new_progress(progress, "lock".into()).debug(
                    format!("Still waiting for it to finish at {}", self.path.display()).as_str(),
                );
                log_count = 0;
            }
        }
        Ok(())
    }
}

fn renew_lock(renew_path: Arc<std::path::Path>, is_locked: StateLock<bool>) -> anyhow::Result<()> {
    let mut is_unlocked = false;
    while !is_unlocked {
        {
            let is_locked_write = is_locked.write();
            if *is_locked_write {
                // open the file and update the contents
                let contents = LockFileContents::default()
                    .serialize()
                    .context(format_context!("Failed to serialize lock file contents"))?;

                // open file for read/write
                let mut file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(renew_path.as_ref())
                    .context(format_context!(
                        "Failed to open lock file {} for renewal",
                        renew_path.display()
                    ))?;

                // truncate contents
                file.set_len(0).context(format_context!(
                    "Failed to truncate {}",
                    renew_path.display()
                ))?;

                file.seek(std::io::SeekFrom::Start(0))
                    .context(format_context!(
                        "Failed to seek to start of {}",
                        renew_path.display()
                    ))?;

                file.write_all(contents.as_bytes())
                    .context(format_context!("Failed to write {}", renew_path.display()))?;
            } else {
                is_unlocked = true;
            }
        }

        // quickly sample the lock to see if renewal is still needed
        for _ in 0..DEFAULT_EXPIRATION_SECONDS * 10 / 2 {
            let mut is_wait = false;
            {
                let is_locked_read = is_locked.read();
                if *is_locked_read {
                    is_wait = true;
                }
            }

            if is_wait {
                std::thread::sleep(std::time::Duration::from_millis(100));
            } else {
                is_unlocked = true;
            }
        }
    }

    Ok(())
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.unlock();
    }
}
