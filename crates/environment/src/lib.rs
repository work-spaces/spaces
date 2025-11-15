use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
enum GetVars {
    Checkout,
    Run,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub vars: HashMap<Arc<str>, Arc<str>>,
    pub paths: Vec<Arc<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_paths: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_vars: Option<Vec<Arc<str>>>,
}

impl Environment {
    pub fn get_path(&self) -> Arc<str> {
        self.get_path_with_system_paths()
    }

    pub fn get_path_with_system_paths(&self) -> Arc<str> {
        let mut path = self.paths.join(":");
        if let Some(system_paths) = &self.system_paths {
            if !system_paths.is_empty() {
                path.push(':');
                path.push_str(system_paths.join(":").as_str());
            }
        }
        path.into()
    }

    fn get_inherited_vars(&self, get_vars: GetVars) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut env_vars = HashMap::new();
        if let Some(inherited) = &self.inherited_vars {
            for key in inherited {
                // if key ends in ? it is optional
                if key.ends_with('?') {
                    if get_vars == GetVars::Checkout {
                        let trimmed_key = key.trim_end_matches('?');
                        if let Ok(value) = std::env::var(trimmed_key) {
                            env_vars.insert(trimmed_key.into(), value.into());
                        }
                    }
                } else if key.ends_with('!') {
                    if get_vars == GetVars::Run {
                        let trimmed_key = key.trim_end_matches('!');
                        let value = std::env::var(trimmed_key).context(format_context!(
                        "failed to get env var {trimmed_key} from calling env to pass to workspace env"))?;
                        env_vars.insert(trimmed_key.into(), value.into());
                    }
                } else if get_vars == GetVars::Checkout {
                    let value = std::env::var(key.as_ref()).context(format_context!(
                        "failed to get env var {key} from calling env to pass to workspace env"
                    ))?;
                    env_vars.insert(key.clone(), value.into());
                }
            }
        }
        Ok(env_vars)
    }

    pub fn merge(&mut self, other: Environment) {
        self.vars.extend(other.vars);

        // add to paths if not already present
        for path in other.paths.iter() {
            if !self.paths.contains(path) {
                self.paths.push(path.clone());
            }
        }

        if let Some(inherited_vars) = other.inherited_vars {
            if let Some(existing_inherited_vars) = self.inherited_vars.as_mut() {
                // extend if not already present
                for var in inherited_vars.iter() {
                    if !existing_inherited_vars.contains(var) {
                        existing_inherited_vars.push(var.clone());
                    }
                }
            } else {
                self.inherited_vars = Some(inherited_vars);
            }
        }

        if let Some(system_paths) = other.system_paths {
            if let Some(existing_system_paths) = self.system_paths.as_mut() {
                // extend if not already present
                for path in system_paths.iter() {
                    if !existing_system_paths.contains(path) {
                        existing_system_paths.push(path.clone());
                    }
                }
            } else {
                self.system_paths = Some(system_paths);
            }
        }
    }

    pub fn get_checkout_vars(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut env_vars = HashMap::new();

        env_vars.extend(
            self.get_inherited_vars(GetVars::Checkout)
                .context(format_context!("Failed to get inherited vars"))?,
        );

        for (key, value) in self.vars.iter() {
            env_vars.insert(key.clone(), value.clone());
        }
        env_vars.insert("PATH".into(), self.get_path());
        Ok(env_vars)
    }

    pub fn get_run_vars(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut env_vars = HashMap::new();

        for (key, value) in self.vars.iter() {
            env_vars.insert(key.clone(), value.clone());
        }

        env_vars.extend(
            self.get_inherited_vars(GetVars::Run)
                .context(format_context!("Failed to get inherited vars"))?,
        );

        Ok(env_vars)
    }

    pub fn create_shell_env(&self, path: std::path::PathBuf) -> anyhow::Result<()> {
        let mut content = String::new();

        let vars = self
            .get_checkout_vars()
            .context(format_context!("Failed to get vars"))?;

        for (key, value) in vars {
            let sanitized_key = key.trim_end_matches('?');
            let line = format!("export {sanitized_key}=\"{value}\"\n");
            content.push_str(&line);
        }

        std::fs::write(path.clone(), content)
            .context(format_context!("failed to write env file {path:?}"))?;

        Ok(())
    }
}
