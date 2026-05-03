use std::collections::HashMap;
use std::sync::Arc;

pub struct Config {
    manifest_url: Arc<str>,
    headers: HashMap<Arc<str>, Arc<str>>,
    env: HashMap<Arc<str>, Arc<str>>,
}

impl Config {
    pub fn new_from_toml() -> anyhow::Result<Self> {
        Err(anyhow::anyhow!("not implemented"))
    }
}
