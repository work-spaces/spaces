use anyhow_source_location::format_error;
use serde::Serialize;

use crate::template;

pub fn get_workspace_name(full_path: &str) -> anyhow::Result<String> {
    let space_name = std::path::Path::new(full_path)
        .file_name()
        .ok_or(format_error!(
            "{full_path} directory is not a space workspace"
        ))?
        .to_str()
        .ok_or(format_error!(
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
    pub template_model: template::Model,
    pub active_repository: std::sync::Mutex<std::collections::HashSet<String>>
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

        let template_model = {
            use anyhow::Context;
            template::Model::new().context(format_error!(""))?
        };

        Ok(Context {
            bare_store_base,
            async_runtime,
            printer: std::sync::RwLock::new(printer::Printer::new_stdout()),
            current_directory: current_directory_str.to_string(),
            active_repository: std::sync::Mutex::new(std::collections::HashSet::new()),
            template_model,
        })
    }

    pub fn get_bare_store_path(&self, name: &str) -> String {
        let mut result = self.bare_store_base.clone();
        result.push('/');
        result.push_str(name);
        result
    }

    #[allow(dead_code)]
    fn get_spaces_path() -> Option<String> {
        let path = which::which("spaces").ok()?;
        let parent = path.parent()?;
        Some(parent.to_string_lossy().to_string())
    }
}

pub struct ExecutionContext {
    pub printer: printer::Printer,
    pub context: Context,
}

impl ExecutionContext {
    pub fn new() -> anyhow::Result<Self> {
        let context = Context::new()?;
        let printer = printer::Printer::new_stdout();
        Ok(ExecutionContext { printer, context })
    }
}
