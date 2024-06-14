use serde::Serialize;

#[macro_export]
macro_rules! format_error_context {
    ($($arg:tt)*) => {{
        let res = format!($($arg)*);
        format!("[{}:{}] {}", file!(), line!(), res)
    }};
}

#[macro_export]
macro_rules! anyhow_error {
    ($($arg:tt)*) => {{
        let res = format!($($arg)*);
        anyhow::anyhow!("[{}:{}] {}", file!(), line!(), res)
    }};
}

pub const SPACES_OVERLAY: &str = "{SPACES_OVERLAY}";
pub const SPACE: &str = "{SPACE}";
pub const USER: &str = "{USER}";
pub const UNIQUE: &str = "{UNIQUE}";
pub const SPACES_SYSROOT: &str = "{SPACES_SYSROOT}";
pub const SPACES_PLATFORM: &str = "{SPACES_PLATFORM}";
pub const SPACES_PATH: &str = "{SPACES_PATH}";
pub const SPACES_BRANCH: &str = "{SPACES_BRANCH}";
pub const SPACES_TOML: &str = "{SPACES_TOML:";

pub use anyhow_error;
pub use format_error_context;

use crate::manifest;

pub fn get_workspace_name(full_path: &str) -> anyhow::Result<String> {
    let space_name = std::path::Path::new(full_path)
        .file_name()
        .ok_or(anyhow_error!(
            "{full_path} directory is not a space workspace"
        ))?
        .to_str()
        .ok_or(anyhow_error!(
            "{full_path} directory is not a space workspace"
        ))?;
    Ok(space_name.to_string())
}

#[derive(Serialize)]
pub struct Substitution {
    pub template_value: String,
    pub replacement_value: String,
}

#[derive(Serialize)]
pub struct Context {
    pub bare_store_base: String,
    pub current_directory: String,
    #[serde(skip)]
    pub async_runtime: tokio::runtime::Runtime,
    #[serde(skip)]
    pub printer: std::sync::RwLock<printer::Printer>,
    pub substitutions: std::collections::HashMap<&'static str, (Option<String>, &'static str)>,
}

impl Context {
    pub fn new() -> anyhow::Result<Self> {
        let mut home_directory = home::home_dir().expect("No home directory found");
        home_directory.push(".spaces");
        home_directory.push("store");
        let bare_store_base = home_directory
            .to_str()
            .unwrap_or_else(|| {
                panic!(
                    "Internal Error: Home directory is not a valid string {:?}",
                    home_directory
                )
            })
            .to_string();

        let async_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Internal Error: Failed to create async runtime");

        let current_directory = std::env::current_dir()?;
        let current_directory_str = current_directory.to_str().ok_or(anyhow::anyhow!(
            "Internal Error: Path is not a valid string"
        ))?;

        let user = std::env::var("USER").ok();

        let unique_timestamp = format!("{}", std::time::Instant::now().elapsed().as_nanos());
        let unique_sha256 = sha256::digest(unique_timestamp.as_bytes());
        let unique = unique_sha256.as_str()[0..8].to_string();

        let substitutions = maplit::hashmap! {
            SPACE => (None, "Name of the space being created or sync'd"),
            SPACES_BRANCH => (None, "The name of the development branch for repositories in the space"),
            SPACES_SYSROOT => (None, "The path to the space's sysroot directory"),
            SPACES_OVERLAY => (None, "The name of the repository or dependency containing the substitution value"),
            SPACES_PATH => (Self::get_spaces_path(), "Parent directory of the spaces binary found in the PATH"),
            SPACES_PLATFORM => (manifest::Platform::get_platform().map(|e| e.to_string()), "The platform of the current system"),
            UNIQUE => (Some(unique), "A unique 8-character identifier generated from a timestamp"),
            USER => (user, "Value of USER in env"),
        };

        Ok(Context {
            bare_store_base,
            async_runtime,
            printer: std::sync::RwLock::new(printer::Printer::new_stdout()),
            current_directory: current_directory_str.to_string(),
            substitutions,
        })
    }

    pub fn get_bare_store_path(&self, name: &str) -> String {
        let mut result = self.bare_store_base.clone();
        result.push('/');
        result.push_str(name);
        result
    }

    pub fn update_printer(&mut self, level: Option<printer::Level>) {
        if let (Some(level), Ok(mut printer)) = (level, self.printer.write()) {
            printer.level = level;
        }
    }

    pub fn update_substitution(&mut self, key: &str, next_value: &str) -> anyhow::Result<()> {
        if let Some((current_value, _)) = self.substitutions.get_mut(key) {
            *current_value = Some(next_value.to_owned());
        } else {
            return Err(anyhow_error!("Invalid substitution key: {}", key));
        }
        Ok(())
    }

    pub fn get_sysroot(&self) -> anyhow::Result<&String> {
        let (value, _description) = self.substitutions.get(SPACES_SYSROOT).ok_or(anyhow_error!(
            "Internal Error: SPACES_SYSROOT not found in map"
        ))?;
        value
            .as_ref()
            .ok_or(anyhow_error!("Internal Error: SPACES_SYSROOT not set"))
    }

    fn get_spaces_path() -> Option<String> {
        let path = which::which("spaces").ok()?;
        let parent = path.parent()?;
        Some(parent.to_string_lossy().to_string())
    }
}
