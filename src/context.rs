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

pub use anyhow_error;
pub use format_error_context;

#[derive(Serialize)]
pub struct Context {
    pub bare_store_base: String,
    pub current_directory: String,
    pub spaces_sysroot: Option<String>,
    #[serde(skip)]
    pub async_runtime: tokio::runtime::Runtime,
    #[serde(skip)]
    pub printer: std::sync::RwLock<printer::Printer>,
    pub is_dry_run: bool,
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

        Ok(Context {
            bare_store_base,
            async_runtime,
            spaces_sysroot: None,
            printer: std::sync::RwLock::new(printer::Printer::new_stdout()),
            is_dry_run: false,
            current_directory: current_directory_str.to_string(),
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
}
