use serde::Serialize;



#[derive(Debug, Serialize)]
pub struct Config {
    pub bare_store_base: String,
    #[serde(skip)]
    pub async_runtime: tokio::runtime::Runtime,
}

impl Default for Config {
    fn default() -> Self {
        let mut home_directory = home::home_dir().expect("No home directory found");
        home_directory.push(".spaces");
        home_directory.push("store");
        let bare_store_base = home_directory.to_str().expect(format!("Home directory is not a valid string {:?}", home_directory).as_str()).to_string();

        let async_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Internal Error: Failed to create async runtime");

        Config {
            bare_store_base,
            async_runtime,
        }
    }
}

impl Config {
    pub fn new() -> anyhow::Result<Self> {
        let result = Config::default();
        Ok(result)
    }


    pub fn get_bare_store_path(&self, name: &str) -> String {
        let mut result = self.bare_store_base.clone();
        result.push_str("/");
        result.push_str(name);
        result
    }

}

pub type Printer =  printer::Printer<Config>;