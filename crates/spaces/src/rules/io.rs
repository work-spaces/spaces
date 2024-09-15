use anyhow::Context;
use anyhow_source_location::format_context;
use bincode::{Decode, Encode};
use std::collections::HashMap;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Input {
    timestamp: u128,
    digest: String,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Io {
    inputs: HashMap<String, Input>,
}

pub enum IsUpdated {
    No,
    Yes,
}

impl Io {
    pub fn new(io_path: &str) -> anyhow::Result<Io> {
        if std::path::Path::new(io_path).exists() {
            let contents =
                std::fs::read(io_path).context(format_context!("Failed to read file {io_path}"))?;
            let (io, _len): (Io, usize) =
                bincode::decode_from_slice(contents.as_slice(), bincode::config::standard())
                    .context(format_context!("Failed to decode file {io_path}"))?;
            return Ok(io);
        }

        Ok(Io {
            inputs: HashMap::new(),
        })
    }

    pub fn save(&self, io_path: &str) -> anyhow::Result<()> {
        let encoded = bincode::encode_to_vec(&self, bincode::config::standard())
            .context(format_context!("Failed to encode io"))?;
        std::fs::write(io_path, encoded).context(format_context!("Failed to write io"))?;
        Ok(())
    }

    fn get_digest(path: &str) -> anyhow::Result<String> {
        let contents =
            std::fs::read(path).context(format_context!("Failed to read file {path}"))?;
        Ok(blake3::hash(&contents).to_string())
    }

    pub fn update(&mut self, path: &str) -> anyhow::Result<IsUpdated> {
        let metadata = std::fs::metadata(&path)
            .context(format_context!("Failed to get metadata for {path}"))?;
        let timestamp = metadata.modified().unwrap().elapsed().unwrap().as_nanos();

        if let Some(input) = self.inputs.get_mut(path) {
            if input.timestamp == timestamp {
                return Ok(IsUpdated::No);
            } else {
                let digest = Self::get_digest(path)?;
                if digest == input.digest {
                    return Ok(IsUpdated::No);
                }

                input.digest = digest.to_string();
                return Ok(IsUpdated::Yes);
            }
        }

        let digest = Self::get_digest(path)?;
        self.inputs.insert(
            path.to_string(),
            Input {
                timestamp,
                digest: digest.to_string(),
            },
        );
        Ok(IsUpdated::Yes)
    }
}
