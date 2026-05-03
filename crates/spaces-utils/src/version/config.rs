use super::manifest::Manifest;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub const VERSION_CONFIG_FILE_NAME: &str = "version.spaces.toml";
const VERSION_CONFIG_PATH_ENV_NAME: &str = "SPACES_ENV_VERSION_CONFIG_PATH";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub manifest_url: Arc<str>,
    #[serde(default)]
    pub headers: HashMap<Arc<str>, Arc<str>>,
    #[serde(default)]
    pub env: HashMap<Arc<str>, Arc<str>>,
}

#[derive(Clone, Debug)]
pub struct ResolvedConfig {
    manifest_url: Arc<str>,
    headers: HashMap<Arc<str>, Arc<str>>,
}

impl ResolvedConfig {
    pub fn manifest_url(&self) -> &Arc<str> {
        &self.manifest_url
    }

    pub fn headers(&self) -> &HashMap<Arc<str>, Arc<str>> {
        &self.headers
    }
}

impl Config {
    pub fn new_from_toml(path_to_store: &std::path::Path) -> anyhow::Result<Option<Self>> {
        let explicit_config_path = std::env::var(VERSION_CONFIG_PATH_ENV_NAME).ok();
        let config_path = if let Some(path) = explicit_config_path.as_ref() {
            let trimmed = path.trim().to_string();
            if trimmed.is_empty() {
                return Err(format_error!(
                    "{} is set but empty",
                    VERSION_CONFIG_PATH_ENV_NAME
                ));
            }
            std::path::PathBuf::from(trimmed)
        } else {
            path_to_store.join(VERSION_CONFIG_FILE_NAME)
        };

        if !config_path.exists() {
            if explicit_config_path.is_some() {
                return Err(format_error!(
                    "{} points to a missing file: {}",
                    VERSION_CONFIG_PATH_ENV_NAME,
                    config_path.display()
                ));
            }
            return Ok(None);
        }

        let contents = std::fs::read_to_string(&config_path).context(format_context!(
            "Failed to read version configuration from {}",
            config_path.display()
        ))?;

        let config: Config = toml::from_str(&contents).context(format_context!(
            "Failed to parse version configuration from {}",
            config_path.display()
        ))?;

        Ok(Some(config))
    }

    pub fn resolve(&self) -> anyhow::Result<ResolvedConfig> {
        if self.manifest_url.trim().is_empty() {
            return Err(format_error!("manifest_url cannot be empty"));
        }

        let mut token_values: Vec<(Arc<str>, String)> = self
            .env
            .iter()
            .map(|(token, env_name)| {
                let value = std::env::var(env_name.as_ref()).map_err(|_| {
                    format_error!(
                        "Environment variable '{}' (for token '{}') is not set",
                        env_name,
                        token
                    )
                })?;
                Ok::<(Arc<str>, String), anyhow::Error>((token.clone(), value))
            })
            .collect::<anyhow::Result<Vec<(Arc<str>, String)>>>()?;

        token_values.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let manifest_url: Arc<str> =
            Self::replace_tokens(self.manifest_url.as_ref(), &token_values).into();

        let mut headers = HashMap::new();
        for (key, value) in &self.headers {
            let resolved_key: Arc<str> = Self::replace_tokens(key.as_ref(), &token_values).into();
            let resolved_value: Arc<str> =
                Self::replace_tokens(value.as_ref(), &token_values).into();
            headers.insert(resolved_key, resolved_value);
        }

        Ok(ResolvedConfig {
            manifest_url,
            headers,
        })
    }

    pub fn populate_manifest(
        &self,
        manifest: &mut Manifest,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<()> {
        let resolved = self
            .resolve()
            .context(format_context!("Failed to resolve version config"))?;

        manifest
            .populate_from_url(
                progress_bar,
                resolved.manifest_url().as_ref(),
                resolved.headers(),
            )
            .context(format_context!(
                "Failed to populate manifest from {}",
                resolved.manifest_url()
            ))
    }

    fn replace_tokens(input: &str, token_values: &[(Arc<str>, String)]) -> String {
        let mut result = input.to_string();
        for (token, value) in token_values {
            result = result.replace(token.as_ref(), value.as_str());
        }
        result
    }
}
