use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

const MANIFEST_FILE_NAME: &str = "store.spaces.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    last_used: u128,
    size: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Store {
    entries: HashMap<Arc<str>, Entry>,
}

impl Store {
    pub fn load(path_to_store: &str) -> anyhow::Result<Self> {
        let path = std::path::Path::new(path_to_store).join(MANIFEST_FILE_NAME);
        if path.exists() {
            let contents = std::fs::read_to_string(path.clone())
                .context(format_context!("Failed to read file: {}", path.display()))?;
            let store: Store = serde_json::from_str(&contents).context(format_context!(
                "Failed to deserialize JSON: {}",
                path.display()
            ))?;
            Ok(store)
        } else {
            Ok(Store {
                entries: HashMap::new(),
            })
        }
    }

    pub fn merge(&mut self, other: Store) {
        for (key, value) in other.entries {
            self.entries.insert(key, value);
        }
    }

    pub fn save(&self, path_to_store: &str) -> anyhow::Result<()> {
        let path = std::path::Path::new(path_to_store).join(MANIFEST_FILE_NAME);
        let contents = serde_json::to_string_pretty(self).context(format_context!(
            "Failed to serialize JSON: {}",
            path.display()
        ))?;
        std::fs::write(path.clone(), contents)
            .context(format_context!("Failed to write file: {}", path.display()))?;
        Ok(())
    }

    pub fn add_entry(&mut self, path: Arc<str>, size: u128) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let map_entry = self.entries.entry(path).or_insert_with(|| Entry {
            last_used: timestamp,
            size,
        });

        map_entry.last_used = timestamp;
        map_entry.size = size;
    }
}
