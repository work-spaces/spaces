use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

use crate::{workspace, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateEnv {
    pub vars: std::collections::HashMap<String, String>,
    pub paths: Vec<String>,
}

impl UpdateEnv {
    pub fn execute(&self, _name: &str, _progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        info::update_env(self.clone()).context(format_context!("failed to update env"))?;
        Ok(())
    }
}

pub fn finalize_env() -> anyhow::Result<()> {
    let env = info::get_env();
    let workspace = workspace::get_workspace_path()
        .context(format_context!("Internal error: workspace path not set"))?;
    let workspace_path = std::path::Path::new(&workspace);
    let env_path = workspace_path.join("env");

    let mut content = String::new();

    for (key, value) in env.vars.iter() {
        let line = format!("{}={}\n", key, value);
        content.push_str(&line);
    }
    content.push_str("\n");
    content.push_str(format!("PATH={}\n", env.paths.join(":")).as_str());

    std::fs::write(env_path.clone(), content)
        .context(format_context!("failed to write env file {env_path:?}"))?;

    Ok(())
}
