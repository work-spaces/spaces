use crate::{age, ci, http_archive, logger};
use anyhow::Context;
use anyhow_source_location::format_context;
use bytesize::ByteSize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

const MANIFEST_FILE_NAME: &str = "store.spaces.json";
pub const SPACES_STORE: &str = ".spaces/store";
pub const SPACES_STORE_RCACHE: &str = "rcache";

pub fn logger(printer: &mut printer::Printer) -> logger::Logger<'_> {
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
    /// Prune the store by deleting entries that are older the specified age.
    Prune {
        /// Delete entries older than this age in days
        #[clap(long, default_value = "30")]
        age: u16,
        /// Show which entries will be deleted without deleting the data
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
    fn get_age(&self, reference: u128) -> u128 {
        age::LastUsed::new(self.last_used).get_age(reference)
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

    pub fn show_info(
        &self,
        printer: &mut printer::Printer,
        sort_by: SortBy,
        is_ci: ci::IsCi,
    ) -> anyhow::Result<()> {
        let group = ci::GithubLogGroup::new_group(printer, is_ci, "Spaces Store Info")?;

        let mut is_fix_needed = false;

        let mut entries: Vec<_> = self.entries.iter().collect();

        let now = age::get_now();
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

            let age = value.get_age(age::get_now());
            logger(printer).info(format!("  Age: {age} days").as_str());
        }
        if is_fix_needed {
            logger(printer).info("run `spaces store fix` to fix the issues");
        }
        let total_bytesize = bytesize::ByteSize(total_size);
        logger(printer).info(format!("Total Size: {}", total_bytesize.display()).as_str());

        group.end_group(printer, is_ci)?;
        Ok(())
    }

    fn remove_unlisted_entries(
        &self,
        printer: &mut printer::Printer,
        is_dry_run: bool,
    ) -> anyhow::Result<()> {
        let path_to_store = self.path_to_store.clone();

        let suffixes: Vec<_> = http_archive::get_archive_suffixes()
            .iter()
            .map(std::ffi::OsStr::new)
            .collect();

        let http_path = path_to_store.join("http");
        let https_path = path_to_store.join("https");

        let http_entries = walkdir::WalkDir::new(&http_path)
            .into_iter()
            .filter_map(|e| e.ok());

        let https_entries = walkdir::WalkDir::new(&https_path)
            .into_iter()
            .filter_map(|e| e.ok());

        let entries = http_entries.chain(https_entries).filter(|e| {
            let path = e.path();
            if path.is_dir() {
                let extension = path.extension().unwrap_or_default();
                suffixes.contains(&extension)
            } else {
                false
            }
        });

        for entry in entries {
            let entry_path = entry.path();
            if let Ok(relative_path) = entry_path.strip_prefix(&path_to_store)
                && !self
                    .entries
                    .keys()
                    .map(|e| std::path::Path::new(e.as_ref()))
                    .any(|e| e == relative_path)
            {
                let display = relative_path.display();
                if is_dry_run {
                    logger(printer).info(
                        format!("Unlisted Entry (not removing, dry run): {display}",).as_str(),
                    );
                } else {
                    logger(printer).info(format!("Unlisted Entry (removing): {display}").as_str());

                    if entry_path.starts_with(&path_to_store) {
                        match std::fs::remove_dir_all(entry_path) {
                            Ok(()) => {}
                            Err(e) => {
                                logger(printer).error(
                                    format!("Failed to remove {display}: {e} - remove manually")
                                        .as_str(),
                                );
                            }
                        }
                    } else {
                        logger(printer).error(
                            format!("Internal Error: can't remove {display} - not in store")
                                .as_str(),
                        );
                    }
                }
            }
        }

        Ok(())
    }

    pub fn fix(
        &mut self,
        printer: &mut printer::Printer,
        is_dry_run: bool,
        is_ci: ci::IsCi,
    ) -> anyhow::Result<()> {
        let group = ci::GithubLogGroup::new_group(printer, is_ci, "Spaces Store Fix")?;

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
            if updated_size != value.size {
                if !is_dry_run {
                    let bytesize = bytesize::ByteSize(updated_size);
                    logger(printer).info(format!(" Updated size {}", bytesize.display()).as_str());
                    value.size = updated_size;
                } else {
                    let bytesize = bytesize::ByteSize(updated_size);
                    logger(printer).info(format!(" Updating size {}", bytesize.display()).as_str());
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

        self.remove_unlisted_entries(printer, is_dry_run)
            .context(format_context!("While checking for unlisted entries"))?;

        group.end_group(printer, is_ci)?;
        Ok(())
    }

    pub fn prune(
        &mut self,
        printer: &mut printer::Printer,
        age: u16,
        is_dry_run: bool,
        is_ci: ci::IsCi,
    ) -> anyhow::Result<()> {
        let group = ci::GithubLogGroup::new_group(printer, is_ci, "Spaces Store Prune")?;
        let mut remove_entries = Vec::new();
        let path_to_store = self.path_to_store.clone();

        let mut total_size_removed = ByteSize(0);
        for (key, entry) in self.entries.iter() {
            let path = path_to_store.join(key.as_ref());
            let entry_age = entry.get_age(age::get_now());
            if entry_age > age as u128 {
                let bytesize = bytesize::ByteSize(entry.size);
                total_size_removed += bytesize.as_u64();
                remove_entries.push((key.clone(), entry_age, bytesize, path.clone()));
            }
        }

        for (key, age, size, path) in remove_entries {
            logger(printer).info(format!("Pruning {key}: {size}").as_str());
            logger(printer).info(format!("- Age: {age} days").as_str());
            logger(printer).info(format!("- Size: {size}").as_str());
            if !is_dry_run {
                self.entries.remove(&key);
                if let Err(e) = std::fs::remove_dir_all(&path) {
                    logger(printer)
                        .error(format!("Failed to remove entry: {key}, error: {e}").as_str());
                } else {
                    logger(printer).info("- Removed.");
                }
            } else {
                logger(printer).info("- Dry run. Not removed.");
            }
        }

        logger(printer).info(format!("Total removed: {total_size_removed}").as_str());

        group.end_group(printer, is_ci)?;

        Ok(())
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
