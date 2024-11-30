use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

use crate::{info, workspace};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateEnv {
    pub vars: std::collections::HashMap<String, String>,
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_paths: Option<Vec<String>>
}

impl UpdateEnv {
    pub fn execute(
        &self,
        name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        progress.log(
            printer::Level::Debug,
            format!("Update env {name}: {:?}", &self).as_str(),
        );
        info::update_env(self.clone()).context(format_context!("failed to update env"))?;
        Ok(())
    }

    pub fn get_path(&self) -> String {
        self.paths.join(":")
    }

    pub fn get_path_with_system_paths(&self) -> String {
        let mut path = self.paths.join(":");
        if let Some(system_paths) = &self.system_paths {
            if !system_paths.is_empty() {
                path.push(':');
                path.push_str(system_paths.join(":").as_str());
            }
        } 
        path  
    }
}

pub fn finalize_env(env: &UpdateEnv) -> anyhow::Result<()> {
    let workspace = workspace::absolute_path();
    let workspace_path = std::path::Path::new(&workspace);
    let env_path = workspace_path.join("env");

    let mut content = String::new();

    for (key, value) in env.vars.iter() {
        let line = format!("export {}=\"{}\"\n", key, value);
        content.push_str(&line);
    }
    content.push('\n');
    content.push_str(format!("export PATH={}\n", env.get_path_with_system_paths()).as_str());

    std::fs::write(env_path.clone(), content)
        .context(format_context!("failed to write env file {env_path:?}"))?;

    Ok(())
}
