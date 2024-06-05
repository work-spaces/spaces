use serde::Serialize;

#[derive(Serialize)]
pub struct Context {
    pub bare_store_base: String,
    #[serde(skip)]
    pub async_runtime: tokio::runtime::Runtime,
    #[serde(skip)]
    pub printer: std::sync::RwLock<printer::Printer>,
    pub is_dry_run: bool,
}

impl Default for Context {
    fn default() -> Self {
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

        Context {
            bare_store_base,
            async_runtime,
            printer: std::sync::RwLock::new(printer::Printer::new_stdout()),
            is_dry_run: false,
        }
    }
}

impl Context {
    pub fn new() -> anyhow::Result<Self> {
        let result = Context::default();
        Ok(result)
    }

    pub fn get_bare_store_path(&self, name: &str) -> String {
        let mut result = self.bare_store_base.clone();
        result.push('/');
        result.push_str(name);
        result
    }

    pub fn update_printer(&mut self, is_dry_run: bool, level: Option<printer::Level>) {
        self.is_dry_run = is_dry_run;
        if let (Some(level), Ok(mut printer)) = (level, self.printer.write()) {
            printer.level = level;
        }
    }
}
