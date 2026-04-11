use crate::{age, ci, http_archive, logger};
use anyhow::Context;
use anyhow_source_location::format_context;
use bytesize::ByteSize;
use serde::{Deserialize, Serialize};
use serde_with::{TimestampSeconds, serde_as};
use std::collections::HashMap;
use std::sync::Arc;

const MANIFEST_FILE_NAME: &str = "store.spaces.json";
pub const SPACES_STORE: &str = ".spaces/store";
pub const SPACES_STORE_RCACHE: &str = "rcache";
pub const SPACES_STORE_HOME: &str = "home";

pub fn logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "store".into())
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

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmanagedDirectory {
    #[serde_as(as = "TimestampSeconds<i64>")]
    modified_system_time: std::time::SystemTime,
    size: u64,
}

impl Default for UnmanagedDirectory {
    fn default() -> Self {
        Self {
            modified_system_time: std::time::SystemTime::UNIX_EPOCH,
            size: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Store {
    entries: HashMap<Arc<str>, Entry>,
    #[serde(default)]
    unmanaged: HashMap<Arc<str>, UnmanagedDirectory>,
    #[serde(skip)]
    path_to_store: std::path::PathBuf,
}

impl Store {
    pub fn new(path_to_store: &std::path::Path) -> Self {
        Self {
            entries: HashMap::new(),
            unmanaged: HashMap::new(),
            path_to_store: path_to_store.into(),
        }
    }

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
                unmanaged: HashMap::new(),
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

    fn get_managed_top_level_dirs(&self) -> std::collections::HashSet<String> {
        self.entries
            .keys()
            .filter_map(|key| {
                std::path::Path::new(key.as_ref())
                    .components()
                    .next()
                    .and_then(|c| c.as_os_str().to_str().map(String::from))
            })
            .collect()
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
        &mut self,
        console: console::Console,
        sort_by: SortBy,
        is_ci: ci::IsCi,
    ) -> anyhow::Result<()> {
        // Collect unmanaged directory sizes before printing anything, with progress indicator.
        let managed_top_level_dirs = self.get_managed_top_level_dirs();

        let mut unmanaged: Vec<(String, u64, std::time::SystemTime)> = Vec::new();
        {
            let candidates =
                get_unmanaged_dir_entries(&self.path_to_store, &managed_top_level_dirs);

            if !candidates.is_empty() {
                let mut progress = console::Progress::new(
                    console.clone(),
                    "Scanning unmanaged store directories",
                    None,
                    None,
                );
                for entry in candidates {
                    let name = entry.file_name().to_string_lossy().to_string();
                    progress.set_message(name.as_str());
                    let dir_modified = get_dir_modified_system_time(&entry.path());
                    let size = match self.unmanaged.get(name.as_str()) {
                        Some(cached)
                            if system_time_as_secs(cached.modified_system_time)
                                == system_time_as_secs(dir_modified) =>
                        {
                            cached.size
                        }
                        _ => {
                            let size = get_size_of_path(&entry.path()).unwrap_or(0);
                            self.unmanaged.insert(
                                name.clone().into(),
                                UnmanagedDirectory {
                                    modified_system_time: dir_modified,
                                    size,
                                },
                            );
                            size
                        }
                    };
                    unmanaged.push((name, size, dir_modified));
                }
                progress.set_finalize_none();
            }
        }

        let group = ci::GithubLogGroup::new_group(console.clone(), is_ci, "Spaces Store Info")?;

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
            logger(console.clone()).info(format!("Path: {key}").as_str());
            let path = self.get_path_in_store(std::path::Path::new(key.as_ref()));
            if !path.exists() {
                is_fix_needed = true;
                logger(console.clone())
                    .info(format!(" !!Path does not exist!! -- {}", path.display()).as_str());
            }
            if value.size == 0 {
                is_fix_needed = true;
                logger(console.clone()).info(" !!Size is zero!!");
            } else {
                let bytesize = bytesize::ByteSize(value.size);
                logger(console.clone()).info(format!("  Size: {}", bytesize.display()).as_str());
            }
            total_size += value.size;

            let age = value.get_age(age::get_now());
            logger(console.clone()).info(format!("  Age: {age} days").as_str());
        }
        if is_fix_needed {
            logger(console.clone()).info("run `spaces store fix` to fix the issues");
        }

        match sort_by {
            SortBy::Name => unmanaged.sort_by(|a, b| a.0.cmp(&b.0)),
            SortBy::Size => unmanaged.sort_by(|a, b| b.1.cmp(&a.1)),
            // oldest modified first
            SortBy::Age => unmanaged.sort_by(|a, b| a.2.cmp(&b.2)),
        }

        for (name, size, _) in &unmanaged {
            logger(console.clone()).info(format!("Path: {name} (unmanaged)").as_str());
            let bytesize = bytesize::ByteSize(*size);
            logger(console.clone()).info(format!("  Size: {}", bytesize.display()).as_str());
            total_size += size;
        }

        let total_bytesize = bytesize::ByteSize(total_size);
        logger(console.clone()).info(format!("Total Size: {}", total_bytesize.display()).as_str());

        group.end_group(console.clone(), is_ci)?;
        Ok(())
    }

    fn remove_unlisted_entries(
        &self,
        console: console::Console,
        is_dry_run: bool,
    ) -> anyhow::Result<()> {
        let path_to_store = self.path_to_store.clone();

        let suffixes: Vec<_> = http_archive::get_archive_suffixes()
            .iter()
            .map(std::ffi::OsStr::new)
            .collect();

        let managed_top_level_dirs = self.get_managed_top_level_dirs();

        let all_entries: Vec<_> = managed_top_level_dirs
            .iter()
            .flat_map(|dir| {
                walkdir::WalkDir::new(path_to_store.join(dir))
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .collect::<Vec<_>>()
            })
            .collect();

        let entries = all_entries.into_iter().filter(|e| {
            let path = e.path();
            if path.is_dir() {
                let extension = path.extension().unwrap_or_default();
                if suffixes.contains(&extension) {
                    true
                } else {
                    let extension = extension.to_string_lossy();
                    extension.starts_with("git") && path.join(".git").exists()
                }
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
                    logger(console.clone()).info(
                        format!("Unlisted Entry (not removing, dry run): {display}",).as_str(),
                    );
                } else {
                    logger(console.clone())
                        .info(format!("Unlisted Entry (removing): {display}").as_str());

                    if entry_path.starts_with(&path_to_store) {
                        match std::fs::remove_dir_all(entry_path) {
                            Ok(()) => {}
                            Err(e) => {
                                logger(console.clone()).error(
                                    format!("Failed to remove {display}: {e} - remove manually")
                                        .as_str(),
                                );
                            }
                        }
                    } else {
                        logger(console.clone()).error(
                            format!("Internal Error: can't remove {display} - not in store")
                                .as_str(),
                        );
                    }
                }
            }
        }

        // Remove unlisted files under the home subdirectory
        let home_dir = path_to_store.join(SPACES_STORE_HOME);
        if home_dir.exists() {
            for entry in walkdir::WalkDir::new(&home_dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
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
                        logger(console.clone()).info(
                            format!("Unlisted Home Entry (not removing, dry run): {display}")
                                .as_str(),
                        );
                    } else {
                        logger(console.clone())
                            .info(format!("Unlisted Home Entry (removing): {display}").as_str());
                        if entry_path.starts_with(&path_to_store) {
                            match std::fs::remove_file(entry_path) {
                                Ok(()) => {}
                                Err(e) => {
                                    logger(console.clone()).error(
                                        format!(
                                            "Failed to remove {display}: {e} - remove manually"
                                        )
                                        .as_str(),
                                    );
                                }
                            }
                        } else {
                            logger(console.clone()).error(
                                format!("Internal Error: can't remove {display} - not in store")
                                    .as_str(),
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn fix(
        &mut self,
        console: console::Console,
        is_dry_run: bool,
        is_ci: ci::IsCi,
    ) -> anyhow::Result<()> {
        let group = ci::GithubLogGroup::new_group(console.clone(), is_ci, "Spaces Store Fix")?;
        let log = logger(console.clone());

        let mut remove_entries = Vec::new();
        let mut delete_directories = Vec::new();
        let path_to_store = self.path_to_store.clone();
        let managed_top_level_dirs = self.get_managed_top_level_dirs();

        let unmanaged_candidates =
            get_unmanaged_dir_entries(&path_to_store, &managed_top_level_dirs);

        let total = (unmanaged_candidates.len() + self.entries.len()) as u64;
        let mut progress = console::Progress::new(console.clone(), "scanning", Some(total), None);

        for (key, value) in self.entries.iter_mut() {
            log.info(format!("Checking {key}").as_str());
            progress.set_message(key.as_ref());
            let path = path_to_store.join(key.as_ref());
            if !path.exists() {
                remove_entries.push(key.clone());
            }

            let updated_size = get_size_of_path(path.as_path()).unwrap_or(0);
            if updated_size != value.size {
                if !is_dry_run {
                    let bytesize = bytesize::ByteSize(updated_size);
                    log.info(format!(" Updated size {}", bytesize.display()).as_str());
                    value.size = updated_size;
                } else {
                    let bytesize = bytesize::ByteSize(updated_size);
                    log.info(format!(" Updating size {}", bytesize.display()).as_str());
                }
            }

            let key_path = std::path::Path::new(key.as_ref());
            let is_git_suffix = key_path
                .extension()
                .map(|e| e.to_string_lossy().starts_with("git"))
                .unwrap_or(false);

            if !is_git_suffix {
                let result = http_archive::check_downloaded_archive(&path);
                if let Err(err) = result {
                    log.warning(format!("{key} is corrupted. {err}").as_str());
                    remove_entries.push(key.clone());
                    delete_directories.push(path);
                }
            }
            progress.increment(1);
        }

        if !is_dry_run {
            make_path_dirs_user_writable(path_to_store.as_path());

            for key in remove_entries {
                log.info(format!("Removing entry: {key}").as_str());
                self.entries.remove(&key);
            }

            for path in delete_directories {
                if path.starts_with(path_to_store.as_path()) {
                    log.info(format!("Deleting directory: {}", path.display()).as_str());
                    std::fs::remove_dir_all(path.as_path()).unwrap_or_else(|err| {
                        log.warning(
                            format!("Failed to delete directory {}: {err}", path.display())
                                .as_str(),
                        );
                    });
                } else {
                    log.error(
                        format!("Cannot delete out of store directory: {}", path.display())
                            .as_str(),
                    );
                }
            }
        }

        self.remove_unlisted_entries(console.clone(), is_dry_run)
            .context(format_context!("While checking for unlisted entries"))?;

        // Always recompute unmanaged directory sizes, bypassing the modification time cache.
        if !unmanaged_candidates.is_empty() {
            for entry in unmanaged_candidates {
                let name = entry.file_name().to_string_lossy().to_string();
                log.info(format!("Checking {name} (unmanaged)").as_str());
                progress.set_message(name.as_str());
                let size = get_size_of_path(&entry.path()).unwrap_or(0);
                let dir_modified = get_dir_modified_system_time(&entry.path());
                self.unmanaged.insert(
                    name.into(),
                    UnmanagedDirectory {
                        modified_system_time: dir_modified,
                        size,
                    },
                );
                progress.increment(1);
            }
        }
        progress.set_finalize_none();

        group.end_group(console.clone(), is_ci)?;
        Ok(())
    }

    pub fn prune(
        &mut self,
        console: console::Console,
        age: u16,
        is_dry_run: bool,
        is_ci: ci::IsCi,
    ) -> anyhow::Result<()> {
        let group = ci::GithubLogGroup::new_group(console.clone(), is_ci, "Spaces Store Prune")?;
        let mut remove_entries = Vec::new();

        let path_to_store = self.path_to_store.clone();
        if !is_dry_run {
            make_path_dirs_user_writable(path_to_store.as_path());
        }

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

        let mut progress = console::Progress::new(
            console.clone(),
            "store-prune",
            Some(remove_entries.len() as u64),
            None,
        );

        for (key, age, size, path) in remove_entries {
            logger(console.clone()).info(format!("Pruning {key}: {size}").as_str());
            logger(console.clone()).info(format!("- Age: {age} days").as_str());
            logger(console.clone()).info(format!("- Size: {size}").as_str());
            progress.set_message(&format!("pruning {key} with {size}"));
            if !is_dry_run {
                self.entries.remove(&key);
                let remove_result = if path.is_file() {
                    std::fs::remove_file(&path)
                } else {
                    std::fs::remove_dir_all(&path)
                };
                if let Err(e) = remove_result {
                    logger(console.clone())
                        .error(format!("Failed to remove entry: {key}, error: {e}").as_str());
                } else {
                    logger(console.clone()).info("- Removed.");
                }
            } else {
                logger(console.clone()).info("- Dry run. Not removed.");
            }
            progress.increment(1);
        }

        let total_removed_message = if is_dry_run {
            format!("Total to remove in dry run: {total_size_removed}")
        } else {
            format!("Total removed: {total_size_removed}")
        };
        logger(console.clone()).info(total_removed_message.as_str());
        let finalize_message = if is_dry_run {
            format!("dry run: would prune {total_size_removed}")
        } else {
            format!("pruned {total_size_removed}")
        };
        progress.set_finalize_lines(logger::make_finalize_line(
            logger::FinalType::Finished,
            progress.elapsed(),
            finalize_message.as_str(),
        ));

        group.end_group(console.clone(), is_ci)?;

        Ok(())
    }
}

fn get_unmanaged_dir_entries(
    path_to_store: &std::path::Path,
    managed_top_level_dirs: &std::collections::HashSet<String>,
) -> Vec<std::fs::DirEntry> {
    std::fs::read_dir(path_to_store)
        .map(|read_dir| {
            read_dir
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    name != MANIFEST_FILE_NAME && !managed_top_level_dirs.contains(&name)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn make_path_dirs_user_writable(path: &std::path::Path) {
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
    {
        if let Ok(metadata) = entry.metadata() {
            let mut perms = metadata.permissions();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                perms.set_mode(perms.mode() | 0o200);
            }
            #[cfg(windows)]
            {
                perms.set_readonly(false);
            }
            let _ = std::fs::set_permissions(entry.path(), perms);
        }
    }
}

fn system_time_as_secs(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            if d.subsec_nanos() >= 500_000_000 {
                secs + 1
            } else {
                secs
            }
        })
        .unwrap_or(0)
}

fn get_dir_modified_system_time(path: &std::path::Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
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
