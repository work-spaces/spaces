use anyhow::Context;
use anyhow_source_location::format_context;
use bincode::{Decode, Encode};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub mod glob;

#[derive(Clone, Debug, Encode, Decode)]
pub enum ChangeDetailType {
    None,
    File(Arc<str>),
    Directory,
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct ChangeDetail {
    pub modified: std::time::SystemTime,
    pub detail_type: ChangeDetailType,
}

fn changes_logger(progress: &mut printer::MultiProgressBar) -> logger::Logger {
    logger::Logger::new_progress(progress, "Changes".into())
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct Changes {
    path: Arc<str>,
    skip_folders: Vec<Arc<str>>,
    pub entries: HashMap<Arc<str>, ChangeDetail>,
}

impl Changes {
    fn skip_hashing(entry: &walkdir::DirEntry, skip_folders: &[Arc<str>]) -> bool {
        let file_name: Arc<str> = entry.file_name().to_string_lossy().into();
        !skip_folders.contains(&file_name)
    }

    fn process_entry(
        progress: &mut printer::MultiProgressBar,
        path: &std::path::Path,
    ) -> anyhow::Result<ChangeDetail> {
        progress.set_message(format!("Processing {path:?}").as_str());

        let detail_type = if path.is_file() {
            let contents =
                std::fs::read(path).context(format_context!("failed to load {path:?}"))?;
            let hash = blake3::hash(&contents);
            ChangeDetailType::File(hash.to_string().into())
        } else if path.is_dir() {
            ChangeDetailType::Directory
        } else {
            ChangeDetailType::None
        };

        let modified = path
            .metadata()
            .context(format_context!("failed to get metadata for {path:?}"))?
            .modified()
            .context(format_context!("failed to get modified time for {path:?}"))?;

        let change_detail = ChangeDetail {
            detail_type,
            modified,
        };

        Ok(change_detail)
    }

    pub fn new(path: &str, skip_folders: Vec<Arc<str>>) -> Changes {
        match Self::load(path) {
            Ok(changes) => changes,
            Err(_) => Changes {
                path: path.into(),
                entries: HashMap::new(),
                skip_folders,
            },
        }
    }

    fn filter_update(
        entry: &walkdir::DirEntry,
        entries: &HashMap<Arc<str>, ChangeDetail>,
        skip_folders: &[Arc<str>],
        globs: &HashSet<Arc<str>>,
    ) -> bool {
        if !Self::skip_hashing(entry, skip_folders) {
            return false;
        }

        if entry.file_type().is_dir() {
            return true;
        }

        let file_path: Arc<str> = entry.path().to_string_lossy().into();

        if !glob::match_globs(globs, file_path.as_ref()) {
            return false;
        }

        if let Some(change_detail) = entries.get(file_path.as_ref()) {
            let modified = match entry.metadata() {
                Ok(metadata) => metadata
                    .modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                Err(_) => return true,
            };

            if modified == change_detail.modified {
                return false;
            }
        }

        true
    }

    fn update_entry(
        &mut self,
        progress: &mut printer::MultiProgressBar,
        path: Arc<str>,
        change_detail: ChangeDetail,
    ) -> bool {
        let sane_path = Self::sanitize_path(&path);
        let mut logger = logger::Logger::new_progress(progress, "Changes".into());
        if let Some(previous_entry) = self
            .entries
            .insert(sane_path.into(), change_detail.clone())
        {
            if let (ChangeDetailType::File(previous_hash), ChangeDetailType::File(new_hash)) =
                (&previous_entry.detail_type, &change_detail.detail_type)
            {
                if previous_hash != new_hash {
                    logger.debug(
                        format!("{path} hash changed").as_str(),
                    );

                    return true;
                }
            }
        } else {
            logger.debug(format!("{path} added hash").as_str());
            return true;
        }

        false
    }

    pub fn update_from_inputs(
        &mut self,
        progress: &mut printer::MultiProgressBar,
        inputs: &HashSet<Arc<str>>,
    ) -> anyhow::Result<()> {
        for input in inputs {
            changes_logger(progress).trace(
                format!("Update changes for {input}").as_str(),
            );

            let mut count = 0usize;
            // convert input from a glob expression to a parent directory
            if input.find('*').is_none() {
                let path = std::path::Path::new(input.as_ref());
                if path.exists() && path.is_file() {
                    let change_detail = Self::process_entry(progress, path)
                        .context(format_context!("Failed to process entry"))?;

                    self.update_entry(progress, path.to_string_lossy().into(), change_detail);

                    progress.increment(1);

                    continue;
                }
            }

            let input_path = if let Some(asterisk_postion) = input.find('*') {
                let mut path = input.to_string();
                path.truncate(asterisk_postion);
                path.into()
            } else {
                // check if input is a file or directory
                input.clone()
            };

            if let Some(glob_include_path) = glob::is_glob_include(input_path.as_ref()) {
                changes_logger(progress).trace(
                    format!("Update glob {glob_include_path}").as_str(),
                );

                let walk_dir: Vec<_> = walkdir::WalkDir::new(glob_include_path.as_ref())
                    .into_iter()
                    .filter_entry(|e| {
                        Self::filter_update(e, &self.entries, &self.skip_folders, inputs)
                    })
                    .filter_map(|entry| entry.ok())
                    .collect();

                for entry in walk_dir.into_iter() {
                    let path = entry.path();
                    let change_detail = Self::process_entry(progress, path)
                        .context(format_context!("Failed to process entry"))?;

                    if self.update_entry(
                        progress,
                        path.to_string_lossy().into(),
                        change_detail,
                    ) {
                        count += 1;
                    }

                    progress.increment(1);
                }

                if count > 0 {
                    changes_logger(progress).message(
                        format!("Updated {count} items from {input}").as_str(),
                    );
                }
            }
        }

        Ok(())
    }

    pub fn sanitize_path(input: &str) -> &str {
        input.strip_prefix("./").unwrap_or(input)
    }

    pub fn get_digest(
        &self,
        progress: &mut printer::MultiProgressBar,
        seed: &str,
        globs: &HashSet<Arc<str>>,
    ) -> anyhow::Result<Arc<str>> {
        let mut inputs = Vec::new();
        for path in self.entries.keys() {
            let sane_path = Self::sanitize_path(path);
            if glob::match_globs(globs, sane_path) {
                inputs.push(path);
            }
        }

        inputs.sort();

        let mut count = 0usize;
        let mut hasher = blake3::Hasher::new();
        hasher.update(seed.as_bytes());
        for input in inputs.iter() {
            if let Some(change_detail) = self.entries.get(*input) {
                if let ChangeDetailType::File(hash) = &change_detail.detail_type {
                    changes_logger(progress).trace( 
                        format!("Hashing {input}:{hash}").as_str(),
                    );
                    count += 1;
                    hasher.update(hash.as_bytes());
                }
            }
        }

        if count > 0 {
            changes_logger(progress).message( 
                format!("Hashed {count} items").as_str(),
            );
        }

        Ok(hasher.finalize().to_string().into())
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let encoded = bincode::encode_to_vec(self, bincode::config::standard())
            .context(format_context!("Failed to serialize"))?;
        std::fs::write(path, encoded).context(format_context!("Failed to write to {path:?}"))?;
        Ok(())
    }

    fn load(path: &str) -> anyhow::Result<Changes> {
        let file = std::fs::File::open(path).context(format_context!("Failed to open {path:?}"))?;
        let reader = std::io::BufReader::new(file);
        let changes: Changes = bincode::decode_from_reader(reader, bincode::config::standard())
            .context(format_context!("Failed to deserialize {path:?}"))?;
        Ok(changes)
    }
}

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
