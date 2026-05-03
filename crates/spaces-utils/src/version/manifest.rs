use crate::{http_archive, logger, platform, store, ws};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

pub const VERSION_FILE_NAME: &str = "spaces.version.json";

fn logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "version/manifest".into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub url: Option<Arc<str>>,
    pub name: Arc<str>,
    pub digest: Option<Arc<str>>,
    pub browser_download_url: Arc<str>,
}

impl Asset {
    pub fn get_digest(&self) -> Option<Arc<str>> {
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
pub struct Release {
    pub tag_name: Arc<str>,
    #[serde(default)]
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

pub struct Manifest {
    path_to_store: Arc<std::path::Path>,
    releases: Vec<Release>,
}

impl Manifest {
    pub fn new(path_to_store: &std::path::Path) -> Self {
        Self {
            path_to_store: path_to_store.into(),
            releases: Vec::new(),
        }
    }

    pub fn releases(&self) -> &Vec<Release> {
        &self.releases
    }

    pub fn load_from_store(
        path_to_store: &std::path::Path,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<Self> {
        let save_path = path_to_store.join(VERSION_FILE_NAME);
        progress_bar.set_message("loading manifest");
        let json = std::fs::read_to_string(&save_path).context(format_context!(
            "Failed to load manifest from {}",
            save_path.display()
        ))?;
        let releases: Vec<Release> =
            serde_json::from_str(&json).context(format_context!("Failed to parse manifest"))?;

        Ok(Self {
            path_to_store: path_to_store.into(),
            releases,
        })
    }

    pub fn populate_using_gh(
        &mut self,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<()> {
        let options = console::ExecuteOptions {
            arguments: vec!["api".into(), "repos/work-spaces/spaces/releases".into()],
            is_return_stdout: true,
            environment: vec![("GH_HOST".into(), "github.com".into())],
            ..Default::default()
        };

        progress_bar.set_message("getting latest version using gh");

        let gh_command =
            ws::get_spaces_tools_path_to_sysroot_bin(self.path_to_store.as_ref()).join("gh");

        if let Some(stdout) = progress_bar
            .execute_process(gh_command.to_string_lossy().as_ref(), options)
            .context(format_context!("Failed to execute gh api to get releases"))?
            .stdout
        {
            self.set_releases_from_json(stdout.as_str())
                .context(format_context!("Failed to parse manifest response from gh"))?;
            self.save_to_store()
                .context(format_context!("Failed to save manifest from gh"))?;
            Ok(())
        } else {
            Err(format_error!("Failed to fetch latest release"))
        }
    }

    pub fn populate_from_url(
        &mut self,
        progress_bar: &mut console::Progress,
        url: &str,
        headers: &HashMap<Arc<str>, Arc<str>>,
    ) -> anyhow::Result<()> {
        progress_bar.set_message("downloading custom version manifest");

        let mut req_headers = reqwest::header::HeaderMap::new();
        for (key, value) in headers {
            let header_name = reqwest::header::HeaderName::from_str(key.as_ref())
                .context(format_context!("Invalid header name '{}'", key))?;
            let header_value = reqwest::header::HeaderValue::from_str(value.as_ref())
                .context(format_context!("Invalid header value for '{}'", key))?;
            req_headers.insert(header_name, header_value);
        }

        let client = reqwest::blocking::ClientBuilder::new()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context(format_context!(
                "Failed to create HTTP client for manifest download"
            ))?;

        let response = client
            .get(url)
            .headers(req_headers)
            .send()
            .context(format_context!("Failed to download manifest from {}", url))?;

        let status = response.status();
        if !status.is_success() {
            return Err(format_error!(
                "Failed to download manifest from {}: HTTP {}",
                url,
                status
            ));
        }

        let body = response.text().context(format_context!(
            "Failed to read manifest response from {}",
            url
        ))?;

        self.set_releases_from_json(body.as_str())
            .context(format_context!(
                "Failed to parse manifest response from {}",
                url
            ))?;
        self.save_to_store()
            .context(format_context!("Failed to save downloaded manifest"))?;

        Ok(())
    }

    fn set_releases_from_json(&mut self, json: &str) -> anyhow::Result<()> {
        let releases: Vec<Release> = serde_json::from_str(json)
            .context(format_context!("Failed to parse JSON response\n{json}"))?;
        self.releases = releases;
        Ok(())
    }

    fn save_to_store(&self) -> anyhow::Result<()> {
        let save_path = self.path_to_store.join(VERSION_FILE_NAME);
        let json = serde_json::to_string_pretty(&self.releases).context(format_context!(
            "Internal error: failed to serialize releases"
        ))?;
        std::fs::write(&save_path, json).context(format_context!(
            "Failed to write releases to file {}",
            save_path.display()
        ))?;
        Ok(())
    }

    pub fn find_release(&self, tag: Option<&str>, include_prerelease: bool) -> Option<&Release> {
        if let Some(tag) = tag {
            self.releases
                .iter()
                .find(|release| release.tag_name.as_ref() == tag)
        } else if include_prerelease {
            self.releases.first()
        } else {
            self.releases.iter().find(|release| !release.prerelease)
        }
    }

    pub fn get_store_path_to_release(
        &self,
        console: console::Console,
        asset: &Asset,
    ) -> Option<Arc<std::path::Path>> {
        match http_archive::HttpArchive::url_to_relative_path(&asset.browser_download_url, &None) {
            Ok(relative_path) => {
                let path_in_store = self.path_to_store.join(relative_path);
                Some(path_in_store.into())
            }
            Err(err) => {
                logger(console.clone())
                    .error(format!("Failed to convert URL to relative path: {err}").as_str());
                None
            }
        }
    }

    pub fn get_store_path_to_store_binary(
        &self,
        console: console::Console,
        asset: &Asset,
    ) -> Option<Arc<std::path::Path>> {
        let store_path_to_release = self.get_store_path_to_release(console, asset);

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

    pub fn get_tools_path_to_binary(&self, tag: &str) -> Arc<std::path::Path> {
        ws::get_spaces_tools_path_to_sysroot_bin(&self.path_to_store)
            .join(format!("spaces-{tag}"))
            .into()
    }

    pub fn create_hard_links_to_tools(&self, console: console::Console) -> anyhow::Result<()> {
        for release in &self.releases {
            for asset in &release.assets {
                if let Some(current_platform) = platform::Platform::get_platform() {
                    if asset
                        .browser_download_url
                        .contains(current_platform.to_string().as_str())
                    {
                        let binary_path = self.get_tools_path_to_binary(&release.tag_name);
                        if binary_path.exists() {
                            logger(console.clone()).trace(
                                format!("Not linking {} already exists", binary_path.display())
                                    .as_str(),
                            );
                            continue;
                        }
                        if let Some(source_path) =
                            self.get_store_path_to_store_binary(console.clone(), asset)
                        {
                            if source_path.exists() {
                                logger(console.clone()).debug(
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
                                logger(console.clone()).debug(
                                    format!("Not linking {} does not exist", source_path.display())
                                        .as_str(),
                                );
                            }
                        }
                    } else {
                        logger(console.clone()).debug(
                            format!("Not linking. No binary for platform {current_platform}",)
                                .as_str(),
                        );
                    }
                } else {
                    logger(console.clone()).debug("Internal error: unknown platform");
                }
            }
        }

        Ok(())
    }

    pub fn sync_release_to_store(
        &self,
        console: console::Console,
        progress_bar: &mut console::Progress,
        release: &Release,
    ) -> anyhow::Result<Arc<std::path::Path>> {
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

        if let Some(path) = self.get_store_path_to_release(console.clone(), asset) {
            if path.exists() {
                logger(console.clone()).info(format!("store path: {}", path.display()).as_str());
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

                progress_bar.set_message(&format!("downloading {}", release.tag_name));
                http_archive.sync(console.clone()).context(format_context!(
                    "Failed to download spaces {}",
                    release.tag_name
                ))?;
            }

            let relative_path =
                http_archive::HttpArchive::url_to_relative_path(&asset.browser_download_url, &None)
                    .context(format_context!(
                        "Failed to get relative path for {}",
                        asset.browser_download_url
                    ))?;

            let mut manifest_store = store::Store::new_from_store_path(&self.path_to_store)
                .context(format_context!("Failed to load store manifest"))?;
            manifest_store
                .add_entry(std::path::Path::new(&relative_path))
                .context(format_context!("Failed to add version to store manifest"))?;
            manifest_store
                .save(&self.path_to_store)
                .context(format_context!("Failed to save store manifest"))?;

            let binary_path = self.get_tools_path_to_binary(release.tag_name.as_ref());
            if !binary_path.exists() {
                self.create_hard_links_to_tools(console.clone())
                    .context(format_context!("Failed to update tools to store links"))?;
            }

            if binary_path.exists() {
                Ok(binary_path)
            } else {
                Err(format_error!(
                    "Internal error: tools binary is not found: {}",
                    binary_path.display()
                ))
            }
        } else {
            Err(format_error!(
                "Failed to determine store path for {}",
                asset.browser_download_url
            ))
        }
    }
}
