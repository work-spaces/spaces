
use std::{sync::{RwLock, RwLockReadGuard, RwLockWriteGuard}};


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
        self.lock.read().expect(format!("Internal error: failed to get read lock for {:?}", self.lock).as_str())
    }

    pub fn write(&self) -> RwLockWriteGuard<ModuleState> {
        self.lock.write().expect(format!("Internal error: failed to get read lock for {:?}", self.lock).as_str())
    }
}