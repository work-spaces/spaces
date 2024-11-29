use crate::workspace;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use bincode::{Decode, Encode};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::RwLock;

pub fn validate_input_globs(globs: &Option<HashSet<String>>) -> anyhow::Result<()> {
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

pub fn is_rule_inputs_changed(
    progress: &mut printer::MultiProgressBar,
    rule_name: &str,
    seed: &str,
    inputs: &HashSet<String>,
) -> anyhow::Result<Option<String>> {
    let state = get_state().read().unwrap();
    state.inputs.is_changed(progress, rule_name, seed, inputs)
}

pub fn update_rule_digest(rule: &str, digest: String) {
    let mut state = get_state().write().unwrap();
    state.inputs.save_digest(rule, digest);
}

pub fn save() -> anyhow::Result<()> {
    let state = get_state().read().unwrap();
    let inputs_path = workspace::get_inputs_path();
    state.inputs.save(inputs_path)
}

#[derive(Debug, Clone, Encode, Decode)]
struct Inputs {
    inputs: HashMap<String, String>,
}

impl Inputs {
    fn new(io_path: &str) -> Inputs {
        match Self::load(io_path) {
            Ok(inputs) => inputs,
            Err(_) => Inputs {
                inputs: HashMap::new(),
            },
        }
    }

    fn save(&self, io_path: &str) -> anyhow::Result<()> {
        let encoded = bincode::encode_to_vec(self, bincode::config::standard())
            .context(format_context!("Failed to encode io"))?;
        std::fs::write(io_path, encoded).context(format_context!("Failed to write io"))?;
        Ok(())
    }

    fn load(path: &str) -> anyhow::Result<Inputs> {
        let file = std::fs::File::open(path).context(format_context!("Failed to open {path:?}"))?;
        let reader = std::io::BufReader::new(file);
        let changes: Inputs = bincode::decode_from_reader(reader, bincode::config::standard())
            .context(format_context!("Failed to deserialize {path:?}"))?;
        Ok(changes)
    }

    fn is_changed(
        &self,
        progress: &mut printer::MultiProgressBar,
        rule_name: &str,
        seed: &str, 
        inputs: &HashSet<String>,
    ) -> anyhow::Result<Option<String>> {
        let digest = workspace::get_rule_inputs_digest(progress, seed, inputs)
            .context(format_context!("Failed to get digest for rule {rule_name}"))?;

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

    fn save_digest(&mut self, rule: &str, digest: String) {
        self.inputs.insert(rule.to_string(), digest);
    }
}

struct State {
    pub inputs: Inputs,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    let inputs_path = workspace::get_inputs_path();

    STATE.set(RwLock::new(State {
        inputs: Inputs::new(inputs_path),
    }));
    STATE.get()
}
