use crate::{changes, deps, labels};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Target {
    File(Arc<str>),
    Directory(Arc<str>),
}

impl Target {
    pub fn sanitize(&mut self, latest_starlark_module: Option<Arc<str>>) {
        match self {
            Target::File(file_path) => {
                *file_path = labels::sanitize_path(file_path.clone(), latest_starlark_module)
            }
            Target::Directory(dir_path) => {
                *dir_path = labels::sanitize_path(dir_path.clone(), latest_starlark_module);
            }
        }
    }

    pub fn get_target_glob(&self) -> deps::Globs {
        match self {
            Target::File(file_path) => deps::Globs::Includes(vec![file_path.clone()]),
            Target::Directory(dir_path) => {
                let dir_glob = format!("{dir_path}/**").into();
                deps::Globs::Includes(vec![dir_glob])
            }
        }
    }

    pub fn get_target_paths(&self) -> Vec<Arc<std::path::Path>> {
        match self {
            Target::File(file_path) => {
                let sane_path = labels::get_path_from_path_label(file_path.as_ref());
                vec![sane_path]
            }
            Target::Directory(dir_path) => {
                let sane_path = labels::get_path_from_path_label(dir_path.as_ref());
                let mut set = HashSet::new();
                set.insert(format!("{}/**", sane_path.display()).into());
                let globs = changes::glob::Globs::new_with_includes(&set);
                globs.collect_matches()
            }
        }
    }
}
