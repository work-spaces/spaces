use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

use crate::info;

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
        let dest_path = get_destination_path(&self.destination).context(format_context!(
            "Failed to get destination path for asset file {}",
            &self.destination
        ))?;
        
        let new_value = if dest_path.exists() {
            let old_value = std::fs::read_to_string(&self.destination).context(format_context!(
                "Failed to read asset file {}",
                &self.destination
            ))?;
            let mut old_value: serde_json::Value = serde_json::from_str(&old_value).context(
                format_context!("Failed to parse asset file {}", &self.destination),
            )?;

            old_value.merge(&self.value);

            old_value
        } else {
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
pub struct AddAsset {
    pub destination: String,
    pub content: String,
}

impl AddAsset {
    pub fn execute(&self, _name: &str, _progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        save_asset(&self.destination, &self.content).context(format_context!("failed to add asset"))?;
        Ok(())
    }
}

fn get_destination_path(destination: &str) -> anyhow::Result<std::path::PathBuf> {
    let workspace_path = info::get_workspace_path()
        .context(format_context!("Failed to get workspace absolute path"))?;

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
