use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use bincode::{Decode, Encode};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Input {
    timestamp: u128,
    digest: String,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Io {
    inputs: HashMap<String, Input>,
}

#[derive(Clone, Copy, PartialEq)]
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

    pub fn update_glob(&mut self, progress: &mut printer::MultiProgressBar, glob: &[&str]) -> anyhow::Result<IsUpdated> {
        let walkdir = globwalk::GlobWalkerBuilder::from_patterns(".", glob)
            .build()
            .context(format_context!("Failed to build walker for {glob:?}"))?;

        let items: Vec<_> = walkdir.filter_map(|e| if e.is_ok() { Some(e.unwrap()) } else { None }).collect();

        progress.set_total(items.len() as u64);

        let mut is_updated = IsUpdated::No;
        progress.set_message("hashing inputs");

        for entry in  items {
            progress.set_message(entry.path().to_string_lossy().to_string().as_str());
            if entry.file_type().is_file() {
                let path = entry.path().to_string_lossy().to_string();
                if self
                    .update(path.as_str())
                    .context(format_context!("Failed to hash {path}"))?
                    == IsUpdated::Yes
                {
                    is_updated = IsUpdated::Yes;
                }
            }
            progress.increment(1);
        }

        Ok(is_updated)
    }

    fn update(&mut self, path: &str) -> anyhow::Result<IsUpdated> {
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
                input.timestamp = timestamp;
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

pub struct State {
    pub io: Io,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

pub fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    let io_path = workspace::get_io_path();

    STATE.set(RwLock::new(State {
        io: Io::new(io_path)
            .expect(format_context!("Internal Error: Failed to initialize IO: {io_path}").as_str()),
    }));
    STATE.get()
}
