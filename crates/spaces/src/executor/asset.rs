use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utils::{copy, labels, logger, ws};

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
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        use json_value_merge::Merge;
        let console = progress.console.clone();
        let logger = logger::Logger::new(console.clone(), name.into());

        // the workspace write lock needs to be held for the duration of the update
        // to prevent concurrent updates to the same asset
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

        logger.debug(format!("update asset {}", self.destination).as_str());

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
        _progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        _name: &str,
    ) -> anyhow::Result<()> {
        let path = which::which(self.which.as_str()).with_context(|| {
            format_context!(
                "Failed to find {} on using `which`. This is required for this workspace",
                self.which
            )
        })?;

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

        copy::create_link(
            destination.clone(),
            source,
            copy::MakeReadOnly::No,
            None,
            copy::LinkType::Hard,
        )
        .with_context(|| {
            format_context!(
                "Failed to create hard link from {} to {}",
                path.display(),
                destination
            )
        })?;

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
        _progress: &mut console::Progress,
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

        // just need this for the deprecation warning
        let _ = labels::sanitize_path(self.source.clone().into(), None);
        let _ = labels::sanitize_path(self.destination.clone().into(), None);

        copy::create_link(
            destination.clone(),
            source.clone(),
            copy::MakeReadOnly::No,
            None,
            copy::LinkType::Hard,
        )
        .with_context(|| {
            format_context!(
                "Failed to create hard link from {} to {}",
                source,
                destination
            )
        })?;

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
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let logger = logger::Logger::new(progress.console.clone(), name.into());
        let mut workspace_write_lock = workspace.write();
        workspace_write_lock.add_checkout_asset(self.destination.clone(), self.content.clone());

        // just need this for the deprecation warning
        let _ = labels::sanitize_path(self.destination.clone(), None);

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
        _progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        _name: &str,
    ) -> anyhow::Result<()> {
        let mut workspace_write_lock = workspace.write();
        workspace_write_lock
            .settings
            .checkout
            .links
            .insert(self.destination.clone().into());

        let workspace = workspace_write_lock.get_absolute_path();
        let destination = format!("{}/{}", workspace, self.destination);
        let source = self.source.clone();

        // just need this for the deprecation warning
        let _ = labels::sanitize_path(source.clone().into(), None);
        let _ = labels::sanitize_path(destination.clone().into(), None);

        let destination_path = std::path::Path::new(&destination);
        if let Some(parent) = destination_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format_context!(
                    "Failed to create parent directories for soft link {}",
                    destination
                )
            })?;
        }

        if destination_path.is_symlink() {
            symlink::remove_symlink_auto(destination_path).with_context(|| {
                format_context!("Failed to remove existing symlink {}", destination)
            })?;
        } else if destination_path.exists() {
            std::fs::remove_file(destination_path).with_context(|| {
                format_context!("Failed to remove existing file {}", destination)
            })?;
        }

        let source_path = std::path::Path::new(&source);
        if source_path.is_dir() {
            symlink::symlink_dir(source_path, destination_path).with_context(|| {
                format_context!(
                    "Failed to create soft link dir from {} to {}",
                    source,
                    destination
                )
            })?;
        } else {
            symlink::symlink_file(source_path, destination_path).with_context(|| {
                format_context!(
                    "Failed to create soft link file from {} to {}",
                    source,
                    destination
                )
            })?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddHomeAsset {
    pub source: String,
}

impl AddHomeAsset {
    pub fn execute(
        &self,
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let logger = logger::Logger::new(progress.console.clone(), name.into());

        let home = std::env::var("HOME")
            .with_context(|| format_context!("HOME environment variable is not set"))?;

        let source_path = std::path::Path::new(&home).join(&self.source);

        if !source_path.exists() {
            logger.info(&format!(
                "Source path {} does not exist - not importing to workspace home",
                source_path.display()
            ));
            return Ok(());
        }

        let mut workspace_write_lock = workspace.write();
        let _ = workspace_write_lock
            .settings
            .checkout
            .links
            .insert(self.source.clone().into());

        let workspace_path = workspace_write_lock.get_absolute_path();
        let workspace_home = std::path::Path::new(workspace_path.as_ref())
            .join(".spaces/home")
            .join(&self.source);

        if let Some(parent) = workspace_home.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format_context!(
                    "Failed to create workspace home directories for {}",
                    workspace_home.display()
                )
            })?;
        }

        normalize_home_asset_store_entry(&workspace_home, source_path.is_dir()).context(
            format_context!(
                "Failed to normalize home asset entry for {}",
                workspace_home.display()
            ),
        )?;

        if source_path.is_dir() {
            copy::copy_with_cow_semantics(
                progress,
                source_path.to_str().with_context(|| {
                    format_context!(
                        "Failed to convert source path to string {}",
                        source_path.display()
                    )
                })?,
                workspace_home.to_str().with_context(|| {
                    format_context!(
                        "Failed to convert workspace home path to string {}",
                        workspace_home.display()
                    )
                })?,
                copy::UseCowSemantics::No,
                None,
            )
            .with_context(|| {
                format_context!(
                    "Failed to copy home asset {} to workspace home {}",
                    source_path.display(),
                    workspace_home.display()
                )
            })?;
        } else {
            std::fs::copy(&source_path, &workspace_home).with_context(|| {
                format_context!(
                    "Failed to copy home asset {} to workspace home {}",
                    source_path.display(),
                    workspace_home.display()
                )
            })?;
        }

        Ok(())
    }
}

