use std::collections::HashMap;
use std::sync::Arc;

pub struct ManifestEntry {
    pub url: Arc<str>,
    pub sha256: Arc<str>,
}

pub struct Manifest {
    pub releases: HashMap<Arc<str>, ManifestEntry>,
}

impl Manifest {
    pub fn new_from_url(
        url: Arc<str>,
        headers: HashMap<Arc<str>, Arc<str>>,
    ) -> anyhow::Result<Self> {
        Err(anyhow::anyhow!("Not implemented"))
    }

    pub fn new() -> Self {
        Self {
            releases: HashMap::new(),
        }
    }
}
