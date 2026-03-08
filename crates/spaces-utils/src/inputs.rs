use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct IsChanged {
    pub is_changed: bool,
    pub digest: Arc<str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Inputs {
    pub entries: HashMap<Arc<str>, Arc<str>>,
}

impl Inputs {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn is_changed(&self, rule_name: &str, digest: Arc<str>) -> IsChanged {
        match self.entries.get(rule_name) {
            Some(current_digest) => {
                if *current_digest != digest {
                    IsChanged {
                        is_changed: true,
                        digest,
                    }
                } else {
                    IsChanged {
                        is_changed: false,
                        digest,
                    }
                }
            }
            None => IsChanged {
                is_changed: true,
                digest,
            },
        }
    }

    pub fn save_digest(&mut self, rule: &str, digest: Arc<str>) {
        self.entries.insert(rule.into(), digest);
    }
}
