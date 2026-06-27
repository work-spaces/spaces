use crate::logger;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::Arc;

mod config;
mod manifest;

pub fn logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "version".into())
}

fn blockquote_from_multiline(
    text: &str,
    variant: console::bootstrap::Variant,
) -> console::bootstrap::Blockquote {
    let mut blockquote = console::bootstrap::Blockquote::new().variant(variant);
    for line in text.lines() {
        blockquote.push_line(line);
    }
    blockquote
}

#[derive(Debug, clap::Subcommand, Clone)]
pub enum Command {
    /// Lists the versions of spaces available. Shows versions available in the store.
    List {},
    /// Fetches the specified version of spaces from the web.
    Fetch {
        /// The tag to fetch, default is the latest version
        #[clap(long)]
        tag: Option<Arc<str>>,
        /// Include pre-releases when fetching the latest version
        #[clap(long)]
        prerelease: bool,
    },
    /// Sets the custom version config file from a TOML file path or set SPACES_ENV_VERSION_CONFIG_PATH
    SetConfig {
        /// Path to a TOML file that contains version config.
        path: Arc<str>,
    },
    /// Unsets the custom version config file.
    UnsetConfig {},
    /// Shows the current version config TOML or a sample if none is set.
    ShowConfig {},
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncludePrerelease {
    No,
    Yes,
}

pub struct Manager {
    path_to_store: Arc<std::path::Path>,
}

impl Manager {
    pub fn new(path_to_store: &std::path::Path) -> Self {
        Self {
            path_to_store: path_to_store.into(),
        }
    }

