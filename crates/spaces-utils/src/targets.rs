use crate::deps::Globs;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnyTarget {
    Files(Vec<Arc<str>>),
    Globs(Vec<Globs>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub name: Arc<str>,
    pub any: AnyTarget,
}

impl Target {
    pub fn new_with_files(name: Arc<str>, files: Vec<Arc<str>>) -> Self {
        Self {
            name,
            any: AnyTarget::Files(files),
        }
    }

    pub fn new_with_globs(name: Arc<str>, globs: Vec<Globs>) -> Self {
        Self {
            name,
            any: AnyTarget::Globs(globs),
        }
    }
}
