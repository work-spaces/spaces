use anyhow::Context;
use anyhow_source_location::format_context;
use bincode::{Decode, Encode};
use std::collections::{HashMap, HashSet};

mod glob;

#[derive(Clone, Debug, Encode, Decode)]
pub enum ChangeDetailType {
    None,
    File(String),
    Directory,
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct ChangeDetail {
    pub modified: std::time::SystemTime,
    pub detail_type: ChangeDetailType,
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct Changes {
    path: String,
    skip_folders: Vec<String>,
    pub entries: HashMap<String, ChangeDetail>,
}

impl Changes {
    fn skip_hashing(entry: &walkdir::DirEntry, skip_folders: &[String]) -> bool {
        let file_name = entry.file_name().to_string_lossy().to_string();
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
            ChangeDetailType::File(hash.to_string())
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

    pub fn new(path: &str, skip_folders: Vec<String>) -> Changes {
        match Self::load(path) {
            Ok(changes) => changes,
            Err(_) => Changes {
                path: path.to_string(),
                entries: HashMap::new(),
                skip_folders,
            },
        }
    }

    fn filter_update(
        entry: &walkdir::DirEntry,
        entries: &HashMap<String, ChangeDetail>,
        skip_folders: &[String],
        globs: &HashSet<String>,
    ) -> bool {
        if !Self::skip_hashing(entry, skip_folders) {
            return false;
        }

        if entry.file_type().is_dir() {
            return true;
        }

        let file_path = entry.path().to_string_lossy().to_string();

        if !glob::match_globs(globs, file_path.as_str()) {
            return false;
        }

        if let Some(change_detail) = entries.get(&file_path) {
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
        path: String,
        change_detail: ChangeDetail,
    ) -> bool {
        let sane_path = Self::sanitize_path(&path);
        if let Some(previous_entry) = self
            .entries
            .insert(sane_path.to_owned(), change_detail.clone())
        {
            if let (ChangeDetailType::File(previous_hash), ChangeDetailType::File(new_hash)) =
                (&previous_entry.detail_type, &change_detail.detail_type)
            {
                if previous_hash != new_hash {
                    progress.log(
                        printer::Level::Debug,
                        format!("{path} hash changed").as_str(),
                    );

                    return true;
                }
            }
        } else {
            progress.log(printer::Level::Debug, format!("{path} added hash").as_str());
            return true;
        }

        false
    }

    pub fn update_from_inputs(
        &mut self,
        progress: &mut printer::MultiProgressBar,
        inputs: &HashSet<String>,
    ) -> anyhow::Result<()> {
        for input in inputs {
            progress.log(
                printer::Level::Trace,
                format!("Update changes for {input}").as_str(),
            );

            let mut count = 0usize;
            // convert input from a glob expression to a parent directory
            if input.find('*').is_none() {
                let path = std::path::Path::new(input);
                if path.exists() && path.is_file() {
                    let change_detail = Self::process_entry(progress, path)
                        .context(format_context!("Failed to process entry"))?;

                    self.update_entry(progress, path.to_string_lossy().to_string(), change_detail);

                    progress.increment(1);

                    continue;
                }
            }

            let input_path = if let Some(asterisk_postion) = input.find('*') {
                let mut path = input.clone();
                path.truncate(asterisk_postion);
                path
            } else {
                // check if input is a file or directory
                input.clone()
            };

            if let Some(glob_include_path) = glob::is_glob_include(input_path.as_str()) {
                progress.log(
                    printer::Level::Trace,
                    format!("Update glob {glob_include_path}").as_str(),
                );

                let walk_dir: Vec<_> = walkdir::WalkDir::new(glob_include_path.as_str())
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
                        path.to_string_lossy().to_string(),
                        change_detail,
                    ) {
                        count += 1;
                    }

                    progress.increment(1);
                }

                if count > 0 {
                    progress.log(
                        printer::Level::Message,
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
        globs: &HashSet<String>,
    ) -> anyhow::Result<String> {
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
                    progress.log(
                        printer::Level::Trace,
                        format!("Hashing {input}:{hash}").as_str(),
                    );
                    count += 1;
                    hasher.update(hash.as_bytes());
                }
            }
        }

        if count > 0 {
            progress.log(
                printer::Level::Message,
                format!("Hashed {count} items").as_str(),
            );
        }

        Ok(hasher.finalize().to_string())
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