    fn populate_manifest_from_source(
        &self,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<manifest::Manifest> {
        let mut manifest = manifest::Manifest::new(self.path_to_store.as_ref());

        if let Some(config) = config::Config::new_from_toml(self.path_to_store.as_ref())
            .context(format_context!("Failed to load version manifest config"))?
        {
            config
                .populate_manifest(&mut manifest, progress_bar)
                .context(format_context!(
                    "Failed to populate manifest from custom version config"
                ))?;
        } else {
            manifest
                .populate_using_gh(progress_bar)
                .context(format_context!("Failed to populate manifest using gh"))?;
        }

        Ok(manifest)
    }

    fn load_manifest(
        &self,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<manifest::Manifest> {
        let save_path = self.path_to_store.join(manifest::VERSION_FILE_NAME);
        if save_path.exists() {
            match manifest::Manifest::load_from_store(self.path_to_store.as_ref(), progress_bar) {
                Ok(manifest) => return Ok(manifest),
                Err(_) => {
                    // Cache is stale or in an incompatible format (e.g. old GitHub API schema).
                    // Delete it so a fresh fetch can succeed.
                    let _ = std::fs::remove_file(&save_path);
                    progress_bar.set_message("stale version cache removed, re-fetching");
                }
            }
        }
        self.populate_manifest_from_source(progress_bar)
            .context(format_context!("Failed to fetch manifest from source"))
    }

    pub fn set_config(&self, console: console::Console, path: Arc<str>) -> anyhow::Result<()> {
        let source_path = std::path::Path::new(path.as_ref());

        if !source_path.exists() {
            return Err(format_error!(
                "Config file does not exist: {}",
                source_path.display()
            ));
        }

        if !source_path.is_file() {
            return Err(format_error!(
                "Config path must be a file: {}",
                source_path.display()
            ));
        }

        if source_path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .map(|ext| ext.eq_ignore_ascii_case("toml"))
            != Some(true)
        {
            return Err(format_error!(
                "Config file must have a .toml extension: {}",
                source_path.display()
            ));
        }

        let contents = std::fs::read_to_string(source_path).context(format_context!(
            "Failed to read version config file from {}",
            source_path.display()
        ))?;

        let _: config::Config = toml::from_str(&contents).context(format_context!(
            "Failed to parse version config from {}",
            source_path.display()
        ))?;

        std::fs::create_dir_all(self.path_to_store.as_ref()).context(format_context!(
            "Failed to create store directory at {}",
            self.path_to_store.display()
        ))?;

        let destination_path = self.path_to_store.join(config::VERSION_CONFIG_FILE_NAME);
        std::fs::copy(source_path, &destination_path).context(format_context!(
            "Failed to copy version config from {} to {}",
            source_path.display(),
            destination_path.display()
        ))?;

        logger(console)
            .info(format!("Version config set: {}", destination_path.display(),).as_str());

        Ok(())
    }

    pub fn unset_config(&self, console: console::Console) -> anyhow::Result<()> {
        let config_path = self.path_to_store.join(config::VERSION_CONFIG_FILE_NAME);
        if config_path.exists() {
            std::fs::remove_file(&config_path).context(format_context!(
                "Failed to remove version config at {}",
                config_path.display()
            ))?;

            logger(console)
                .info(format!("Version config removed: {}", config_path.display()).as_str());
        } else {
            logger(console)
                .info(format!("No version config found at {}", config_path.display()).as_str());
        }

        Ok(())
    }

    pub fn show_config(&self, console: console::Console) -> anyhow::Result<()> {
        const SAMPLE_CONFIG: &str = r#"# Example custom version config (version.spaces.toml)
manifest_url = "https://example.internal/spaces/version.spaces.json"

[headers]
Authorization = "Bearer {{SPACES_VERSION_TOKEN}}"

# Tokens used in manifest_url or headers are replaced by env var values.
[env]
"{{SPACES_VERSION_TOKEN}}" = "SPACES_VERSION_TOKEN"
"#;

        const SAMPLE_MANIFEST: &str = r#"[
  {
    "tag_name": "v0.60.1",
    "prerelease": false,
    "assets": {
      "macos-aarch64": {
        "name": "spaces-v0.60.1-aarch64-apple-darwin.zip",
        "sha256": "8d5f0d7e8f5f2d8a7fbb6f8d6c0d2aa0e4a6cc724d42b8ef90f3e9e7ea1d2f34",
        "url": "https://example.internal/spaces/releases/v0.60.1/spaces-v0.60.1-aarch64-apple-darwin.zip"
      },
      "macos-x86_64": {
        "name": "spaces-v0.60.1-x86_64-apple-darwin.zip",
        "sha256": "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b",
        "url": "https://example.internal/spaces/releases/v0.60.1/spaces-v0.60.1-x86_64-apple-darwin.zip"
      },
      "linux-x86_64": {
        "name": "spaces-v0.60.1-x86_64-unknown-linux-gnu.zip",
        "sha256": "6e81c32d04f7df2be6c7b8d3b57e77a94dd84267fb64f57ea7a0d1f8f6f11df4",
        "url": "https://example.internal/spaces/releases/v0.60.1/spaces-v0.60.1-x86_64-unknown-linux-gnu.zip"
      },
      "linux-aarch64": {
        "name": "spaces-v0.60.1-aarch64-unknown-linux-gnu.zip",
        "sha256": "7f90d43e15a8eb2cf7d8c4b68f88b05a5ee95378gc75g68fb01g4f0f8fb2e3g45",
        "url": "https://example.internal/spaces/releases/v0.60.1/spaces-v0.60.1-aarch64-unknown-linux-gnu.zip"
      }
    }
  }
]
"#;

        let config_path = self.path_to_store.join(config::VERSION_CONFIG_FILE_NAME);
        let mut ui = console::container::Container::new();

        ui.add(
            console::bootstrap::Header::h1("Version config")
                .variant(console::bootstrap::Variant::Primary),
        );

        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path).context(format_context!(
                "Failed to read version config from {}",
                config_path.display()
            ))?;

            ui.add(
                console::bootstrap::DescriptionList::new()
                    .variant(console::bootstrap::Variant::Info)
                    .item("Config path:", config_path.display().to_string())
                    .compact(true),
            );
            ui.add(
                console::bootstrap::Header::h2("version.spaces.toml")
                    .variant(console::bootstrap::Variant::Primary),
            );
            ui.add(blockquote_from_multiline(
                &contents,
                console::bootstrap::Variant::Default,
            ));
        } else {
            ui.add(
                console::bootstrap::Alert::new(format!(
                    "No custom config found at:\n{}",
                    config_path.display()
                ))
                .title("Custom version config")
                .variant(console::bootstrap::Variant::Warning)
                .width(console::bootstrap::Width::Large),
            );

            ui.add(
                console::bootstrap::Header::h2("Default gh lookup")
                    .variant(console::bootstrap::Variant::Primary),
            );
            ui.add(
                console::bootstrap::DescriptionList::new()
                    .variant(console::bootstrap::Variant::Info)
                    .item(manifest::GH_HOST_ENV, "github.com")
                    .item(manifest::GH_REPO, "work-spaces/spaces")
                    .compact(true),
            );
            ui.add(
                console::bootstrap::Paragraph::new(
                    "Update these env values to change where gh looks for releases.",
                )
                .variant(console::bootstrap::Variant::Default),
            );
            ui.add(
                console::bootstrap::Header::h2("Sample version.spaces.toml")
                    .variant(console::bootstrap::Variant::Primary),
            );
            ui.add(blockquote_from_multiline(
                SAMPLE_CONFIG,
                console::bootstrap::Variant::Default,
            ));
        }

