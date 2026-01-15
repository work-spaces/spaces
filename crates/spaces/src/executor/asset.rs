use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utils::{http_archive, logger, ws};

use crate::workspace;

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
#[serde(deny_unknown_fields)]
pub struct UpdateAsset {
    pub destination: Arc<str>,
    pub format: AssetFormat,
    pub value: serde_json::Value,
}

fn parse_value(format: AssetFormat, content: &str) -> anyhow::Result<serde_json::Value> {
    match format {
        AssetFormat::Json => serde_json::from_str(content)
            .context(format_context!("Failed to parse asset file as JSON",)),
        AssetFormat::Toml => {
            toml::from_str(content).context(format_context!("Failed to parse asset file as TOML",))
        }
        AssetFormat::Yaml => serde_yaml::from_str(content)
            .context(format_context!("Failed to parse asset file as YAML",)),
    }
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

impl UpdateAsset {
    pub fn execute(
        &self,
        mut progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        use json_value_merge::Merge;

        let mut logger = logger::Logger::new_progress(&mut progress, name.into());
        let mut workspace_write_lock = workspace.write();
        let _ = workspace_write_lock
            .settings
            .checkout
            .updated_assets
            .insert(self.destination.clone());
        let workspace_path = workspace_write_lock.get_absolute_path();

        let dest_path = get_destination_path(workspace_path.clone(), &self.destination).context(
            format_context!(
                "Failed to get destination path for asset file {}",
                &self.destination
            ),
        )?;

        logger.message(format!("update asset {}", self.destination).as_str());

        let new_value = if workspace_write_lock
            .updated_assets
            .contains(&self.destination)
        {
            logger.debug(format!("load existing value {}", self.destination).as_str());

            let old_value = std::fs::read_to_string(dest_path.clone()).context(format_context!(
                "Failed to read asset file {}",
                dest_path.display()
            ))?;

            logger
                .trace(format!("Parsing asset file `{}` as {:?}", old_value, self.format).as_str());
            let mut old_value = parse_value(self.format, &old_value).context(format_context!(
                "Failed to parse asset file {}",
                &self.destination
            ))?;

            old_value.merge(&self.value);

            old_value
        } else {
            logger.debug(format!("Add new value to {}", self.destination).as_str());
            workspace_write_lock
                .updated_assets
                .insert(self.destination.clone());
            self.value.clone()
        };

        let content = format_value(self.format, &new_value).context(format_context!(
            "Failed to format asset file {}",
            &self.destination
        ))?;

        save_asset(workspace_path, &self.destination, &content)
            .context(format_context!("failed to add asset {}", self.destination))?;

        logger.debug(
            format!(
                "Updating asset {} with hash to workspace settings",
                self.destination
            )
            .as_str(),
        );

        workspace_write_lock
            .settings
            .json
            .assets
            .insert(self.destination.clone(), ws::Asset::new_contents(&content));

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddWhichAsset {
    pub which: String,
    pub destination: String,
}

impl AddWhichAsset {
    pub fn execute(
        &self,
        _progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        _name: &str,
    ) -> anyhow::Result<()> {
        let path = which::which(self.which.as_str()).context(format_context!(
            "Failed to find {} on using `which`. This is required for this workspace",
            self.which
        ))?;

        let mut workspace_write_lock = workspace.write();
        let _ = workspace_write_lock
            .settings
            .checkout
            .links
            .insert(self.destination.clone().into());

        // create the hard link to sysroot
        let workspace = workspace_write_lock.get_absolute_path();
        let destination = format!("{}/{}", workspace, self.destination);

        let source = path.to_string_lossy().to_string();

        http_archive::HttpArchive::create_link(
            destination.clone(),
            source,
            http_archive::MakeReadOnly::No,
            None,
            http_archive::ArchiveLink::Hard,
        )
        .context(format_context!(
            "Failed to create hard link from {} to {}",
            path.display(),
            destination
        ))?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddHardLink {
    pub source: String,
    pub destination: String,
}

impl AddHardLink {
    pub fn execute(
        &self,
        _progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        _name: &str,
    ) -> anyhow::Result<()> {
        // create the hard link to sysroot
        let mut workspace_write_lock = workspace.write();
        let _ = workspace_write_lock
            .settings
            .checkout
            .links
            .insert(self.destination.clone().into());

        let workspace = workspace_write_lock.get_absolute_path();
        let destination = format!("{}/{}", workspace, self.destination);
        let source = self.source.clone();

        http_archive::HttpArchive::create_link(
            destination.clone(),
            source.clone(),
            http_archive::MakeReadOnly::No,
            None,
            http_archive::ArchiveLink::Hard,
        )
        .context(format_context!(
            "Failed to create hard link from {} to {}",
            source,
            destination
        ))?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddAsset {
    pub destination: Arc<str>,
    pub content: Arc<str>,
}

impl AddAsset {
    pub fn execute(
        &self,
        mut progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let mut logger = logger::Logger::new_progress(&mut progress, name.into());
        let mut workspace_write_lock = workspace.write();
        workspace_write_lock.add_checkout_asset(self.destination.clone(), self.content.clone());

        let previous_checkout = workspace_write_lock.settings.clone_existing_checkout();
        // does this already exist and has it been modified
        if previous_checkout.is_asset_modified(self.destination.clone()) {
            logger
                .warning(format!("Asset {} is modified. Not updating", self.destination).as_str());
            return Ok(());
        }

        let workspace_path = workspace_write_lock.get_absolute_path();
        save_asset(workspace_path, &self.destination, &self.content)
            .context(format_context!("failed to add asset"))?;

        logger.debug(
            format!(
                "Adding asset {} with hash to workspace settings",
                self.destination
            )
            .as_str(),
        );

        workspace_write_lock.settings.json.assets.insert(
            self.destination.clone(),
            ws::Asset::new_contents(&self.content),
        );

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddSoftLink {
    pub source: String,
    pub destination: String,
}

impl AddSoftLink {
    pub fn execute(
        &self,
        _progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        _name: &str,
    ) -> anyhow::Result<()> {
        // create the hard link to sysroot
        let mut workspace_write_lock = workspace.write();
        workspace_write_lock
            .settings
            .checkout
            .links
            .insert(self.destination.clone().into());

        let workspace = workspace_write_lock.get_absolute_path();
        let destination = format!("{}/{}", workspace, self.destination);
        let source = self.source.clone();

        let desination_path = std::path::Path::new(&destination);
        if let Some(parent) = desination_path.parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create parent directories for soft link {}",
                destination
            ))?;
        }

        if desination_path.exists() {
            std::fs::remove_file(destination.clone()).context(format_context!(
                "Failed to remove existing symlink {}",
                destination
            ))?;
        }

        let source_path = std::path::Path::new(&source);
        if source_path.is_dir() {
            symlink::symlink_dir(source.clone(), destination.clone()).context(format_context!(
                "Failed to create soft link from {} to {}",
                source,
                destination
            ))?;
        } else {
            symlink::symlink_file(source.clone(), destination.clone()).context(format_context!(
                "Failed to create soft link from {} to {}",
                source,
                destination
            ))?;
        }

        Ok(())
    }
}

fn get_destination_path(
    workspace_path: Arc<str>,
    destination: &str,
) -> anyhow::Result<std::path::PathBuf> {
    Ok(std::path::Path::new(workspace_path.as_ref()).join(destination))
}

fn save_asset(workspace_path: Arc<str>, destination: &str, content: &str) -> anyhow::Result<()> {
    let output_path = get_destination_path(workspace_path, destination)
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
