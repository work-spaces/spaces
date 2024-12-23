use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Debug)]
pub struct StateLock<ModuleState: std::fmt::Debug> {
    lock: RwLock<ModuleState>,
}

impl<ModuleState: std::fmt::Debug> StateLock<ModuleState> {
    pub fn new(state: ModuleState) -> Self {
        Self {
            lock: RwLock::new(state),
        }
    }

    pub fn read(&self) -> RwLockReadGuard<ModuleState> {
        self.lock.read().unwrap_or_else(|_| {
            panic!(
                "Internal error: failed to get read lock for {:?}",
                self.lock
            )
        })
    }

    pub fn write(&self) -> RwLockWriteGuard<ModuleState> {
        self.lock.write().unwrap_or_else(|_| {
            panic!(
                "Internal error: failed to get write lock for {:?}",
                self.lock
            )
        })
    }
}

pub fn get_process_group_id_env_name() -> &'static str {
    const SPACES_PROCESS_GROUP_ENV_VAR: &str = "SPACES_PROCESS_GROUP_ID";
    SPACES_PROCESS_GROUP_ENV_VAR
}

pub fn get_process_group_id() -> Arc<str> {
    if let Ok(process_group_id) = std::env::var(get_process_group_id_env_name()) {
        process_group_id.into()
    } else {
        // create process ID from system time
        format!("{}", chrono::Utc::now().timestamp()).into()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct LockFileContents {
    process_group_id: Arc<str>,
}

#[derive(Debug, PartialEq)]
pub enum LockStatus {
    Locked,
    Busy,
}

#[derive(Debug)]
pub struct FileLock {
    pub path: Arc<str>,
    is_locked: bool,
}

impl FileLock {
    pub fn new(path: Arc<str>) -> Self {
        Self {
            path,
            is_locked: false,
        }
    }

    pub fn try_lock(&mut self) -> anyhow::Result<LockStatus> {
        let path_as_path = std::path::Path::new(self.path.as_ref());
        if let Some(parent) = path_as_path.parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create {}",
                parent.to_str().unwrap()
            ))?;
        }

        match std::fs::OpenOptions::new()
            .write(true) // Open for writing
            .create_new(true) // Create only if it does NOT exist
            .open(self.path.as_ref())
        {
            Ok(file) => {
                let contents = LockFileContents {
                    process_group_id: get_process_group_id(),
                };
                serde_json::to_writer(file, &contents)
                    .context(format_context!("Failed to write {}", self.path))?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(std::time::Duration::from_millis(20));
                let contents_result = std::fs::read_to_string(self.path.as_ref())
                    .context(format_context!("Failed to read {}", self.path));

                if let Ok(contents) = contents_result {
                    let existing_info_result = serde_json::from_str::<LockFileContents>(&contents)
                        .context(format_context!(
                            "failed to parse `{contents}` from {} - delete the file and try again",
                            self.path
                        ));

                    if let Ok(existing_info) = existing_info_result {
                        if existing_info.process_group_id == get_process_group_id() {
                            return Ok(LockStatus::Busy);
                        } else {
                            // File exists but belongs to an old process group
                            let contents = LockFileContents {
                                process_group_id: get_process_group_id(),
                            };
                            let lock_contents = serde_json::to_string(&contents)
                                .context(format_context!("Failed to serialize capsule run info"))?;

                            // over write the file
                            std::fs::write(self.path.as_ref(), lock_contents.as_str())
                                .context(format_context!("Failed to create file {}", self.path))?;
                        }
                    } else {
                        return Ok(LockStatus::Busy);
                    }
                } else {
                    return Ok(LockStatus::Busy);
                }
            }
            Err(err) => {
                return Err(format_error!(
                    "Failed to create file '{}': {err:?} - delete the file and try again",
                    self.path
                ));
            }
        }
        self.is_locked = true;
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
        std::fs::remove_file(self.path.as_ref())
            .context(format_context!("Failed to remove {}", self.path))?;
        Ok(())
    }

    pub fn wait(&self, progress: &mut printer::MultiProgressBar) -> anyhow::Result<()> {
        progress.set_message("Capsule already started, waiting for it to finish");
        let lock_file_path = std::path::Path::new(self.path.as_ref());
        let mut log_count = 0;
        while lock_file_path.exists() {
            // another process may have just created this file.
            // This delay gives the other process time to finish creating the file.
            std::thread::sleep(std::time::Duration::from_millis(100));

            // the holding process may have deleted the file by now.
            let contents_result = std::fs::read_to_string(lock_file_path)
                .context(format_context!("Failed to read {}", self.path));

            match contents_result {
                Ok(contents) => {
                    let lock_info: LockFileContents =
                        serde_json::from_str(&contents).context(format_context!(
                            "failed to parse {} - delete the file and try again",
                            self.path
                        ))?;

                    if lock_info.process_group_id != get_process_group_id() {
                        return Ok(());
                    }
                }
                Err(_) => {}
            }

            progress.increment(1);
            std::thread::sleep(std::time::Duration::from_millis(500));
            log_count += 1;
            if log_count == 10 {
                logger::Logger::new_progress(progress, "lock".into()).debug(
                    format!("Still waiting for capsule to finish at {}", self.path).as_str(),
                );
                log_count = 0;
            }
        }
        Ok(())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if self.is_locked {
            let _ = self.unlock();
        }
    }
}
