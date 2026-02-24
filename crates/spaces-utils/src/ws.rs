use crate::{changes, graph, inputs, logger, store};
use anyhow::Context;
use anyhow_source_location::format_context;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const SPACES_LOGS_NAME: &str = ".spaces/logs";
pub const METRICS_FILE_NAME: &str = ".spaces/metrics.spaces.json";
pub const SETTINGS_FILE_NAME: &str = ".spaces/settings.spaces.json";
const CHECKOUT_FILE_NAME: &str = ".spaces/checkout.spaces.json";
const BIN_SETTINGS_FILE_NAME: &str = "build/workspace.settings.3.spaces";
pub const SPACES_WORKSPACE_ENV_VAR: &str = "SPACES_WORKSPACE";
const SPACES_HOME_ENV_VAR: &str = "SPACES_HOME";

fn logger(progress: &mut printer::MultiProgressBar) -> logger::Logger<'_> {
    logger::Logger::new_progress(progress, "ws".into())
}

pub fn get_checkout_store_path() -> Arc<str> {
    if let Ok(spaces_home) = std::env::var(SPACES_HOME_ENV_VAR) {
        return format!("{spaces_home}/{}", store::SPACES_STORE).into();
    }
    if let Ok(Some(home_path)) = homedir::my_home() {
        return format!("{}/{}", home_path.to_string_lossy(), store::SPACES_STORE).into();
    }
    panic!("Failed to get home directory");
}

pub fn get_checkout_store_path_as_path() -> Arc<std::path::Path> {
    std::path::Path::new(get_checkout_store_path().as_ref()).into()
}

pub fn get_spaces_tools_path(store_path: &str) -> Arc<str> {
    format!("{store_path}/spaces_tools").into()
}

pub fn get_rcache_path(store_path: &str) -> Arc<str> {
    format!("{store_path}/{}", store::SPACES_STORE_RCACHE).into()
}

pub fn get_spaces_tools_path_as_path(store_path: &std::path::Path) -> Arc<std::path::Path> {
    store_path.join("spaces_tools").into()
}

