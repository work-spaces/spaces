use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Inputs {
    pub entries: HashMap<Arc<str>, Arc<str>>,
}

impl Inputs {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn is_changed(
        &self,
        rule_name: &str,
        digest: Arc<str>,
    ) -> anyhow::Result<Option<Arc<str>>> {
        let current_digest = match self.entries.get(rule_name) {
            Some(digest) => digest,
            None => return Ok(Some(digest)),
        };

        if *current_digest != digest {
            Ok(Some(digest))
        } else {
            Ok(None)
        }
    }

    pub fn save_digest(&mut self, rule: &str, digest: Arc<str>) {
        self.entries.insert(rule.into(), digest);
    }
}
