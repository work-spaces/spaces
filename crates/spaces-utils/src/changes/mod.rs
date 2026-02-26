use crate::{logger, rule};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub mod glob;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum ChangeDetailType {
    #[default]
    None,
    File(Arc<str>),
    Directory,
    Symlink(Arc<str>),
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChangeDetail {
    pub modified: Option<std::time::SystemTime>,
    pub detail_type: ChangeDetailType,
}

fn changes_logger(progress: &mut printer::MultiProgressBar) -> logger::Logger<'_> {
    logger::Logger::new_progress(progress, "changes".into())
}

pub fn get_modified_time<ErrorType>(
    metadata_result: Result<std::fs::Metadata, ErrorType>,
) -> Option<std::time::SystemTime> {
    metadata_result
        .ok()
        .and_then(|metadata| metadata.modified().ok())
}

pub fn is_modified(
    metadata_modified: Option<std::time::SystemTime>,
    last_modified: Option<std::time::SystemTime>,
) -> bool {
    metadata_modified
        .map(|metadata_modified| {
            metadata_modified != last_modified.unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        })
        .unwrap_or(true)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CheckIsModified {
    No,
    Yes,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Changes {
    pub path: Arc<str>,
    pub skip_folders: Vec<Arc<str>>,
    // map of sanitized paths to change details
    pub entries: HashMap<Arc<str>, ChangeDetail>,
}

impl Changes {
    fn update_entry(
        &mut self,
        progress: &mut printer::MultiProgressBar,
        path: Arc<str>,
        change_detail: ChangeDetail,
    ) -> bool {
        let sane_path = Self::sanitize_path(&path);
        let mut logger = logger::Logger::new_progress(progress, "Changes".into());
        if let Some(previous_entry) = self.entries.insert(sane_path.into(), change_detail.clone())
            && let (ChangeDetailType::File(previous_hash), ChangeDetailType::File(new_hash))
            | (ChangeDetailType::Symlink(previous_hash), ChangeDetailType::Symlink(new_hash)) =
                (&previous_entry.detail_type, &change_detail.detail_type)
        {
            if previous_hash != new_hash {
                logger.debug(format!("{path} hash changed").as_str());

                return true;
            }
        } else {
            logger.debug(format!("{path} added hash").as_str());
            return true;
        }

        false
    }

    fn walk_glob_dir(
        &self,
        progress: &mut printer::MultiProgressBar,
        glob_include_path: Arc<str>,
        check_is_modified: CheckIsModified,
        inputs: &glob::Globs,
    ) -> Vec<walkdir::DirEntry> {
        walkdir::WalkDir::new(glob_include_path.as_ref())
            .into_iter()
            .filter_entry(|e| {
                filter_update(
                    progress,
                    e,
                    if check_is_modified == CheckIsModified::Yes {
                        Some(&self.entries)
                    } else {
                        None
                    },
                    &self.skip_folders,
                    inputs,
                )
            })
            .filter_map(|entry| entry.ok())
            .collect()
    }

    pub fn inspect_inputs(
        &self,
        progress: &mut printer::MultiProgressBar,
        inputs: &glob::Globs,
    ) -> anyhow::Result<Vec<String>> {
        let mut set = HashSet::new();
        for input in inputs.includes.iter() {
            changes_logger(progress).message(format!("Inspecting input {input}").as_str());
            if let Some(path) = input_includes_no_asterisk(input.as_ref()) {
                set.insert(path.display().to_string());
            } else {
                let input_path = get_glob_path(input.clone());
                changes_logger(progress)
                    .trace(format!("inspect include input path `{input_path}`").as_str());
                let walk_dir =
                    self.walk_glob_dir(progress, input_path, CheckIsModified::No, inputs);
                for entry in walk_dir.into_iter() {
                    if !entry.file_type().is_dir() {
                        set.insert(entry.path().display().to_string());
                    }
                }
            }
        }

        let mut result: Vec<_> = set.into_iter().collect();
        result.sort();

        Ok(result)
    }

    /// Processes all the files that are specified in the input globs
    pub fn update_from_inputs(
        &mut self,
        progress: &mut printer::MultiProgressBar,
        inputs: &rule::InputsOutputs,
    ) -> anyhow::Result<()> {
        let inputs = inputs.get_globs();
        for input in inputs.includes.iter() {
            changes_logger(progress).trace(format!("Update changes for {input}").as_str());

            let mut count = 0usize;
            let input_path = get_glob_path(input.clone());
            changes_logger(progress).trace(format!("include input path `{input_path}`").as_str());

            let walk_dir = self.walk_glob_dir(progress, input_path, CheckIsModified::Yes, &inputs);

            changes_logger(progress).trace(format!("walked {} entries", walk_dir.len()).as_str());

            for entry in walk_dir.into_iter() {
                if entry.file_type().is_dir() {
                    continue;
                }

                let path = entry.path();
                let path_string: Arc<str> = path.to_string_lossy().into();
                changes_logger(progress).trace(format!("process {}", path.display()).as_str());

                let change_detail = process_entry(progress, path)
                    .context(format_context!("Failed to process entry"))?;

                if self.update_entry(progress, path_string.clone(), change_detail) {
                    count += 1;
                }

                progress.increment(1);
            }

            if count > 0 {
                changes_logger(progress)
                    .debug(format!("Updated {count} items from {input}").as_str());
            }

            changes_logger(progress).trace(format!("Done updating {input}").as_str());
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
        globs: &glob::Globs,
    ) -> anyhow::Result<Arc<str>> {
        let mut inputs = Vec::new();

        for path in self.entries.keys() {
            let sane_path = Self::sanitize_path(path);
            if globs.is_match(sane_path) {
                inputs.push(path);
            }
        }

        // sort the inputs to ensure consistent hashing
        inputs.sort();

        let mut count = 0usize;
        let mut hasher = blake3::Hasher::new();
        hasher.update(seed.as_bytes());
        for input in inputs.iter() {
            if let Some(change_detail) = self.entries.get(*input)
                && let ChangeDetailType::File(hash) = &change_detail.detail_type
            {
                changes_logger(progress).trace(format!("Hashing {input}:{hash}").as_str());
                count += 1;
                hasher.update(hash.as_bytes());
            }
        }

        if count > 0 {
            changes_logger(progress).debug(format!("Hashed {count} items").as_str());
        }

        Ok(hasher.finalize().to_string().into())
    }
}

fn input_includes_no_asterisk(input: &str) -> Option<std::path::PathBuf> {
    if input.find('*').is_none()
        && let Some(input) = input.strip_prefix('+')
    {
        let path = std::path::Path::new(input);
        if path.exists() && path.is_file() {
            return Some(path.to_path_buf());
        }
    }
    None
}

// callback used when walking a directory to filter out directory entries
// that do not match the globs specified in globs
fn filter_update(
    progress: &mut printer::MultiProgressBar,
    entry: &walkdir::DirEntry,
    entries: Option<&HashMap<Arc<str>, ChangeDetail>>,
    skip_folders: &[Arc<str>],
    globs: &glob::Globs,
) -> bool {
    if !skip_hashing(entry, skip_folders) {
        return false;
    }

    if entry.file_type().is_dir() {
        let file_name = entry.file_name().to_string_lossy();
        if file_name == ".git" || file_name == ".spaces" {
            return false;
        }
        return true;
    }

    let file_path: Arc<str> = entry.path().to_string_lossy().into();

    if !globs.is_match(file_path.as_ref()) {
        changes_logger(progress).trace(format!("filtered `{file_path}`").as_str());
        return false;
    }

    if let Some(entries) = entries
        && let Some(change_detail) = entries.get(file_path.as_ref())
    {
        let modified_time = get_modified_time(entry.metadata());
        return is_modified(modified_time, change_detail.modified);
    }

    true
}

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
            std::fs::read(path).with_context(|| format_context!("failed to load {path:?}"))?;
        let hash = blake3::hash(&contents);
        ChangeDetailType::File(hash.to_string().into())
    } else if path.is_dir() {
        ChangeDetailType::Directory
    } else if path.is_symlink() {
        // This will detect a change if the value to the symlink changes
        // it won't hash the target which could be a file or a directory
        ChangeDetailType::Symlink(
            path.read_link()
                .context(format_context!("failed to read symlink {path:?}"))?
                .display()
                .to_string()
                .into(),
        )
    } else {
        ChangeDetailType::None
    };

    match path.metadata() {
        Ok(metadata) => {
            let modified = metadata
                .modified()
                .with_context(|| format_context!("failed to get modified time for {path:?}"))?;
            let change_detail = ChangeDetail {
                detail_type,
                modified: Some(modified),
            };
            Ok(change_detail)
        }
        Err(err) => {
            if path.is_symlink() {
                changes_logger(progress).warning(
                    format!("metadata for symlink destination not found for {path:?}").as_str(),
                );
                Ok(ChangeDetail {
                    detail_type,
                    modified: None,
                })
            } else {
                Err(format_error!(
                    "Failed to get metadata for {path:?}: {}",
                    err
                ))
            }
        }
    }
}

// This is used to limit globbing to a subset of the workspace
fn get_glob_path(input: Arc<str>) -> Arc<str> {
    if let Some(asterisk_position) = input.find('*') {
        let mut path = input.to_string();
        path.truncate(asterisk_position);
        if path.is_empty() {
            ".".into()
        } else {
            path.into()
        }
    } else {
        // no asterisk found, return input as-is
        input.clone()
    }
}
