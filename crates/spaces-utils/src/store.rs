use crate::{age, ci, http_archive, logger};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use bytesize::ByteSize;
use console::style::{Color, ContentStyle, StyledContent};
use serde::{Deserialize, Serialize};
use serde_with::{TimestampSeconds, serde_as};
use std::collections::HashMap;
use std::sync::Arc;

const MANIFEST_FILE_NAME: &str = "store.spaces.json";
pub const SPACES_STORE: &str = ".spaces/store";
pub const SPACES_STORE_RCACHE: &str = "rcache";
pub const SPACES_STORE_BARE: &str = "bare";
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
        /// Sort by name/age/size
        #[clap(long, default_value = "name")]
        sort_by: SortBy,
        /// Output format
        #[clap(long, value_enum, default_value_t = console::Format::Pretty)]
        format: console::Format,
    },
    /// Check the store for errors and delete any entries that have an error.
    Fix {
        /// Show which entries have errors and will be deleted without deleting the data
        #[clap(long)]
        dry_run: bool,
    },
    /// Prune the store by deleting entries that are this age or older (inclusive).
    Prune {
        /// Delete entries this age or older in days (inclusive). Use --age=0 to prune all entries.
        #[clap(long, default_value = "30")]
        age: u16,
        /// Show which entries will be deleted without deleting the data
        #[clap(long)]
        dry_run: bool,
        /// Prune only the rule cache, leaving store entries untouched
        #[clap(long)]
        rcache_only: bool,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum CloneType {
    #[default]
    Reference, // --reference clone (Default/Blobless)
    Worktree, // worktree (deprecated)
    Shallow,  // shallow clone
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceLink {
    // Absolute path to the workspace root
    pub workspace_root: Arc<str>,
    // Relative path of the repo within the workspace
    pub repo_path: Arc<str>,
    // Last time this link was verified/updated
    pub last_verified: u128,
    // Clone type used (for information/debugging)
    pub clone_type: CloneType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    last_used: u128,
    size: u64,
    // Track which workspaces reference this store entry
    // Key: absolute workspace path
    #[serde(default)]
    workspace_links: HashMap<Arc<str>, WorkspaceLink>,
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

#[derive(Debug)]
pub struct VerifyResult {
    pub stale_links_removed: usize,
    pub invalid_workspaces: Vec<Arc<str>>,
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
            if let Some(existing_entry) = self.entries.get_mut(&key) {
                // Merge workspace links from other into existing entry
                for (ws_root, ws_link) in value.workspace_links {
                    existing_entry.workspace_links.insert(ws_root, ws_link);
                }
                // Update last_used and size to the newer values
                existing_entry.last_used = value.last_used;
                existing_entry.size = value.size;
            } else {
                // Entry doesn't exist, insert it
                self.entries.insert(key, value);
            }
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
                workspace_links: HashMap::new(),
            });

        map_entry.last_used = timestamp;
        map_entry.size = size;

        Ok(())
    }

    /// Add a workspace link to a store entry
    pub fn add_workspace_link(
        &mut self,
        store_path: &std::path::Path,
        workspace_root: Arc<str>,
        repo_path: Arc<str>,
        clone_type: CloneType,
    ) -> anyhow::Result<()> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);

        let store_path_str: Arc<str> = store_path.display().to_string().into();

        if let Some(entry) = self.entries.get_mut(&store_path_str) {
            let link = WorkspaceLink {
                workspace_root: workspace_root.clone(),
                repo_path,
                last_verified: timestamp,
                clone_type,
            };
            entry.workspace_links.insert(workspace_root, link);
        } else {
            return Err(format_error!(
                "Store entry not found for path: {}",
                store_path.display()
            ));
        }

        Ok(())
    }

    /// Remove a specific workspace link from a store entry
    pub fn remove_workspace_link(
        &mut self,
        store_path: &std::path::Path,
        workspace_root: &str,
    ) -> anyhow::Result<()> {
        let store_path_str: Arc<str> = store_path.display().to_string().into();

        if let Some(entry) = self.entries.get_mut(&store_path_str) {
            entry.workspace_links.remove(workspace_root);
        }

        Ok(())
    }

    /// Verify all links and remove stale ones (where workspace no longer exists)
    pub fn verify_and_clean_links(&mut self) -> anyhow::Result<VerifyResult> {
        let mut stale_links_removed = 0;
        let mut invalid_workspaces: Vec<Arc<str>> = Vec::new();
        let mut seen_invalid: std::collections::HashSet<Arc<str>> =
            std::collections::HashSet::new();

        for entry in self.entries.values_mut() {
            let mut stale_keys = Vec::new();

            for (workspace_root, _) in entry.workspace_links.iter() {
                let workspace_path = std::path::Path::new(workspace_root.as_ref());
                if !workspace_path.exists() {
                    stale_keys.push(workspace_root.clone());
                    if !seen_invalid.contains(workspace_root) {
                        invalid_workspaces.push(workspace_root.clone());
                        seen_invalid.insert(workspace_root.clone());
                    }
                }
            }

            for key in stale_keys {
                entry.workspace_links.remove(&key);
                stale_links_removed += 1;
            }
        }

        Ok(VerifyResult {
            stale_links_removed,
            invalid_workspaces,
        })
    }

    pub fn show_info(
        &mut self,
        console: console::Console,
        sort_by: SortBy,
        format: console::Format,
        is_ci: ci::IsCi,
        rcache_path: &std::path::Path,
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
            SortBy::Size => entries.sort_by_key(|b| std::cmp::Reverse(b.1.size)),
            // oldest to newest
            SortBy::Age => entries.sort_by_key(|b| std::cmp::Reverse(b.1.get_age(now))),
        }

        // Collect managed entry info
        let mut info_entries: Vec<StoreInfoEntry> = Vec::new();
        for (key, value) in entries.iter() {
            let path = self.get_path_in_store(std::path::Path::new(key.as_ref()));
            let path_missing = !path.exists();
            if path_missing || value.size == 0 {
                is_fix_needed = true;
            }
            let workspace_count = value.workspace_links.len();
            let stale_links = value
                .workspace_links
                .values()
                .filter(|link| !std::path::Path::new(link.workspace_root.as_ref()).exists())
                .count();

            info_entries.push(StoreInfoEntry {
                path: key.to_string(),
                size_bytes: value.size,
                age_days: value.get_age(now),
                managed: true,
                path_missing,
                workspace_count,
                stale_links,
            });
        }

        match sort_by {
            SortBy::Name => unmanaged.sort_by(|a, b| a.0.cmp(&b.0)),
            SortBy::Size => unmanaged.sort_by_key(|b| std::cmp::Reverse(b.1)),
            // oldest modified first
            SortBy::Age => unmanaged.sort_by_key(|a| a.2),
        }

        for (name, size, _) in &unmanaged {
            info_entries.push(StoreInfoEntry {
                path: name.clone(),
                size_bytes: *size,
                age_days: 0,
                managed: false,
                path_missing: false,
                workspace_count: 0,
                stale_links: 0,
            });
        }

        let total_size: u64 = info_entries.iter().map(|e| e.size_bytes).sum();

        match format {
            console::Format::Pretty => {
                emit_pretty_info(&console, &info_entries, total_size, is_fix_needed);
            }
            console::Format::Yaml => {
                console.write(
                    &serialise_store_info_yaml(&info_entries, total_size)
                        .context(format_context!("Failed to serialize store info as YAML"))?,
                )?;
            }
            console::Format::Json => {
                console.write(
                    &serialise_store_info_json(&info_entries, total_size)
                        .context(format_context!("Failed to serialize store info as JSON"))?,
                )?;
            }
        }

        group.end_group(console.clone(), is_ci)?;

        crate::rcache::show_info(rcache_path, console.clone(), is_ci, &format)
            .context(format_context!("While showing rcache info"))?;

        let bare_path = self.path_to_store.join(SPACES_STORE_BARE);

        // Count unique workspaces linking to bare repositories
        let mut bare_workspaces = std::collections::HashSet::new();
        let bare_prefix = format!("{}/", SPACES_STORE_BARE);
        for (key, entry) in &self.entries {
            if key.starts_with(&bare_prefix) {
                for workspace_root in entry.workspace_links.keys() {
                    bare_workspaces.insert(workspace_root.clone());
                }
            }
        }
        let workspace_count = bare_workspaces.len();

        show_bare_info(&bare_path, console.clone(), is_ci, &format, workspace_count)
            .context(format_context!("While showing bare repositories info"))?;

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

        // Clean up stale workspace links
        log.info("Verifying workspace links");

        if is_dry_run {
            // Count stale links without removing them
            let mut stale_count = 0;
            for entry in self.entries.values() {
                for workspace_root in entry.workspace_links.keys() {
                    let workspace_path = std::path::Path::new(workspace_root.as_ref());
                    if !workspace_path.exists() {
                        stale_count += 1;
                    }
                }
            }

            if stale_count > 0 {
                log.info(format!("Would remove {} stale workspace links", stale_count).as_str());
            }
        } else {
            // Actually remove stale links
            let verify_result = self
                .verify_and_clean_links()
                .context(format_context!("While verifying workspace links"))?;

            if verify_result.stale_links_removed > 0 {
                log.info(
                    format!(
                        "Removed {} stale workspace links",
                        verify_result.stale_links_removed
                    )
                    .as_str(),
                );
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
            if entry_age >= age as u128 {
                // Check for active workspace links
                let active_links: Vec<_> = entry
                    .workspace_links
                    .values()
                    .filter(|link| std::path::Path::new(link.workspace_root.as_ref()).exists())
                    .collect();

                if !active_links.is_empty() {
                    logger(console.clone()).warning(
                        format!(
                            "Skipping {key}: {} active workspace(s) still reference this entry",
                            active_links.len()
                        )
                        .as_str(),
                    );
                    continue;
                }

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
            let mut item_progress =
                console::Progress::new(console.clone(), key.as_ref(), None, None);
            item_progress.set_message(&format!("{size}, age {age} days"));
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
                    item_progress.set_finalize_lines(logger::make_finalize_line(
                        logger::FinalType::Failed,
                        item_progress.elapsed(),
                        format!("failed to remove {key}: {e}").as_str(),
                    ));
                } else {
                    item_progress.set_finalize_lines(logger::make_finalize_line(
                        logger::FinalType::Completed,
                        item_progress.elapsed(),
                        format!("{key}: {size}").as_str(),
                    ));
                }
            } else {
                item_progress.set_finalize_lines(logger::make_finalize_line(
                    logger::FinalType::Cancelled,
                    item_progress.elapsed(),
                    format!("dry run: {key} ({size})").as_str(),
                ));
            }
            progress.increment(1);
        }

        let finalize_message = if is_dry_run {
            format!("dry run: would prune {total_size_removed} from store")
        } else {
            format!("pruned {total_size_removed} from store")
        };
        progress.set_finalize_lines(logger::make_finalize_line(
            logger::FinalType::Finished,
            progress.elapsed(),
            finalize_message.as_str(),
        ));

        logger(console.clone()).message(finalize_message.as_str());
        group.end_group(console.clone(), is_ci)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// store info output types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct StoreInfoEntry {
    path: String,
    size_bytes: u64,
    age_days: u128,
    managed: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    path_missing: bool,
    workspace_count: usize,
    stale_links: usize,
}

