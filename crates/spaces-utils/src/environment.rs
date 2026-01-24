use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
enum GetVars {
    Checkout,
    Run,
}

pub fn calculate_digest(vars: &std::collections::HashMap<Arc<str>, Arc<str>>) -> String {
    let mut hasher = blake3::Hasher::new();
    let mut vars_list: Vec<String> = vars
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    vars_list.sort();
    for item in vars_list {
        hasher.update(item.as_bytes());
    }
    hasher.finalize().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vars: Option<HashMap<Arc<str>, Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_paths: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited_vars: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional_inherited_vars: Option<Vec<Arc<str>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_inherited_vars: Option<Vec<Arc<str>>>,
}

impl Environment {
    pub fn get_path(&self) -> Arc<str> {
        self.get_path_with_system_paths()
    }

    pub fn get_path_with_system_paths(&self) -> Arc<str> {
        let mut all_paths = Vec::new();
        if let Some(paths) = self.paths.as_ref() {
            all_paths.extend_from_slice(paths);
        }
        if let Some(system_paths) = &self.system_paths {
            all_paths.extend_from_slice(system_paths);
        }
        let path = all_paths.join(":");
        path.into()
    }

    fn get_inherited_vars(&self, get_vars: GetVars) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut env_vars: HashMap<Arc<str>, Arc<str>> = HashMap::new();
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
                    if let Ok(value) = std::env::var(key.as_ref()) {
                        // first try to re-inherit from calling env
                        env_vars.insert(key.clone(), value.into());
                    } else if let Some(value) = self.vars.as_ref().and_then(|e| e.get(key)) {
                        // second try to grab the value from the workspace env
                        env_vars.insert(key.clone(), value.clone());
                    } else {
                        return Err(format_error!(
                            "failed to get env var {key} from calling env to pass to workspace env"
                        ));
                    }
                }
            }
        }

        if get_vars == GetVars::Checkout {
            if let Some(optional_inherited) = &self.optional_inherited_vars {
                for key in optional_inherited {
                    if let Ok(value) = std::env::var(key.as_ref()) {
                        env_vars.insert(key.clone(), value.into());
                    }
                }
            }
        }

        if get_vars == GetVars::Run {
            if let Some(run_inherited) = &self.run_inherited_vars {
                for key in run_inherited {
                    let value =
                        std::env::var(key.as_ref())
                            .context(format_context!(
                    "failed to get env var {key} from calling env to pass to workspace env"))?;
                    env_vars.insert(key.clone(), value.into());
                }
            }
        }

        Ok(env_vars)
    }

    pub fn merge(&mut self, other: Environment) {
        if let Some(other_vars) = other.vars {
            self.vars.get_or_insert_default().extend(other_vars);
        }

        if let Some(other_paths) = other.paths {
            // add to paths if not already present
            for path in other_paths.iter() {
                if self
                    .paths
                    .as_ref()
                    .is_none_or(|paths| !paths.contains(path))
                {
                    self.paths.get_or_insert_default().push(path.clone());
                }
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

        if let Some(run_inherited_vars) = other.run_inherited_vars {
            if let Some(existing_run_inherited_vars) = self.run_inherited_vars.as_mut() {
                // extend if not already present
                for var in run_inherited_vars.iter() {
                    if !existing_run_inherited_vars.contains(var) {
                        existing_run_inherited_vars.push(var.clone());
                    }
                }
            } else {
                self.run_inherited_vars = Some(run_inherited_vars);
            }
        }

        if let Some(optional_inherited_vars) = other.optional_inherited_vars {
            if let Some(existing_optional_inherited_vars) = self.optional_inherited_vars.as_mut() {
                // extend if not already present
                for var in optional_inherited_vars.iter() {
                    if !existing_optional_inherited_vars.contains(var) {
                        existing_optional_inherited_vars.push(var.clone());
                    }
                }
            } else {
                self.optional_inherited_vars = Some(optional_inherited_vars);
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

        if let Some(vars) = self.vars.as_ref() {
            for (key, value) in vars.iter() {
                env_vars.insert(key.clone(), value.clone());
            }
        }

        env_vars.extend(
            self.get_inherited_vars(GetVars::Checkout)
                .context(format_context!("Failed to get inherited vars"))?,
        );

        env_vars.insert("PATH".into(), self.get_path());
        Ok(env_vars)
    }

    pub fn get_run_vars(&self) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
        let mut env_vars = HashMap::new();

        if let Some(vars) = self.vars.as_ref() {
            for (key, value) in vars.iter() {
                env_vars.insert(key.clone(), value.clone());
            }
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
