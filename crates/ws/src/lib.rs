use anyhow::Context;
use anyhow_source_location::format_context;
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, HashMap};
use std::sync::Arc;

pub const SPACES_LOGS_NAME: &str = ".spaces/logs";
pub const METRICS_FILE_NAME: &str = ".spaces/metrics.spaces.json";
const SETTINGS_FILE_NAME: &str = ".spaces/settings.spaces.json";
const BIN_SETTINGS_FILE_NAME: &str = ".spaces/settings.spaces";
pub const SPACES_WORKSPACE_ENV_VAR: &str = "SPACES_WORKSPACE";
const SPACES_HOME_ENV_VAR: &str = "SPACES_HOME";

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
        self.version.as_ref().map_or(false, |version| {
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

#[derive(Debug, Encode, Decode)]
pub struct Task {
    pub name: Arc<str>,
    pub hash: Arc<str>,
}

#[derive(Debug, Encode, Decode, Default)]
pub struct BinSettings {
    pub tasks: Vec<Task>,
    pub scanned_modules: HashMap<Arc<str>, Arc<str>>,
}

fn get_unknown_version() -> Arc<str> {
    "unknown".into()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
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

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

impl Settings {
    pub fn new() -> Self {
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

    pub fn load(workspace_path: &str) -> anyhow::Result<Self> {
        let path = format!("{workspace_path}/{SETTINGS_FILE_NAME}");
        let content = std::fs::read_to_string(path.as_str())
            .context(format_context!("Failed to read load order file {path}"))?;
        let mut settings: Settings = serde_json::from_str(content.as_str())
            .context(format_context!("Failed to parse load order file {path}"))?;

        let path = format!("{workspace_path}/{BIN_SETTINGS_FILE_NAME}");
        if std::path::Path::new(path.as_str()).exists() {
            let file = std::fs::File::open(path.as_str())
                .context(format_context!("Failed to open {path:?}"))?;
            let reader = std::io::BufReader::new(file);
            settings.bin_settings =
                bincode::decode_from_reader(reader, bincode::config::standard())
                    .context(format_context!("Failed to deserialize {path:?}"))?;
        }

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

    pub fn save(&self, workspace_path: &str) -> anyhow::Result<()> {

        std::fs::create_dir_all(workspace_path)
            .context(format_context!("Failed to create workspace directory {workspace_path}"))?;

        let path = format!("{workspace_path}/{SETTINGS_FILE_NAME}");
        let content = serde_json::to_string_pretty(&self)
            .context(format_context!("Failed to serialize load order"))?;
        std::fs::write(path.as_str(), content.as_str())
            .context(format_context!("Failed to save settings file {path}"))?;

        let path = format!("{workspace_path}/{BIN_SETTINGS_FILE_NAME}");
        let encoded = bincode::encode_to_vec(&self.bin_settings, bincode::config::standard())
            .context(format_context!("Failed to encode bin settings"))?;
        std::fs::write(path.as_str(), encoded)
            .context(format_context!("Failed to write to {path:?}"))?;
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
