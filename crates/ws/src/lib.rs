use anyhow::Context;
use anyhow_source_location::format_context;
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const SPACES_LOGS_NAME: &str = ".spaces/logs";
pub const METRICS_FILE_NAME: &str = ".spaces/metrics.spaces.json";
const SETTINGS_FILE_NAME: &str = ".spaces/settings.spaces.json";
const BIN_SETTINGS_FILE_NAME: &str = "build/workspace.settings.spaces";
pub const SPACES_WORKSPACE_ENV_VAR: &str = "SPACES_WORKSPACE";
const SPACES_HOME_ENV_VAR: &str = "SPACES_HOME";

fn logger(progress: &mut printer::MultiProgressBar) -> logger::Logger<'_> {
    logger::Logger::new_progress(progress, "ws".into())
}

pub fn get_checkout_store_path() -> Arc<str> {
    if let Ok(spaces_home) = std::env::var(SPACES_HOME_ENV_VAR) {
        return format!("{}/.spaces/store", spaces_home).into();
    }
    if let Ok(Some(home_path)) = homedir::my_home() {
        return format!("{}/.spaces/store", home_path.to_string_lossy()).into();
    }
    panic!("Failed to get home directory");
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

#[derive(Debug, Encode, Decode, Default)]
pub struct BinDetail {
    pub hash: [u8; blake3::OUT_LEN],
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum IsDirty {
    No,
    Yes,
}

#[derive(Debug, Encode, Decode, Default)]
pub struct BinSettings {
    pub tasks_json: Arc<str>,
    pub env_json: Arc<str>,
    pub run_all: HashSet<Arc<str>>,
    pub star_files: HashMap<Arc<str>, BinDetail>, // modules and hashes to detect changes
    pub changes: changes::Changes,
    pub inputs: inputs::Inputs,
    pub is_always_evaluate: bool,
}

impl BinSettings {
    fn new(path: &str) -> Self {
        Self::load(path).unwrap_or_default()
    }

    fn save(&self, path: &str) -> anyhow::Result<()> {
        let encoded = bincode::encode_to_vec(self, bincode::config::standard())
            .context(format_context!("Failed to encode bin settings"))?;
        std::fs::write(path, encoded).context(format_context!("Failed to write to {path:?}"))?;
        Ok(())
    }

    fn load(path: &str) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path).context(format_context!("Failed to open {path:?}"))?;
        let reader = std::io::BufReader::new(file);
        let bin_settings = bincode::decode_from_reader(reader, bincode::config::standard())
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
        for (module_path, bin_detail) in self.star_files.iter_mut() {
            progress.increment(1);
            let mod_path = std::path::Path::new(module_path.as_ref());
            logger(progress).debug(format!("Checking {:?} for changes", mod_path).as_str());
            let modified = changes::get_modified_time(mod_path.metadata());
            if changes::is_modified(modified, bin_detail.modified) {
                let content: Arc<str> = std::fs::read_to_string(module_path.as_ref())
                    .context(format_context!("Failed to read file {module_path}"))?
                    .into();
                let content_hash = blake3::hash(content.as_bytes());
                if content_hash.as_bytes() != &bin_detail.hash {
                    bin_detail.hash = content_hash.into();
                    bin_detail.modified = modified;
                    result = IsDirty::Yes;
                    updated_modules.push(module_path.clone());
                }
            }
        }

        if self.env_json.is_empty() || self.tasks_json.is_empty() {
            result = IsDirty::Yes
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
    #[serde(default = "HashMap::new")]
    pub members: HashMap<Arc<str>, Vec<Member>>,
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

#[derive(Debug, PartialEq)]
pub enum IsJsonAvailable {
    No,
    Yes,
}

#[derive(Debug)]
pub struct Settings {
    pub json: JsonSettings,
    pub bin: BinSettings,
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

        let mut bin_settings = BinSettings::new(BIN_SETTINGS_FILE_NAME);
        if bin_settings.changes.skip_folders.is_empty() {
            bin_settings.changes.skip_folders = vec![SPACES_LOGS_NAME.into()];
        }

        (
            Self {
                json: json_settings,
                bin: bin_settings,
            },
            is_json_available,
        )
    }

    pub fn save_bin(&self) -> anyhow::Result<()> {
        self.bin
            .save(BIN_SETTINGS_FILE_NAME)
            .context(format_context!("Bin settings"))?;
        Ok(())
    }

    pub fn save_json(&self) -> anyhow::Result<()> {
        self.json
            .save(SETTINGS_FILE_NAME)
            .context(format_context!("Bin settings"))?;
        Ok(())
    }

    pub fn clear_inputs(&mut self) -> anyhow::Result<()> {
        self.bin.inputs.clear();
        Ok(())
    }
}
