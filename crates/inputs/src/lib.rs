use anyhow_source_location::format_error;
use bincode::{Decode, Encode};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

pub fn validate_input_globs(globs: &Option<HashSet<Arc<str>>>) -> anyhow::Result<()> {
    if let Some(globs) = globs.as_ref() {
        for glob in globs {
            if !glob.starts_with('+') && !glob.starts_with('-') {
                return Err(format_error!(
                    "Invalid glob: {glob:?}. Must begin with '+' (includes) or '-' (excludes)"
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Encode, Decode, Default)]
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