#[derive(Debug, Serialize)]
struct StoreInfoOutput<'a> {
    entries: &'a [StoreInfoEntry],
    total_size_bytes: u64,
}

fn serialise_store_info_json(
    entries: &[StoreInfoEntry],
    total_size_bytes: u64,
) -> anyhow::Result<String> {
    let output = StoreInfoOutput {
        entries,
        total_size_bytes,
    };
    serde_json::to_string_pretty(&output).context(format_context!(
        "Internal Error: failed to serialize store info for JSON"
    ))
}

fn serialise_store_info_yaml(
    entries: &[StoreInfoEntry],
    total_size_bytes: u64,
) -> anyhow::Result<String> {
    let output = StoreInfoOutput {
        entries,
        total_size_bytes,
    };
    serde_yaml::to_string(&output).context(format_context!(
        "Internal Error: failed to serialize store info for YAML"
    ))
}

// ---------------------------------------------------------------------------
// store info pretty output
// ---------------------------------------------------------------------------

fn age_style(age_days: u128) -> ContentStyle {
    let color = if age_days < 7 {
        Color::Green
    } else if age_days <= 30 {
        Color::DarkYellow
    } else {
        Color::DarkRed
    };
    ContentStyle {
        foreground_color: Some(color),
        background_color: None,
        underline_color: None,
        attributes: Default::default(),
    }
}

