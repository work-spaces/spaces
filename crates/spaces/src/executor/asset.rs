use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::RwLock;

use crate::workspace;

struct State {
    updated_assets: HashSet<String>,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    STATE.set(RwLock::new(State {
        updated_assets: HashSet::new(),
    }));
    STATE.get()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AssetFormat {
    #[serde(rename = "json")]
    Json,
    #[serde(rename = "toml")]
    Toml,
    #[serde(rename = "yaml")]
    Yaml,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAsset {
    pub destination: String,
    pub format: AssetFormat,
    pub value: serde_json::Value,
}

impl UpdateAsset {
    pub fn execute(&self, _name: &str, _progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        use json_value_merge::Merge;

        // hold the mutex to ensure exclusive access to the output file
        let mut state = get_state().write().unwrap();

        let dest_path = get_destination_path(&self.destination).context(format_context!(
            "Failed to get destination path for asset file {}",
            &self.destination
        ))?;

        let new_value = if state.updated_assets.get(&self.destination).is_some() {
            let old_value = std::fs::read_to_string(dest_path.clone()).context(format_context!(
                "Failed to read asset file {}",
                dest_path.display()
            ))?;
            let mut old_value: serde_json::Value = serde_json::from_str(&old_value).context(
                format_context!("Failed to parse asset file {}", &self.destination),
            )?;

            old_value.merge(&self.value);

            old_value
        } else {
            state.updated_assets.insert(self.destination.clone());
            self.value.clone()
        };

        let content = format_value(self.format, &new_value).context(format_context!(
            "Failed to format asset file {}",
            &self.destination
        ))?;

        save_asset(&self.destination, &content).context(format_context!("failed to add asset"))?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddWhichAsset {
    pub which: String,
    pub destination: String,
}

impl AddWhichAsset {
    pub fn execute(&self, _name: &str, _progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        let path = which::which(self.which.as_str()).context(format_context!(
            "Failed to find {} on using `which`. This is required for this workspace",
            self.which
        ))?;

        // create the hard link to sysroot
        let workspace = workspace::absolute_path();
        let destination = format!("{}/{}", workspace, self.destination);

        let source = path.to_string_lossy().to_string();

        http_archive::HttpArchive::create_hard_link(destination.clone(), source).context(format_context!(
            "Failed to create hard link from {} to {}",
            path.display(),
            destination
        ))?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAsset {
    pub destination: String,
    pub content: String,
}

impl AddAsset {
    pub fn execute(&self, _name: &str, _progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        save_asset(&self.destination, &self.content)
            .context(format_context!("failed to add asset"))?;
        Ok(())
    }
}

fn get_destination_path(destination: &str) -> anyhow::Result<std::path::PathBuf> {
    let workspace_path = workspace::absolute_path();
    Ok(std::path::Path::new(&workspace_path).join(destination))
}

fn save_asset(destination: &str, content: &str) -> anyhow::Result<()> {
    let output_path = get_destination_path(destination)
        .context(format_context!("Failed to get destaiont for {destination}"))?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).context(format_context!(
            "Failed to create parent directories for asset file {}",
            output_path.to_string_lossy()
        ))?;
    }
    std::fs::write(output_path.clone(), content).context(format_context!(
        "Failed to write asset file {}",
        output_path.to_string_lossy()
    ))?;

    Ok(())
}

fn format_value(format: AssetFormat, value: &serde_json::Value) -> anyhow::Result<String> {
    match format {
        AssetFormat::Json => serde_json::to_string_pretty(value)
            .context(format_context!("Failed to serialize asset file as JSON",)),
        AssetFormat::Toml => toml::to_string_pretty(value)
            .context(format_context!("Failed to serialize asset file as TOML",)),
        AssetFormat::Yaml => serde_yaml::to_string(value)
            .context(format_context!("Failed to serialize asset file as YAML",)),
    }
}
