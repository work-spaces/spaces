use crate::{changes, deps, labels};
use anyhow::Context;
use anyhow_source_location::format_context;
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

    pub fn remove(&self) -> anyhow::Result<()> {
        match self {
            Target::File(file_path) => {
                let path = std::path::Path::new(file_path.as_ref());
                if path.exists() {
                    std::fs::remove_file(path)
                        .context(format_context!("Failed to remove target {file_path}"))?;
                }
            }
            Target::Directory(dir_path) => {
                let path = std::path::Path::new(dir_path.as_ref());
                if path.is_dir() {
                    for entry in std::fs::read_dir(path)
                        .context(format_context!("Failed to read directory {dir_path}"))?
                    {
                        let entry =
                            entry.context(format_context!("Failed to read entry in {dir_path}"))?;
                        let entry_path = entry.path();
                        if entry_path.is_dir() {
                            std::fs::remove_dir_all(&entry_path).context(format_context!(
                                "Failed to remove directory {}",
                                entry_path.display()
                            ))?;
                        } else {
                            std::fs::remove_file(&entry_path).context(format_context!(
                                "Failed to remove file {}",
                                entry_path.display()
                            ))?;
                        }
                    }
                }
            }
        }
        Ok(())
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