        ui.add(
            console::bootstrap::Header::h2(
                "Sample version.spaces.json served by your manifest_url",
            )
            .variant(console::bootstrap::Variant::Primary),
        );
        ui.add(blockquote_from_multiline(
            SAMPLE_MANIFEST,
            console::bootstrap::Variant::Default,
        ));

        console.emit_container(&ui);

        Ok(())
    }

    pub fn list(&self, console: console::Console) -> anyhow::Result<()> {
        let mut progress_bar = console::Progress::new(console.clone(), "version-list", None, None);

        let manifest = self
            .load_manifest(&mut progress_bar)
            .map_err(|err| format_error!("Failed to load/fetch available releases\n{err:?}"))?;

        manifest
            .create_hard_links_to_tools(console.clone())
            .map_err(|err| format_error!("Failed to create hard links to tools\n{err:?}"))?;

        let mut ui = console::container::Container::new();
        ui.add(
            console::bootstrap::Header::h1("Available versions")
                .variant(console::bootstrap::Variant::Primary),
        );

        if manifest.releases().is_empty() {
            ui.add(
                console::bootstrap::Alert::new("No releases are currently available")
                    .title("Version list")
                    .variant(console::bootstrap::Variant::Warning)
                    .width(console::bootstrap::Width::Large),
            );
        }

        for release in manifest.releases() {
            ui.add(
                console::bootstrap::Divider::new()
                    .style(console::bootstrap::DividerStyle::Double)
                    .width(console::bootstrap::Width::Large),
            );

            let mut release_line = console::Line::default();
            release_line.push(console::Span::new_styled_lossy(
                console::style::StyledContent::new(
                    console::primary_style(),
                    release.tag_name.to_string(),
                ),
            ));
            let badge_text = if release.prerelease {
                " [pre-release]"
            } else {
                " [stable]"
            };
            release_line.push(console::Span::new_styled_lossy(
                console::style::StyledContent::new(console::danger_style(), badge_text.to_owned()),
            ));
            ui.add(console::bootstrap::Paragraph::from_line(release_line));

            for asset in release.assets.values() {
                ui.add(
                    console::bootstrap::Header::h3(format!("Asset: {}", asset.name))
                        .variant(console::bootstrap::Variant::Default),
                );

                let mut details = console::bootstrap::DescriptionList::new()
                    .item("url:", asset.url.to_string())
                    .item("sha256:", asset.sha256.to_string())
                    .compact(true);

                if let Some(path) = manifest.get_store_path_to_release(console.clone(), asset) {
                    details = details.item(
                        "store:",
                        if path.exists() {
                            "Available".to_owned()
                        } else {
                            "Not available".to_owned()
                        },
                    );
                }

                ui.add(details);
            }

            let binary_path = manifest.get_tools_path_to_binary(release.tag_name.as_ref());
            ui.add(
                console::bootstrap::DescriptionList::new()
                    .item(
                        "tools:",
                        if binary_path.exists() {
                            binary_path.display().to_string()
                        } else {
                            "Not available in tools path".to_owned()
                        },
                    )
                    .compact(true),
            );
        }

        if !manifest.releases().is_empty() {
            ui.add(
                console::bootstrap::Divider::new()
                    .style(console::bootstrap::DividerStyle::Double)
                    .width(console::bootstrap::Width::Large),
            );
        }

        console.emit_container(&ui);

        progress_bar.set_finalize_lines(logger::make_finalize_line(
            logger::FinalType::Finished,
            None,
            "version list",
        ));

        Ok(())
    }