fn emit_separator(console: &console::Console, width: usize) {
    let mut line = console::Line::default();
    line.push(console::Span::new_styled_lossy(StyledContent::new(
        console::key_style(),
        "─".repeat(width),
    )));
    console.emit_line(line);
}

fn emit_pretty_summary(
    console: &console::Console,
    entries: &[StoreInfoEntry],
    total_size: u64,
    is_fix_needed: bool,
) {
    let managed_count = entries.iter().filter(|e| e.managed).count();
    let unmanaged_count = entries.len() - managed_count;
    let managed_size: u64 = entries
        .iter()
        .filter(|e| e.managed)
        .map(|e| e.size_bytes)
        .sum();
    let unmanaged_size = total_size - managed_size;

    // Managed row
    {
        let mut line = console::Line::default();
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::key_style(),
            format!("  {:<12}", "Managed"),
        )));
        line.push(console::Span::new_unstyled_lossy(format!(
            "{:>4} entries   {}",
            managed_count,
            ByteSize(managed_size).display()
        )));
        console.emit_line(line);
    }

    // Unmanaged row
    {
        let mut line = console::Line::default();
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::key_style(),
            format!("  {:<12}", "Unmanaged"),
        )));
        line.push(console::Span::new_unstyled_lossy(format!(
            "{:>4} entries   {}",
            unmanaged_count,
            ByteSize(unmanaged_size).display()
        )));
        console.emit_line(line);
    }

    // Workspace links row
    {
        let entries_with_links = entries.iter().filter(|e| e.workspace_count > 0).count();
        let total_links: usize = entries.iter().map(|e| e.workspace_count).sum();
        let entries_with_stale: usize = entries.iter().filter(|e| e.stale_links > 0).count();

        if entries_with_links > 0 || entries_with_stale > 0 {
            let mut line = console::Line::default();
            line.push(console::Span::new_styled_lossy(StyledContent::new(
                console::key_style(),
                format!("  {:<12}", "Workspaces"),
            )));
            line.push(console::Span::new_unstyled_lossy(format!(
                "{:>4} links across {} entries",
                total_links, entries_with_links
            )));
            if entries_with_stale > 0 {
                line.push(console::Span::new_styled_lossy(StyledContent::new(
                    console::warning_style(),
                    format!("   ({} with stale links)", entries_with_stale),
                )));
            }
            console.emit_line(line);
        }
    }

    // Total row
    {
        let mut line = console::Line::default();
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::total_style(),
            format!(
                "  {:<12}{:>4} entries   {}",
                "Total",
                entries.len(),
                ByteSize(total_size).display()
            ),
        )));
        if is_fix_needed {
            line.push(console::Span::new_styled_lossy(StyledContent::new(
                console::warning_style(),
                "   !! run `spaces store fix`".to_owned(),
            )));
        }
        console.emit_line(line);
    }
}

