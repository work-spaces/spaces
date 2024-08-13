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
    pub current_directory: String,
    #[serde(skip)]
    pub async_runtime: tokio::runtime::Runtime,
    #[serde(skip)]
    pub printer: std::sync::RwLock<printer::Printer>,
    pub template_model: template::Model,
    pub active_repository: std::sync::Mutex<std::collections::HashSet<String>>,
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

        let log_directory = format!("{bare_store_base}/spaces_logs");

        {
            use anyhow::Context;
            std::fs::create_dir_all(log_directory.as_str())
                .context(format_error!("Failed to create log directory"))?;
        }

        let template_model = {
            use anyhow::Context;
            let mut model =
                template::Model::new(log_directory.as_str()).context(format_error!(""))?;
            model.spaces.store = bare_store_base;
            model
        };

        Ok(Context {
            async_runtime,
            printer: std::sync::RwLock::new(printer::Printer::new_stdout()),
            current_directory: current_directory_str.to_string(),
            active_repository: std::sync::Mutex::new(std::collections::HashSet::new()),
            template_model,
        })
    }

    pub fn get_bare_store_path(&self, name: &str) -> String {
        format!("{}/{name}", self.template_model.spaces.store)
    }

    pub fn get_log_directory(&self) -> &str {
        self.template_model.spaces.log_directory.as_str()
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
