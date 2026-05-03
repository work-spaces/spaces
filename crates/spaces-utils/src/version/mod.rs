use crate::logger;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::Arc;

mod config;
mod manifest;

pub fn logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "version".into())
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
    "assets": [
      {
        "name": "spaces-v0.60.1-aarch64-apple-darwin.zip",
        "sha256": "8d5f0d7e8f5f2d8a7fbb6f8d6c0d2aa0e4a6cc724d42b8ef90f3e9e7ea1d2f34",
        "url": "https://example.internal/spaces/releases/v0.60.1/spaces-v0.60.1-aarch64-apple-darwin.zip"
      },
      {
        "name": "spaces-v0.60.1-x86_64-unknown-linux-gnu.zip",
        "sha256": "6e81c32d04f7df2be6c7b8d3b57e77a94dd84267fb64f57ea7a0d1f8f6f11df4",
        "url": "https://example.internal/spaces/releases/v0.60.1/spaces-v0.60.1-x86_64-unknown-linux-gnu.zip"
      }
    ]
  }
]
"#;

        let config_path = self.path_to_store.join(config::VERSION_CONFIG_FILE_NAME);
        let divider = "━".repeat(56);

        let write_header = |title: &str| -> anyhow::Result<()> {
            let styled_title =
                console::style::StyledContent::new(console::total_style(), title.to_owned());
            console.raw(format!("{divider}\n"))?;
            console.raw(format!("{styled_title}\n"))?;
            Ok(())
        };

        let write_subheader = |title: &str| -> anyhow::Result<()> {
            let styled_title =
                console::style::StyledContent::new(console::name_style(), title.to_owned());
            console.raw(format!("{divider}\n"))?;
            console.raw(format!("{styled_title}\n"))?;
            Ok(())
        };

        write_header("Version config")?;

        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path).context(format_context!(
                "Failed to read version config from {}",
                config_path.display()
            ))?;

            let config_path_label =
                console::style::StyledContent::new(console::key_style(), "Config path: ");
            let config_path_value = console::style::StyledContent::new(
                console::keyword_style(),
                config_path.display().to_string(),
            );
            console.raw(format!("{config_path_label}{config_path_value}\n\n"))?;

            write_subheader("spaces.version.toml")?;
            console.raw(format!("{contents}\n"))?;
        } else {
            let no_config_label = console::style::StyledContent::new(
                console::warning_style(),
                "No custom config found at:",
            );
            let no_config_bullet = console::style::StyledContent::new(console::key_style(), "- ");
            let no_config_path = console::style::StyledContent::new(
                console::keyword_style(),
                config_path.display().to_string(),
            );
            console.raw(format!(
                "{no_config_label}\n{no_config_bullet}{no_config_path}\n\n"
            ))?;

            let default_config_label = console::style::StyledContent::new(
                console::total_style(),
                "Default config uses gh to fetch releases from:",
            );
            console.raw(format!("{default_config_label}\n"))?;

            let bullet_prefix = console::style::StyledContent::new(console::key_style(), "- ");
            let gh_host_env =
                console::style::StyledContent::new(console::keyword_style(), manifest::GH_HOST_ENV);
            let separator = console::style::StyledContent::new(console::key_style(), ": ");
            let gh_host_default =
                console::style::StyledContent::new(console::name_style(), "github.com");
            console.raw(format!(
                "{bullet_prefix}{gh_host_env}{separator}{gh_host_default}\n"
            ))?;

            let gh_repo_env =
                console::style::StyledContent::new(console::keyword_style(), manifest::GH_REPO);
            let gh_repo_default =
                console::style::StyledContent::new(console::name_style(), "work-spaces/spaces");
            console.raw(format!(
                "{bullet_prefix}{gh_repo_env}{separator}{gh_repo_default}\n"
            ))?;

            let customize_message = console::style::StyledContent::new(
                console::total_style(),
                "Update ^ env values to change where gh looks for releases",
            );
            console.raw(format!("{customize_message}\n\n"))?;

            write_subheader("Sample spaces.version.toml")?;
            console.raw(format!("{SAMPLE_CONFIG}\n"))?;
        }

        console.raw("\n")?;
        write_subheader("Sample version.spaces.json served by your manifest_url")?;
        console.raw(format!("{SAMPLE_MANIFEST}\n"))?;

        Ok(())
    }

    pub fn list(&self, console: console::Console) -> anyhow::Result<()> {
        let mut progress_bar = console::Progress::new(console.clone(), "version-list", None, None);

        let manifest = self
            .load_manifest(&mut progress_bar)
            .context(format_context!("Failed to load/fetch available releases"))?;

        manifest
            .create_hard_links_to_tools(console.clone())
            .context(format_context!("Failed to create hard links to tools"))?;

        let divider = console::style::StyledContent::new(console::key_style(), "─".repeat(56));

        for release in manifest.releases() {
            console.raw(format!("{divider}\n"))?;

            // Release tag name + prerelease badge
            let tag = console::style::StyledContent::new(
                console::name_style(),
                release.tag_name.to_string(),
            );
            if release.prerelease {
                let badge = console::style::StyledContent::new(
                    console::warning_style(),
                    " [pre-release]".to_owned(),
                );
                console.raw(format!("{tag}{badge}\n"))?;
            } else {
                let badge = console::style::StyledContent::new(
                    console::keyword_style(),
                    " [stable]".to_owned(),
                );
                console.raw(format!("{tag}{badge}\n"))?;
            }

            // Per-asset details
            for asset in &release.assets {
                let asset_name = console::style::StyledContent::new(
                    console::total_style(),
                    format!("  {}", asset.name),
                );
                console.raw(format!("{asset_name}\n"))?;

                let url_label = console::style::StyledContent::new(
                    console::key_style(),
                    "    url:    ".to_owned(),
                );
                let url_value = console::style::StyledContent::new(
                    console::keyword_style(),
                    asset.url.to_string(),
                );
                console.raw(format!("{url_label}{url_value}\n"))?;

                let sha_label = console::style::StyledContent::new(
                    console::key_style(),
                    "    sha256: ".to_owned(),
                );
                let sha_value = console::style::StyledContent::new(
                    console::keyword_style(),
                    asset.sha256.to_string(),
                );
                console.raw(format!("{sha_label}{sha_value}\n"))?;

                if let Some(path) = manifest.get_store_path_to_release(console.clone(), asset) {
                    let store_label = console::style::StyledContent::new(
                        console::key_style(),
                        "    store:  ".to_owned(),
                    );
                    if path.exists() {
                        let status = console::style::StyledContent::new(
                            console::name_style(),
                            "Available".to_owned(),
                        );
                        console.raw(format!("{store_label}{status}\n"))?;
                    } else {
                        let status = console::style::StyledContent::new(
                            console::warning_style(),
                            "Not available".to_owned(),
                        );
                        console.raw(format!("{store_label}{status}\n"))?;
                    }
                }
            }

            // Tools binary path
            let binary_path = manifest.get_tools_path_to_binary(release.tag_name.as_ref());
            let tools_label =
                console::style::StyledContent::new(console::key_style(), "  tools: ".to_owned());
            if binary_path.exists() {
                let tools_value = console::style::StyledContent::new(
                    console::keyword_style(),
                    binary_path.display().to_string(),
                );
                console.raw(format!("{tools_label}{tools_value}\n"))?;
            } else {
                let tools_status = console::style::StyledContent::new(
                    console::warning_style(),
                    "Not available in tools path".to_owned(),
                );
                console.raw(format!("{tools_label}{tools_status}\n"))?;
            }
        }

        if !manifest.releases().is_empty() {
            console.raw(format!("{divider}\n"))?;
        }

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
            .context(format_context!("Failed to fetch latest releases"))?;

        let release = manifest.find_release(
            tag.as_deref(),
            matches!(include_prerelease, IncludePrerelease::Yes),
        );

        if let Some(release) = release {
            logger(console.clone()).debug(format!("analyzing {tag:?} (None = latest)").as_str());

            let binary_path = manifest
                .sync_release_to_store(console.clone(), &mut progress_bar, release)
                .context(format_context!(
                    "Failed to download release {}",
                    release.tag_name
                ))?;

            let exec_path = std::env::current_exe()
                .context(format_context!("Failed to get current executable path"))?;
            let command = format!("cp -lf {} {}", binary_path.display(), exec_path.display());

            let divider = console::style::StyledContent::new(console::key_style(), "─".repeat(56));
            console.raw(format!("{divider}\n"))?;

            let tag_label = console::style::StyledContent::new(
                console::key_style(),
                "Fetched release: ".to_owned(),
            );
            let tag_value = console::style::StyledContent::new(
                console::name_style(),
                release.tag_name.to_string(),
            );
            console.raw(format!("{tag_label}{tag_value}\n"))?;

            let tools_label = console::style::StyledContent::new(
                console::key_style(),
                "  tools path:   ".to_owned(),
            );
            let tools_value = console::style::StyledContent::new(
                console::keyword_style(),
                binary_path.display().to_string(),
            );
            console.raw(format!("{tools_label}{tools_value}\n"))?;

            let install_header = console::style::StyledContent::new(
                console::total_style(),
                "\nRun the following command to install the fetched release:".to_owned(),
            );
            console.raw(format!("{install_header}\n"))?;

            let cmd_value =
                console::style::StyledContent::new(console::keyword_style(), command.clone());
            console.raw(format!("{cmd_value}\n"))?;

            if let Ok(mut clipboard) = arboard::Clipboard::new()
                && clipboard
                    .set_text(command)
                    .context(format_context!("Failed to copy command to clipboard"))
                    .is_ok()
            {
                let clipboard_msg = console::style::StyledContent::new(
                    console::name_style(),
                    "Command copied to clipboard".to_owned(),
                );
                console.raw(format!("\n{clipboard_msg}\n"))?;
            }

            console.raw(format!("{divider}\n"))?;

            progress_bar.set_finalize_lines(logger::make_finalize_line(
                logger::FinalType::Finished,
                None,
                format!("downloaded release {}", release.tag_name).as_str(),
            ));
        } else {
            let divider = console::style::StyledContent::new(console::key_style(), "─".repeat(56));
            console.raw(format!("{divider}\n"))?;

            let error_msg = console::style::StyledContent::new(
                console::warning_style(),
                format!(
                    "Release for {} is not available",
                    tag.as_deref().unwrap_or("latest")
                ),
            );
            console.raw(format!("{error_msg}\n"))?;

            console.raw(format!("{divider}\n"))?;

            progress_bar.set_finalize_lines(logger::make_finalize_line(
                logger::FinalType::Failed,
                None,
                "release is not available",
            ));
        }

        Ok(())
    }
}