    pub fn fetch(
        &self,
        console: console::Console,
        tag: Option<Arc<str>>,
        include_prerelease: IncludePrerelease,
    ) -> Result<(), anyhow::Error> {
        let mut progress_bar = console::Progress::new(console.clone(), "fetch", None, None);

        let manifest = self
            .populate_manifest_from_source(&mut progress_bar)
            .map_err(|err| format_error!("{err:?}"))?;

        let release = manifest.find_release(
            tag.as_deref(),
            matches!(include_prerelease, IncludePrerelease::Yes),
        );

        if let Some(release) = release {
            logger(console.clone()).debug(format!("analyzing {tag:?} (None = latest)").as_str());

            let binary_path = manifest
                .sync_release_to_store(console.clone(), &mut progress_bar, release)
                .map_err(|err| {
                    format_error!("Failed to download release {}\n{err:?}", release.tag_name)
                })?;

            let exec_path = std::env::current_exe()
                .map_err(|err| format_error!("Failed to get current executable path\n{err:?}"))?;
            let command = format!("cp -lf {} {}", binary_path.display(), exec_path.display());

            let mut ui = console::container::Container::new();
            ui.add(
                console::bootstrap::Banner::new("Release fetched")
                    .width(console::bootstrap::Width::Large)
                    .variant(console::bootstrap::Variant::Success),
            );
            ui.add(
                console::bootstrap::DescriptionList::new()
                    .item("release:", release.tag_name.to_string())
                    .item("tools path:", binary_path.display().to_string())
                    .compact(true)
                    .variant(console::bootstrap::Variant::Primary),
            );

            ui.add(
                console::bootstrap::Header::h2("Run this command to install the fetched release")
                    .variant(console::bootstrap::Variant::Default),
            );
            ui.add(console::bootstrap::Paragraph::from_line(
                console::bootstrap::code(command.clone()),
            ));

            if let Ok(mut clipboard) = arboard::Clipboard::new()
                && clipboard
                    .set_text(command.clone())
                    .map_err(|err| format_error!("Failed to copy command to clipboard\n{err:?}"))
                    .is_ok()
            {
                ui.add(console::bootstrap::VerticalSpacer::new(1));
                ui.add(
                    console::bootstrap::Alert::new("Command copied to clipboard")
                        .variant(console::bootstrap::Variant::Info),
                );
            }

            ui.add(
                console::bootstrap::Divider::new()
                    .style(console::bootstrap::DividerStyle::Double)
                    .width(console::bootstrap::Width::Large),
            );

            console.emit_container(&ui);

            progress_bar.set_finalize_lines(logger::make_finalize_line(
                logger::FinalType::Finished,
                None,
                format!("downloaded release {}", release.tag_name).as_str(),
            ));
        } else {
            let mut ui = console::container::Container::new();
            ui.add(
                console::bootstrap::Alert::new(format!(
                    "Release for {} is not available",
                    tag.as_deref().unwrap_or("latest")
                ))
                .title("Release unavailable")
                .variant(console::bootstrap::Variant::Danger)
                .width(console::bootstrap::Width::Large),
            );
            console.emit_container(&ui);

            progress_bar.set_finalize_lines(logger::make_finalize_line(
                logger::FinalType::Failed,
                None,
                "release is not available",
            ));
        }

        Ok(())
    }
}
