use crate::{http_archive, logger};
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

const MANIFEST_FILE_NAME: &str = "store.spaces.json";

pub fn logger(printer: &mut printer::Printer) -> logger::Logger {
    logger::Logger::new_printer(printer, "store".into())
}

#[derive(Debug, Clone, PartialEq, clap::ValueEnum)]
pub enum SortBy {
    /// Sort by name
    Name,
    /// Sort by size
    Size,
    /// Sort by age (time since last used)
    Age,
}

#[derive(Debug, clap::Subcommand, Clone)]
pub enum StoreCommand {
    /// Show information about the data in the store
    Info {
        /// Sorty by name/age/size
        #[clap(long, default_value = "name")]
        sort_by: SortBy,
    },
    /// Check the store for errors and delete any entries that have an error.
    Fix {
        /// Show which entries have errors and will be deleted without deleting the data
        #[clap(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    last_used: u128,
    size: u64,
}

impl Entry {
    fn get_age(&self, now: u128) -> u128 {
        (now - self.last_used) / (24 * 60 * 60 * 1000)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Store {
    entries: HashMap<Arc<str>, Entry>,
    #[serde(skip)]
    path_to_store: std::path::PathBuf,
}

impl Store {
    pub fn new_from_store_path(path_to_store: &std::path::Path) -> anyhow::Result<Self> {
        let path = std::path::Path::new(path_to_store).join(MANIFEST_FILE_NAME);
        if path.exists() {
            let contents = std::fs::read_to_string(path.clone())
                .context(format_context!("Failed to read file: {}", path.display()))?;
            let mut store: Store = serde_json::from_str(&contents).context(format_context!(
                "Failed to deserialize JSON: {}",
                path.display()
            ))?;
            store.path_to_store = path_to_store.into();
            Ok(store)
        } else {
            Ok(Store {
                entries: HashMap::new(),
                path_to_store: path_to_store.into(),
            })
        }
    }

    pub fn merge(&mut self, other: Store) {
        for (key, value) in other.entries {
            self.entries.insert(key, value);
        }
    }

    pub fn save(&self, path_to_store: &std::path::Path) -> anyhow::Result<()> {
        let path = path_to_store.join(MANIFEST_FILE_NAME);
        let contents = serde_json::to_string_pretty(self).context(format_context!(
            "Failed to serialize JSON: {}",
            path.display()
        ))?;
        std::fs::write(path.clone(), contents)
            .context(format_context!("Failed to write file: {}", path.display()))?;
        Ok(())
    }

    fn get_path_in_store(&self, path: &std::path::Path) -> std::path::PathBuf {
        self.path_to_store.join(path)
    }

    pub fn add_entry(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);

        let full_path = self.get_path_in_store(path);
        let size = get_size_of_path(full_path.as_path()).context(format_context!(
            "Failed to get size of path: {}",
            path.display()
        ))?;

        let path_entry = path.display().to_string();
        let map_entry = self
            .entries
            .entry(path_entry.into())
            .or_insert_with(|| Entry {
                last_used: timestamp,
                size,
            });

        map_entry.last_used = timestamp;
        map_entry.size = size;

        Ok(())
    }

    pub fn show_info(&self, printer: &mut printer::Printer, sort_by: SortBy) {
        let mut is_fix_needed = false;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);

        let mut entries: Vec<_> = self.entries.iter().collect();

        match sort_by {
            SortBy::Name => entries.sort_by(|a, b| a.0.cmp(b.0)),
            // largest to smallest
            SortBy::Size => entries.sort_by(|a, b| b.1.size.cmp(&a.1.size)),
            // oldest to newest
            SortBy::Age => entries.sort_by(|a, b| b.1.get_age(now).cmp(&a.1.get_age(now))),
        }

        let mut total_size = 0;
        for (key, value) in entries.iter() {
            logger(printer).info(format!("Path: {key}").as_str());
            let path = self.get_path_in_store(std::path::Path::new(key.as_ref()));
            if !path.exists() {
                is_fix_needed = true;
                logger(printer)
                    .info(format!(" !!Path does not exist!! -- {}", path.display()).as_str());
            }
            if value.size == 0 {
                is_fix_needed = true;
                logger(printer).info(" !!Size is zero!!");
            } else {
                let bytesize = bytesize::ByteSize(value.size);
                logger(printer).info(format!("  Size: {}", bytesize.display()).as_str());
            }
            total_size += value.size;

            let age = value.get_age(now);
            logger(printer).info(format!("  Age: {age} days").as_str());
        }
        if is_fix_needed {
            logger(printer).info("run `spaces store fix` to fix the issues");
        }
        let total_bytesize = bytesize::ByteSize(total_size);
        logger(printer).info(format!("Total Size: {}", total_bytesize.display()).as_str());
    }

    pub fn fix(&mut self, printer: &mut printer::Printer, is_dry_run: bool) {
        let mut remove_entries = Vec::new();
        let mut delete_directories = Vec::new();
        let path_to_store = self.path_to_store.clone();
        for (key, value) in self.entries.iter_mut() {
            logger(printer).info(format!("Path: {key}").as_str());
            let path = path_to_store.join(key.as_ref());
            if !path.exists() {
                remove_entries.push(key.clone());
            }

            let updated_size = get_size_of_path(path.as_path()).unwrap_or(0);
            let bytesize = bytesize::ByteSize(value.size);
            if updated_size != value.size {
                if !is_dry_run {
                    logger(printer).info(format!(" Updated size {}", bytesize.display()).as_str());
                    value.size = updated_size;
                } else {
                    logger(printer)
                        .info(format!(" Size needs updating {}", bytesize.display()).as_str());
                }
            }

            if !key.ends_with(".git") {
                let result = http_archive::check_downloaded_archive(&path);
                if let Err(err) = result {
                    logger(printer).warning(format!("{key} is corrupted. {err}").as_str());
                    remove_entries.push(key.clone());
                    delete_directories.push(path);
                }
            }
        }

        if !is_dry_run {
            for key in remove_entries {
                logger(printer).info(format!("Removing entry: {key}").as_str());
                self.entries.remove(&key);
            }

            for path in delete_directories {
                if path.starts_with(path_to_store.as_path()) {
                    logger(printer)
                        .info(format!("Deleting directory: {}", path.display()).as_str());
                    std::fs::remove_dir_all(path.as_path()).unwrap_or_else(|err| {
                        logger(printer).warning(
                            format!("Failed to delete directory {}: {err}", path.display())
                                .as_str(),
                        );
                    });
                } else {
                    logger(printer).error(
                        format!("Cannot delete out of store directory: {}", path.display())
                            .as_str(),
                    );
                }
            }
        }
    }
}

fn get_size_of_path(path: &std::path::Path) -> anyhow::Result<u64> {
    let iter = walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len());

    Ok(iter.sum())
}
