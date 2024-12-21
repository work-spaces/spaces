use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
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

#[derive(Debug, Clone, Encode, Decode)]
pub struct Inputs {
    inputs: HashMap<Arc<str>, Arc<str>>,
}

impl Inputs {
    pub fn new(io_path: &str) -> Inputs {
        match Self::load(io_path) {
            Ok(inputs) => inputs,
            Err(_) => Inputs {
                inputs: HashMap::new(),
            },
        }
    }

    pub fn save(&self, io_path: &str) -> anyhow::Result<()> {
        let encoded = bincode::encode_to_vec(self, bincode::config::standard())
            .context(format_context!("Failed to encode io"))?;
        std::fs::write(io_path, encoded).context(format_context!("Failed to write io"))?;
        Ok(())
    }

    pub fn load(path: &str) -> anyhow::Result<Inputs> {
        let file = std::fs::File::open(path).context(format_context!("Failed to open {path:?}"))?;
        let reader = std::io::BufReader::new(file);
        let changes: Inputs = bincode::decode_from_reader(reader, bincode::config::standard())
            .context(format_context!("Failed to deserialize {path:?}"))?;
        Ok(changes)
    }

    pub fn is_changed(
        &self,
        rule_name: &str,
        digest: Arc<str>,
    ) -> anyhow::Result<Option<Arc<str>>> {

        let current_digest = match self.inputs.get(rule_name) {
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
        self.inputs.insert(rule.into(), digest);
    }
}
