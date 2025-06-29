use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub const WORKFLOW_TOML_NAME: &str = "workflows.spaces.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Devflow {
    pub checkout_scripts: Vec<Arc<str>>,
    pub new_branches: Vec<Arc<str>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Flow {
    Flow(Vec<Arc<str>>),
    DevFlow(Devflow),
}

pub type Workflows = HashMap<Arc<str>, Flow>;

pub fn try_workflows(directory: &str, key_name: &str) -> anyhow::Result<Option<Devflow>> {
    let workflows_json_path = format!("{directory}/{WORKFLOW_TOML_NAME}");
    let mut result = None;
    if std::path::Path::new(workflows_json_path.as_str()).exists() {
        let workflows_toml: Workflows = toml::from_str(
            std::fs::read_to_string(workflows_json_path.as_str())
                .context(format_context!("Failed to read workflows json"))?
                .as_str(),
        )
        .context(format_context!("Failed to parse workflows toml"))?;

        result = if let Some(flow) = workflows_toml.get(key_name) {
            match flow {
                Flow::Flow(scripts) => Some(Devflow {
                    checkout_scripts: scripts.clone(),
                    new_branches: vec![],
                }),
                Flow::DevFlow(devflow) => Some(devflow.clone()),
            }
        } else {
            None
        };
    }

    Ok(result)
}
