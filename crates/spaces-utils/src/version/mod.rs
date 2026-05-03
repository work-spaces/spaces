use crate::logger;
use anyhow::Context;
use anyhow_source_location::format_context;
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
            manifest::Manifest::load_from_store(self.path_to_store.as_ref(), progress_bar)
                .context(format_context!("Failed to load manifest from store"))
        } else {
            self.populate_manifest_from_source(progress_bar)
                .context(format_context!("Failed to fetch manifest from source"))
        }
    }

    pub fn list(&self, console: console::Console) -> anyhow::Result<()> {
        let mut progress_bar = console::Progress::new(console.clone(), "version-list", None, None);

        let manifest = self
            .load_manifest(&mut progress_bar)
            .context(format_context!("Failed to load/fetch available releases"))?;

        manifest
            .create_hard_links_to_tools(console.clone())
            .context(format_context!("Failed to create hard links to tools"))?;

        for release in manifest.releases() {
            logger(console.clone()).info(format!("{}", release.tag_name).as_str());
            for asset in &release.assets {
                logger(console.clone()).info(format!("  {}", asset.name).as_str());
                logger(console.clone())
                    .info(format!("    {}", asset.browser_download_url).as_str());
                if let Some(digest) = asset.digest.as_ref() {
                    logger(console.clone()).info(format!("    {digest}").as_str());
                }
                if let Some(path) = manifest.get_store_path_to_release(console.clone(), asset) {
                    if path.exists() {
                        logger(console.clone()).info("    Is Available in the store");
                    } else {
                        logger(console.clone()).info("    Is NOT Available in the store");
                    }
                }
            }

            let binary_path = manifest.get_tools_path_to_binary(release.tag_name.as_ref());
            if binary_path.exists() {
                logger(console.clone())
                    .info(format!("    tools path: {}", binary_path.display(),).as_str());
            } else {
                logger(console.clone()).info("    Is NOT Available in the store tools path");
            }
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

            logger(console.clone()).info(format!("tools path: {}", binary_path.display()).as_str());
            let exec_path = std::env::current_exe()
                .context(format_context!("Failed to get current executable path"))?;
            let command = format!("cp -lf {} {}", binary_path.display(), exec_path.display());
            logger(console.clone()).info(
                format!(
                    "You need to execute the following command to install the fetched tag:\n\n{command}\n",
                )
                .as_str(),
            );
            if let Ok(mut clipboard) = arboard::Clipboard::new()
                && clipboard
                    .set_text(command)
                    .context(format_context!("Failed to copy command to clipboard"))
                    .is_ok()
            {
                logger(console.clone()).info("Command above was copied to the clipboard");
            }

            progress_bar.set_finalize_lines(logger::make_finalize_line(
                logger::FinalType::Finished,
                None,
                format!("downloaded release {}", release.tag_name).as_str(),
            ));
        } else {
            logger(console.clone()).error(
                format!(
                    "Release for {} is not available",
                    tag.as_deref().unwrap_or("latest")
                )
                .as_str(),
            );
            progress_bar.set_finalize_lines(logger::make_finalize_line(
                logger::FinalType::Failed,
                None,
                "release is not available",
            ));
        }

        Ok(())
    }
}