fn normalize_home_asset_store_entry(
    store_full: &std::path::Path,
    source_is_dir: bool,
) -> anyhow::Result<()> {
    if !store_full.exists() {
        return Ok(());
    }

    let store_is_dir = store_full.is_dir();
    if !source_is_dir && !store_is_dir {
        return Ok(());
    }

    if store_is_dir {
        std::fs::remove_dir_all(store_full).context(format_context!(
            "Failed to remove stale store directory {}",
            store_full.display()
        ))?;
    } else {
        std::fs::remove_file(store_full).context(format_context!(
            "Failed to remove stale store file {}",
            store_full.display()
        ))?;
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type")]
pub enum AnyAsset {
    SoftLink(AddSoftLink),
    Asset(AddAsset),
    HardLink(AddHardLink),
    Which(AddWhichAsset),
    Home(AddHomeAsset),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddAnyAssets {
    pub any: Vec<AnyAsset>,
}

impl AddAnyAssets {
    pub fn execute(
        &self,
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        for asset in self.any.iter() {
            match asset {
                AnyAsset::SoftLink(asset) => asset.execute(progress, workspace.clone(), name)?,
                AnyAsset::Asset(asset) => asset.execute(progress, workspace.clone(), name)?,
                AnyAsset::HardLink(asset) => asset.execute(progress, workspace.clone(), name)?,
                AnyAsset::Which(asset) => asset.execute(progress, workspace.clone(), name)?,
                AnyAsset::Home(asset) => asset.execute(progress, workspace.clone(), name)?,
            }
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

#[cfg(test)]
mod tests {
    use super::normalize_home_asset_store_entry;

    #[test]
    fn keeps_existing_file_for_file_source() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let store_full = tempdir.path().join("asset");
        std::fs::write(&store_full, "existing")?;

        normalize_home_asset_store_entry(&store_full, false)?;

        assert!(store_full.is_file());
        assert_eq!(std::fs::read_to_string(store_full)?, "existing");
        Ok(())
    }

    #[test]
    fn removes_existing_directory_for_file_source() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let store_full = tempdir.path().join("asset");
        std::fs::create_dir(&store_full)?;
        std::fs::write(store_full.join("stale"), "stale")?;

        normalize_home_asset_store_entry(&store_full, false)?;

        assert!(!store_full.exists());
        Ok(())
    }

    #[test]
    fn removes_existing_file_for_directory_source() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let store_full = tempdir.path().join("asset");
        std::fs::write(&store_full, "stale")?;

        normalize_home_asset_store_entry(&store_full, true)?;

        assert!(!store_full.exists());
        Ok(())
    }

    #[test]
    fn removes_existing_directory_for_directory_source() -> anyhow::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let store_full = tempdir.path().join("asset");
        std::fs::create_dir(&store_full)?;
        std::fs::write(store_full.join("stale"), "stale")?;

        normalize_home_asset_store_entry(&store_full, true)?;

        assert!(!store_full.exists());
        Ok(())
    }
}