pub fn get_spaces_tools_path_to_sysroot_bin(store_path: &std::path::Path) -> Arc<std::path::Path> {
    get_spaces_tools_path_as_path(store_path)
        .join("sysroot")
        .join("bin")
        .into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequiredType {
    Revision(Arc<str>),
    SemVer(Arc<str>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberRequirement {
    pub url: Arc<str>,                  // Full url used to reference the member
    pub required: Option<RequiredType>, // git rev, tag, or branch, if provided, it is used
}

impl MemberRequirement {
    fn get_hash_key(&self) -> Arc<str> {
        self.url.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub path: Arc<str>,            // relative workspace path to the member
    pub url: Arc<str>,             // Full url used to reference the member
    pub rev: Arc<str>,             // git tag or rev or sha256
    pub version: Option<Arc<str>>, // version e.g. 1.0.0
}

impl Member {
    fn get_hash_key(&self) -> Arc<str> {
        self.url.clone()
    }

    fn is_semver_match(&self, semver_required: &str) -> bool {
        self.version.as_ref().is_some_and(|version| {
            let semver_member_version = semver::Version::parse(version.as_ref()).ok();
            let semver_required_version = semver::VersionReq::parse(semver_required).ok();
            if let (Some(semver_member_version), Some(semver_required_version)) =
                (semver_member_version, semver_required_version)
            {
                semver_required_version.matches(&semver_member_version)
            } else {
                false
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub hash: Arc<str>,
}

impl Asset {
    pub fn new_contents(contents: &str) -> Self {
        let hash = blake3::hash(contents.as_bytes()).to_hex();
        Self {
            hash: hash.to_string().into(),
        }
    }
}

pub type Blake3Hash = [u8; blake3::OUT_LEN];

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct BinDetail {
    pub hash: Blake3Hash,
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Copy, Clone, Debug, PartialEq, strum::Display)]
pub enum IsDirty {
    No,
    YesModuleChange,
    YesModuleRemoved,
    YesEnvJsonMissing,
    YesTasksJsonMissing,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct BinSettings {
    pub tasks_json: Arc<str>,
    pub env_json: Arc<str>,
    pub run_all: HashSet<Arc<str>>,
    pub star_files: HashMap<Arc<str>, BinDetail>, // modules and hashes to detect changes
    pub changes: changes::Changes,
    pub inputs: inputs::Inputs,
    pub graph: graph::Graph,
    pub is_always_evaluate: bool,
}

impl BinSettings {
    fn new(path: &str) -> Self {
        Self::load(path).unwrap_or_default()
    }

    fn save(&self, path: &str) -> anyhow::Result<()> {
        let encoded =
            postcard::to_stdvec(self).context(format_context!("Failed to encode bin settings"))?;
        std::fs::write(path, encoded).context(format_context!("Failed to write to {path:?}"))?;
        Ok(())
    }

    fn load(path: &str) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path).context(format_context!("Failed to read {path:?}"))?;
        let bin_settings = postcard::from_bytes(&bytes)
            .context(format_context!("Failed to deserialize {path:?}"))?;
        Ok(bin_settings)
    }

    pub fn update_hashes(
        &mut self,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<(Vec<Arc<str>>, IsDirty)> {
        let mut result = IsDirty::No;
        let mut updated_modules = Vec::new();
        logger(progress).debug(format!("Checking {:?} star files", self.star_files.len()).as_str());
        let mut remove_list = Vec::new();
        for (module_path, bin_detail) in self.star_files.iter_mut() {
            progress.increment(1);
            let mod_path = std::path::Path::new(module_path.as_ref());
            logger(progress).debug(format!("Checking {mod_path:?} for changes").as_str());
            let modified = changes::get_modified_time(mod_path.metadata());
            if changes::is_modified(modified, bin_detail.modified) {
                if mod_path.exists() {
                    let content: Arc<str> = std::fs::read_to_string(module_path.as_ref())
                        .context(format_context!("Failed to read file {module_path}"))?
                        .into();
                    let content_hash = blake3::hash(content.as_bytes());
                    if content_hash.as_bytes() != &bin_detail.hash {
                        bin_detail.hash = content_hash.into();
                        bin_detail.modified = modified;
                        if result == IsDirty::No {
                            logger(progress).debug(format!("`{module_path}` is dirty").as_str());
                        }
                        result = IsDirty::YesModuleChange;
                        updated_modules.push(module_path.clone());
                    }
                } else {
                    remove_list.push(module_path.clone());
                    logger(progress).warning(format!("{mod_path:?} has been removed").as_str());
                }
            }
        }

        for module_path in remove_list {
            self.star_files.remove(&module_path);
            result = IsDirty::YesModuleRemoved;
        }

        if self.env_json.is_empty() {
            result = IsDirty::YesEnvJsonMissing;
        }
        if self.tasks_json.is_empty() {
            result = IsDirty::YesTasksJsonMissing;
        }

        Ok((updated_modules, result))
    }
}

fn get_unknown_version() -> Arc<str> {
    "unknown".into()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonSettings {
    pub store_path: Arc<str>,
    #[serde(default = "get_unknown_version")]
    pub spaces_version: Arc<str>,
    #[serde(default)]
    pub scanned_modules: HashSet<Arc<str>>,
    pub order: Vec<Arc<str>>,
    pub is_scanned: Option<bool>,
    pub minimum_version: Option<Arc<str>>,
    #[serde(default = "HashMap::new")]
    pub members: HashMap<Arc<str>, Vec<Member>>,
    #[serde(default = "HashMap::new")]
    pub assets: HashMap<Arc<str>, Asset>,
    #[serde(default = "Vec::new")]
    pub dev_branches: Vec<Arc<str>>,
    #[serde(skip)]
    pub bin_settings: BinSettings,
}

impl Default for JsonSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonSettings {
    fn new() -> Self {
        Self {
            store_path: get_checkout_store_path(),
            order: Vec::new(),
            spaces_version: env!("CARGO_PKG_VERSION").into(),
            is_scanned: None,
            scanned_modules: HashSet::new(),
            members: HashMap::new(),
            assets: HashMap::new(),
            minimum_version: None,
            dev_branches: Vec::new(),
            bin_settings: Default::default(),
        }
    }

    fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .context(format_context!("Failed to read load order file {path}"))?;
        let settings: JsonSettings = serde_json::from_str(content.as_str())
            .context(format_context!("Failed to parse load order file {path}"))?;
        Ok(settings)
    }

    pub fn push_module(&mut self, module: Arc<str>) {
        self.order.push(module);
    }

    pub fn push_member(&mut self, member: Member) {
        let entry = self
            .members
            .entry(member.get_hash_key().clone())
            .or_default();
        entry.push(member);
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let path = std::path::Path::new(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create parent directory {}",
                parent.display()
            ))?;
        }

        let content = serde_json::to_string_pretty(&self)
            .context(format_context!("Failed to serialize load order"))?;
        std::fs::write(path, content.as_str()).context(format_context!(
            "Failed to save settings file {}",
            path.display()
        ))?;
        Ok(())
    }

    pub fn get_path_to_member(&self, member_requirment: &MemberRequirement) -> Option<Arc<str>> {
        let entry_option = self.members.get(&member_requirment.get_hash_key());
        let path_option = entry_option.map(|member_entries| {
            let mut path_option = None;
            for member in member_entries {
                match &member_requirment.required {
                    Some(RequiredType::Revision(rev)) => {
                        if member.rev == *rev {
                            path_option = Some(member.path.clone());
                            break;
                        }
                    }
                    Some(RequiredType::SemVer(semver)) => {
                        if member.is_semver_match(semver.as_ref()) {
                            path_option = Some(member.path.clone());
                            break;
                        }
                    }
                    None => {
                        path_option = Some(member.path.clone());
                        break;
                    }
                }
            }
            path_option
        });

        path_option.flatten()
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct CheckoutSettings {
    pub links: HashSet<Arc<str>>,
    pub assets: HashMap<Arc<str>, Arc<str>>,
    pub updated_assets: HashSet<Arc<str>>,
}

impl CheckoutSettings {
    pub fn load() -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(CHECKOUT_FILE_NAME).context(format_context!(
            "Failed to read checkout settings file {CHECKOUT_FILE_NAME}"
        ))?;
        let settings: Self = serde_json::from_str(content.as_str()).context(format_context!(
            "Failed to parse checkout settings file {CHECKOUT_FILE_NAME}"
        ))?;
        Ok(settings)
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let path = std::path::Path::new(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create parent directory {}",
                parent.display()
            ))?;
        }

        let content = serde_json::to_string_pretty(&self)
            .context(format_context!("Failed to serialize checkout settings"))?;
        std::fs::write(path, content.as_str()).context(format_context!(
            "Failed to save settings file {}",
            path.display()
        ))?;
        Ok(())
    }

    pub fn is_asset_modified(&self, path: Arc<str>) -> bool {
        if let Some(entry) = self.assets.get(path.as_ref()) {
            match std::fs::read_to_string(path.as_ref()) {
                Ok(contents) => {
                    let file_hash = blake3::hash(contents.as_bytes());
                    file_hash.to_string() != entry.as_ref()
                }
                _ => false,
            }
        } else {
            false
        }
    }

    pub fn insert_asset(&mut self, path: Arc<str>, contents: Arc<str>) {
        let content_hash = blake3::hash(contents.as_bytes());
        let _ = self.assets.insert(path, content_hash.to_string().into());
    }
}

#[derive(Debug, PartialEq)]
pub enum IsJsonAvailable {
    No,
    Yes,
}

#[derive(Debug)]
pub struct Settings {
    pub json: JsonSettings,
    pub bin: BinSettings,
    pub checkout: CheckoutSettings,
    pub existing_checkout: Option<CheckoutSettings>,
}

impl Settings {
    pub fn load() -> (Self, IsJsonAvailable) {
        let mut is_json_available = IsJsonAvailable::No;
        let json_settings = {
            match JsonSettings::load(SETTINGS_FILE_NAME) {
                Ok(settings) => {
                    is_json_available = IsJsonAvailable::Yes;
                    settings
                }
                Err(_) => JsonSettings::default(),
            }
        };

        let checkout_settings = CheckoutSettings::default();

        let mut bin_settings = BinSettings::new(BIN_SETTINGS_FILE_NAME);
        if bin_settings.changes.skip_folders.is_empty() {
            bin_settings.changes.skip_folders = vec![SPACES_LOGS_NAME.into()];
        }

        (
            Self {
                json: json_settings,
                bin: bin_settings,
                checkout: checkout_settings,
                existing_checkout: None,
            },
            is_json_available,
        )
    }

    pub fn save_bin(&self) -> anyhow::Result<()> {
        self.bin
            .save(BIN_SETTINGS_FILE_NAME)
            .context(format_context!("Bin settings: {BIN_SETTINGS_FILE_NAME}"))?;
        Ok(())
    }

    pub fn save_json(&self) -> anyhow::Result<()> {
        self.json
            .save(SETTINGS_FILE_NAME)
            .context(format_context!("JSON settings: {SETTINGS_FILE_NAME}"))?;
        Ok(())
    }

    pub fn save_checkout(&self) -> anyhow::Result<()> {
        self.checkout
            .save(CHECKOUT_FILE_NAME)
            .context(format_context!("Checkout settings: {CHECKOUT_FILE_NAME}"))?;
        Ok(())
    }

    pub fn clear_inputs(&mut self) -> anyhow::Result<()> {
        self.bin.inputs.clear();
        Ok(())
    }

    pub fn clone_existing_checkout(&mut self) -> CheckoutSettings {
        if self.existing_checkout.is_none() {
            self.existing_checkout = Some(CheckoutSettings::load().unwrap_or_default());
        }
        self.existing_checkout.as_ref().unwrap().clone()
    }

    pub fn get_extraneous_files(&mut self) -> Vec<Arc<str>> {
        // which files exist in previous but not self
        let previous = self.clone_existing_checkout();
        let mut result = Vec::new();
        for (key, _hash) in previous.assets.iter() {
            if !self.checkout.assets.contains_key(key) && !previous.is_asset_modified(key.clone()) {
                result.push(key.clone());
            }
        }

        for value in previous.links.iter() {
            if !self.checkout.links.contains(value) {
                result.push(value.clone());
            }
        }

        for value in previous.updated_assets.iter() {
            if !self.checkout.updated_assets.contains(value) {
                result.push(value.clone());
            }
        }

        result
    }
}
