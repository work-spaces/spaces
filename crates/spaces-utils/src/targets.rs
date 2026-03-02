use crate::deps::Globs;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnyTarget {
    Files(Vec<Arc<str>>),
    Globs(Globs),
}

impl AnyTarget {
    fn get_target_paths(&self) -> Vec<Arc<std::path::Path>> {
        match &self {
            AnyTarget::Files(files) => files
                .iter()
                .map(|file| std::path::Path::new(file.as_ref()).into())
                .collect(),
            AnyTarget::Globs(globs) => globs.get_glob_matches(),
        }
    }
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

    pub fn get_target_paths(&self) -> Vec<Arc<std::path::Path>> {
        self.any.get_target_paths()
    }
}