fn emit_pretty_age_histogram(console: &console::Console, entries: &[StoreInfoEntry]) {
    let managed: Vec<_> = entries.iter().filter(|e| e.managed).collect();
    if managed.is_empty() {
        return;
    }

    let fresh = managed.iter().filter(|e| e.age_days < 7).count();
    let aging = managed
        .iter()
        .filter(|e| e.age_days >= 7 && e.age_days <= 30)
        .count();
    let stale = managed.iter().filter(|e| e.age_days > 30).count();
    let max_count = fresh.max(aging).max(stale).max(1);
    const BAR_WIDTH: usize = 20;

    let mut heading = console::Line::default();
    heading.push(console::Span::new_styled_lossy(StyledContent::new(
        console::total_style(),
        "Age distribution".to_owned(),
    )));
    console.emit_line(heading);

    for (label, count, representative_age) in [
        ("fresh  < 7d ", fresh, 0u128),
        ("aging 7-30d ", aging, 14u128),
        ("stale  > 30d", stale, 60u128),
    ] {
        let bar_len = count * BAR_WIDTH / max_count;
        let bar = "█".repeat(bar_len);
        let mut line = console::Line::default();
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::key_style(),
            format!("  {label}  "),
        )));
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            age_style(representative_age),
            format!("{bar:<BAR_WIDTH$}"),
        )));
        line.push(console::Span::new_unstyled_lossy(format!("  {count}")));
        console.emit_line(line);
    }
}

