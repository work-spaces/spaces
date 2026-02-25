use crate::{http_archive, logger, platform, ws};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

const VERSION_FILE_NAME: &str = "spaces.version.json";

pub fn logger(printer: &mut printer::Printer) -> logger::Logger<'_> {
    logger::Logger::new_printer(printer, "version".into())
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
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GithubAsset {
    url: Option<Arc<str>>,
    name: Arc<str>,
    digest: Option<Arc<str>>,
    browser_download_url: Arc<str>,
}

impl GithubAsset {
    fn get_digest(&self) -> Option<Arc<str>> {
        if let Some(digest) = &self.digest {
            if let Some((_, digest)) = digest.split_once(':') {
                Some(digest.into())
            } else {
                None
            }
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GithubRelease {
    tag_name: Arc<str>,
    assets: Vec<GithubAsset>,
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

    fn load_from_store(
        &self,
        printer: &mut printer::Printer,
    ) -> anyhow::Result<Vec<GithubRelease>> {
        let save_path = self.path_to_store.join(VERSION_FILE_NAME);
        if save_path.exists() {
            let json = std::fs::read_to_string(save_path)
                .context(format_context!("Failed to load from store"))?;
            let releases: Vec<GithubRelease> = serde_json::from_str(&json)
                .context(format_context!("Failed to parse JSON from store"))?;
            Ok(releases)
        } else {
            self.fetch_latest(printer)
        }
    }

    fn fetch_latest(&self, printer: &mut printer::Printer) -> anyhow::Result<Vec<GithubRelease>> {
        let options = printer::ExecuteOptions {
            arguments: vec!["api".into(), "repos/work-spaces/spaces/releases".into()],
            is_return_stdout: true,
            environment: vec![("GH_HOST".into(), "github.com".into())],
            ..Default::default()
        };

        let gh_command =
            ws::get_spaces_tools_path_to_sysroot_bin(self.path_to_store.as_ref()).join("gh");

        let mut multi_progress = printer::MultiProgress::new(printer);
        let mut progress_bar = multi_progress.add_progress("download", None, None);

        if let Some(stdout) = progress_bar
            .execute_process(gh_command.to_string_lossy().as_ref(), options)
            .context(format_context!("Failed to execute gh api to get releases"))?
        {
            let releases: Vec<GithubRelease> = serde_json::from_str(stdout.as_str()).context(
                format_context!("Failed to parse JSON response ```\n{stdout}```\n"),
            )?;

            let save_path = self.path_to_store.join(VERSION_FILE_NAME);
            let json = serde_json::to_string_pretty(&releases).context(format_context!(
                "Internal error: failed to serialize releases"
            ))?;
            std::fs::write(&save_path, json).context(format_context!(
                "Failed to write releases to file {}",
                save_path.display()
            ))?;

            Ok(releases)
        } else {
            Err(format_error!("Failed to fetch latest release"))
        }
    }

    fn get_store_path_to_release(
        &self,
        printer: &mut printer::Printer,
        asset: &GithubAsset,
    ) -> Option<Arc<std::path::Path>> {
        match http_archive::HttpArchive::url_to_relative_path(&asset.browser_download_url, &None) {
            Ok(relative_path) => {
                let path_in_store = self.path_to_store.join(relative_path);
                Some(path_in_store.into())
            }
            Err(err) => {
                logger(printer)
                    .error(format!("Failed to convert URL to relative path: {err}").as_str());
                None
            }
        }
    }

    fn get_store_path_to_store_binary(
        &self,
        printer: &mut printer::Printer,
        asset: &GithubAsset,
    ) -> Option<Arc<std::path::Path>> {
        let store_path_to_release = self.get_store_path_to_release(printer, asset);

        if let (Some(store_path), Some(digest)) = (store_path_to_release, asset.get_digest()) {
            Some(
                store_path
                    .join(format!("{digest}.zip_files"))
                    .join("spaces")
                    .into(),
            )
        } else {
            None
        }
    }

    fn get_tools_path_to_binary(&self, tag: &str) -> Arc<std::path::Path> {
        ws::get_spaces_tools_path_to_sysroot_bin(&self.path_to_store)
            .join(format!("spaces-{tag}"))
            .into()
    }

    fn create_hard_links_to_tools(
        &self,
        printer: &mut printer::Printer,
        releases: &Vec<GithubRelease>,
    ) -> anyhow::Result<()> {
        for release in releases {
            for asset in release.assets.iter() {
                if let Some(current_platform) = platform::Platform::get_platform() {
                    if asset
                        .browser_download_url
                        .contains(current_platform.to_string().as_str())
                    {
                        let binary_path = self.get_tools_path_to_binary(&release.tag_name);
                        if binary_path.exists() {
                            logger(printer).trace(
                                format!("Not linking {} already exists", binary_path.display())
                                    .as_str(),
                            );
                            continue;
                        }
                        if let Some(source_path) =
                            self.get_store_path_to_store_binary(printer, asset)
                        {
                            if source_path.exists() {
                                logger(printer).debug(
                                    format!(
                                        "Creating hard link from {} to {}",
                                        source_path.display(),
                                        binary_path.display()
                                    )
                                    .as_str(),
                                );
                                std::fs::hard_link(&source_path, &binary_path).context(
                                    format_context!(
                                        "failed to link {} to {}",
                                        source_path.display(),
                                        binary_path.display()
                                    ),
                                )?;
                            } else {
                                logger(printer).debug(
                                    format!("Not linking {} does not exist", source_path.display())
                                        .as_str(),
                                );
                            }
                        }
                    } else {
                        logger(printer).debug(
                            format!("Not linking. No binary for platform {current_platform}",)
                                .as_str(),
                        );
                    }
                } else {
                    logger(printer).debug("Internal error: unknown platform");
                }
            }
        }
        Ok(())
    }

    pub fn list(&self, printer: &mut printer::Printer) -> anyhow::Result<()> {
        let releases = self
            .load_from_store(printer)
            .context(format_context!("Failed to load/fetch available releases"))?;

        self.create_hard_links_to_tools(printer, &releases)
            .context(format_context!("Failed to create hard links to tools"))?;

        for release in releases {
            logger(printer).info(format!("{}", release.tag_name).as_str());
            for asset in release.assets {
                logger(printer).info(format!("  {}", asset.name).as_str());
                logger(printer).info(format!("    {}", asset.browser_download_url).as_str());
                if let Some(digest) = asset.digest.as_ref() {
                    logger(printer).info(format!("    {digest}").as_str());
                }
                if let Some(path) = self.get_store_path_to_release(printer, &asset) {
                    if path.exists() {
                        logger(printer).info("    Is Available in the store");
                    } else {
                        logger(printer).info("    Is NOT Available in the store");
                    }
                }
            }
            let binary_path = self.get_tools_path_to_binary(release.tag_name.as_ref());
            if binary_path.exists() {
                logger(printer).info(
                    format!(
                        "    tools path: {}",
                        self.get_tools_path_to_binary(release.tag_name.as_ref())
                            .display(),
                    )
                    .as_str(),
                );
            } else {
                logger(printer).info("    Is NOT Available in the store tools path");
            }
        }

        Ok(())
    }

    pub fn fetch(
        &self,
        printer: &mut printer::Printer,
        tag: Option<Arc<str>>,
    ) -> Result<(), anyhow::Error> {
        let releases = self
            .fetch_latest(printer)
            .context(format_context!("Failed to fetch latest releases"))?;

        let release = if let Some(tag) = tag.clone() {
            releases.iter().find(|release| release.tag_name == tag)
        } else {
            releases.first()
        };

        if let Some(release) = release {
            logger(printer).debug("analyzing {tag:?} (None = latest)");
            let current_platform = platform::Platform::get_platform()
                .context(format_context!("Internal Error: Unknown Platform"))?;

            let asset = release
                .assets
                .iter()
                .find(|asset| asset.name.contains(current_platform.to_string().as_str()))
                .context(format_context!(
                    "No asset found for the current platform for {}",
                    release.tag_name
                ))?;

            if let Some(path) = self.get_store_path_to_release(printer, asset) {
                if path.exists() {
                    logger(printer).info(format!("store path: {}", path.display()).as_str());
                } else {
                    let digest = asset
                        .get_digest()
                        .context(format_context!("No digest available for asset"))?;

                    let archive = http_archive::Archive {
                        url: asset.browser_download_url.clone(),
                        sha256: digest.clone(),
                        link: http_archive::ArchiveLink::Hard,
                        ..Default::default()
                    };

                    let http_archive = http_archive::HttpArchive::new(
                        &self.path_to_store.to_string_lossy(),
                        format!("spaces-{}", release.tag_name).as_str(),
                        &archive,
                        &ws::get_spaces_tools_path_to_sysroot_bin(&self.path_to_store)
                            .to_string_lossy(),
                    )
                    .context(format_context!(
                        "Failed to create http_archive to download spaces {}",
                        release.tag_name
                    ))?;

                    let mut multiprogress = printer::MultiProgress::new(printer);
                    let multiprogress_bar = multiprogress.add_progress("download", None, None);
                    http_archive
                        .sync(multiprogress_bar)
                        .context(format_context!(
                            "Failed to download spaces {}",
                            release.tag_name
                        ))?;
                }
                let binary_path = self.get_tools_path_to_binary(release.tag_name.as_ref());

                if !binary_path.exists() {
                    self.create_hard_links_to_tools(printer, &releases)
                        .context(format_context!("Failed to update tools to store links"))?;
                }

                if binary_path.exists() {
                    logger(printer).info(format!("tools path: {}", binary_path.display()).as_str());
                    let exec_path = std::env::current_exe()
                        .context(format_context!("Failed to get current executable path"))?;
                    let command =
                        format!("cp -lf {} {}", binary_path.display(), exec_path.display());
                    logger(printer).info(format!("Install with:\n\n{command}\n",).as_str());
                    if let Ok(mut clipboard) = arboard::Clipboard::new()
                        && clipboard
                            .set_text(command)
                            .context(format_context!("Failed to copy command to clipboard"))
                            .is_ok()
                    {
                        logger(printer).info("Command above was copied to the clipboard");
                    }
                } else {
                    logger(printer).error(
                        format!(
                            "Internal error: tools binary is not found: {}",
                            binary_path.display()
                        )
                        .as_str(),
                    );
                }
            }
        } else {
            logger(printer).error(
                format!(
                    "Release for {} is not available",
                    tag.unwrap_or("latest".into())
                )
                .as_str(),
            );
        }

        Ok(())
    }
}
