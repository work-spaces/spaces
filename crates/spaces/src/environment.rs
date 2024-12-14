use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub vars: HashMap<String, String>,
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_vars: Option<Vec<String>>,
}

impl Environment {
    pub fn get_path(&self) -> String {
        self.get_path_with_system_paths()
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

    pub fn get_vars(&self) -> anyhow::Result<HashMap<String, String>> {
        let mut env_vars = HashMap::new();

        if let Some(inherited) = &self.inherited_vars {
            for key in inherited {
                let value = std::env::var(key).context(format_context!(
                    "failed to get env var {key} from calling env to pass to workspace env"
                ))?;
                env_vars.insert(key.clone(), value);
            }
        }

        for (key, value) in self.vars.iter() {
            env_vars.insert(key.clone(), value.clone());
        }
        env_vars.insert("PATH".to_string(), self.get_path());
        Ok(env_vars)
    }

    pub fn create_shell_env(&self, path: std::path::PathBuf) -> anyhow::Result<()> {
        let mut content = String::new();

        let vars = self
            .get_vars()
            .context(format_context!("Failed to get vars"))?;

        for (key, value) in vars {
            let line = format!("export {}=\"{}\"\n", key, value);
            content.push_str(&line);
        }

        std::fs::write(path.clone(), content)
            .context(format_context!("failed to write env file {path:?}"))?;

        Ok(())
    }
}