fn emit_top_entries_group(console: &console::Console, heading: &str, entries: &[&StoreInfoEntry]) {
    const TOP_N: usize = 5;
    let mut by_size = entries.to_vec();
    by_size.sort_by_key(|b| std::cmp::Reverse(b.size_bytes));
    let top = &by_size[..TOP_N.min(by_size.len())];

    if top.is_empty() {
        return;
    }

    let mut heading_line = console::Line::default();
    heading_line.push(console::Span::new_styled_lossy(StyledContent::new(
        console::total_style(),
        heading.to_owned(),
    )));
    console.emit_line(heading_line);

    let name_width = top.iter().map(|e| e.path.len()).max().unwrap_or(10).max(10);

    for entry in top {
        let size_str = ByteSize(entry.size_bytes).display().to_string();
        let mut line = console::Line::default();
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::name_style(),
            format!("  {:<name_width$}", entry.path),
        )));
        line.push(console::Span::new_unstyled_lossy(format!(
            "  {size_str:<10}"
        )));
        if entry.path_missing || entry.size_bytes == 0 {
            line.push(console::Span::new_styled_lossy(StyledContent::new(
                console::warning_style(),
                "  !!".to_owned(),
            )));
        }
        console.emit_line(line);
    }
}

fn emit_pretty_top_entries(console: &console::Console, entries: &[StoreInfoEntry]) {
    let unmanaged: Vec<_> = entries.iter().filter(|e| !e.managed).collect();
    emit_top_entries_group(console, "Top 5 unmanaged by size", &unmanaged);
}

fn emit_pretty_issues(console: &console::Console, entries: &[StoreInfoEntry]) {
    let issues: Vec<_> = entries
        .iter()
        .filter(|e| e.path_missing || e.size_bytes == 0)
        .collect();

    if issues.is_empty() {
        return;
    }

    console.emit_line(console::Line::default());
    emit_separator(console, 56);

    let mut heading = console::Line::default();
    heading.push(console::Span::new_styled_lossy(StyledContent::new(
        console::warning_style(),
        format!("Issues  ({} entries need attention)", issues.len()),
    )));
    console.emit_line(heading);

    for entry in issues {
        let mut line = console::Line::default();
        let reason = if entry.path_missing {
            "path does not exist"
        } else {
            "size is zero"
        };
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::warning_style(),
            format!("  !! {reason:<22}"),
        )));
        line.push(console::Span::new_unstyled_lossy(&entry.path));
        console.emit_line(line);
    }
}

fn emit_pretty_info(
    console: &console::Console,
    entries: &[StoreInfoEntry],
    total_size: u64,
    is_fix_needed: bool,
) {
    emit_separator(console, 56);
    emit_pretty_summary(console, entries, total_size, is_fix_needed);
    emit_separator(console, 56);
    console.emit_line(console::Line::default());
    emit_pretty_age_histogram(console, entries);
    console.emit_line(console::Line::default());
    emit_pretty_top_entries(console, entries);
    emit_pretty_issues(console, entries);
    console.emit_line(console::Line::default());
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
                    name != MANIFEST_FILE_NAME
                        && name != SPACES_STORE_RCACHE
                        && name != SPACES_STORE_BARE
                        && !managed_top_level_dirs.contains(&name)
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

#[derive(serde::Serialize, serde::Deserialize)]
struct BareInfoOutput {
    bare_size_bytes: u64,
    workspace_count: usize,
}

fn serialise_bare_info_json(
    bare_size_bytes: u64,
    workspace_count: usize,
) -> anyhow::Result<String> {
    let output = BareInfoOutput {
        bare_size_bytes,
        workspace_count,
    };
    serde_json::to_string_pretty(&output)
        .context(format_context!("Failed to serialize bare info as JSON"))
}

fn serialise_bare_info_yaml(
    bare_size_bytes: u64,
    workspace_count: usize,
) -> anyhow::Result<String> {
    let output = BareInfoOutput {
        bare_size_bytes,
        workspace_count,
    };
    serde_yaml::to_string(&output).context(format_context!("Failed to serialize bare info as YAML"))
}

fn bare_separator(console: &console::Console, width: usize) {
    let mut line = console::Line::default();
    line.push(console::Span::new_styled_lossy(StyledContent::new(
        console::key_style(),
        "─".repeat(width),
    )));
    console.emit_line(line);
}

fn emit_pretty_bare_info(console: &console::Console, bare_size: u64, workspace_count: usize) {
    // heading
    {
        let mut line = console::Line::default();
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::total_style(),
            "Bare Repositories".to_owned(),
        )));
        console.emit_line(line);
    }

    // size row
    {
        let mut line = console::Line::default();
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::key_style(),
            format!("  {:<14}", "Total Size"),
        )));
        line.push(console::Span::new_unstyled_lossy(format!(
            "{}",
            ByteSize(bare_size).display()
        )));
        console.emit_line(line);
    }

    // workspace count row
    {
        let mut line = console::Line::default();
        line.push(console::Span::new_styled_lossy(StyledContent::new(
            console::key_style(),
            format!("  {:<14}", "Workspaces"),
        )));
        line.push(console::Span::new_unstyled_lossy(format!(
            "{}",
            workspace_count
        )));
        console.emit_line(line);
    }

    bare_separator(console, 56);
}

fn show_bare_info(
    bare_path: &std::path::Path,
    console: console::Console,
    is_ci: ci::IsCi,
    format: &console::Format,
    workspace_count: usize,
) -> anyhow::Result<()> {
    if !bare_path.exists() {
        return Ok(());
    }

    let group =
        ci::GithubLogGroup::new_group(console.clone(), is_ci, "Spaces Bare Repositories Info")?;

    let bare_size = get_size_of_path(bare_path).unwrap_or(0);

    match format {
        console::Format::Pretty => {
            emit_pretty_bare_info(&console, bare_size, workspace_count);
        }
        console::Format::Yaml => {
            console.write(
                &serialise_bare_info_yaml(bare_size, workspace_count)
                    .context(format_context!("Failed to serialize bare info as YAML"))?,
            )?;
        }
        console::Format::Json => {
            console.write(
                &serialise_bare_info_json(bare_size, workspace_count)
                    .context(format_context!("Failed to serialize bare info as JSON"))?,
            )?;
        }
    }

    group.end_group(console.clone(), is_ci)?;
    Ok(())
}
