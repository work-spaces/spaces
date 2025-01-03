mod gh;

use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tokio::io::AsyncWriteExt;

struct State {}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    STATE.set(RwLock::new(State {}));
    STATE.get()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum ArchiveLink {
    None,
    #[default]
    Hard,
}

fn label_logger<'a>(
    progress: &'a mut printer::MultiProgressBar,
    label: &str,
) -> logger::Logger<'a> {
    logger::Logger::new_progress(progress, label.into())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub url: Arc<str>,
    pub sha256: Arc<str>,
    pub link: ArchiveLink,
    pub includes: Option<Vec<Arc<str>>>,
    pub excludes: Option<Vec<Arc<str>>>,
    pub globs: Option<HashSet<Arc<str>>>,
    pub strip_prefix: Option<Arc<str>>,
    pub add_prefix: Option<Arc<str>>,
    pub filename: Option<Arc<str>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Files {
    files: HashSet<Arc<str>>,
}

pub fn download(
    mut progress: printer::MultiProgressBar,
    url: &str,
    destination: &str,
    runtime: &tokio::runtime::Runtime,
) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<printer::MultiProgressBar>>> {
    label_logger(&mut progress, url)
        .trace(format!("Downloading using reqwest {url} -> {destination}").as_str());

    let destination = destination.to_string();
    let url = url.to_string();

    let join_handle = runtime.spawn(async move {
        let client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::limited(20))
            .build()?;

        let request = client
            .get(&url)
            .header(reqwest::header::USER_AGENT, "wget")
            .header(reqwest::header::ACCEPT, "*/*");

        label_logger(&mut progress, &url).debug(format!("Reqwest request: {request:?}").as_str());

        let mut response = request.send().await?;

        if !response.status().is_success() {
            return Err(format_error!(
                "Failed to download {url}. got response {response:?}"
            ));
        }

        label_logger(&mut progress, &url).debug(format!("Response: {response:?}").as_str());

        let total_size = response.content_length().unwrap_or(0);
        progress.set_total(total_size);
        progress.set_message(url.as_str());

        let mut output_file = tokio::fs::File::create(destination).await?;

        while let Some(chunk) = response.chunk().await? {
            progress.increment(chunk.len() as u64);
            output_file.write_all(&chunk).await?;
        }

        Ok(progress)
    });

    Ok(join_handle)
}

// TODO Add a version of this that uses GH
pub fn download_string(url: &str) -> anyhow::Result<Arc<str>> {
    let response =
        reqwest::blocking::get(url).context(format_context!("Failed to download {url}"))?;
    let content = response
        .text()
        .context(format_context!("Failed to read response from {url}"))?;
    Ok(content.into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpArchive {
    pub spaces_key: String,
    archive: Archive,
    archive_driver: Option<easy_archiver::driver::Driver>,
    pub full_path_to_archive: String,
    tools_path: String,
    allow_gh_for_download: bool,
}

impl HttpArchive {
    pub fn new(
        bare_store_path: &str,
        spaces_key: &str,
        archive: &Archive,
        tools_path: &str,
    ) -> anyhow::Result<Self> {
        let relative_path = Self::url_to_relative_path(archive.url.as_ref(), &archive.filename)
            .context(format_context!("no relative path for {}", archive.url))?;

        let full_path_to_archive = format!("{bare_store_path}/{relative_path}");

        let archive_path = std::path::Path::new(full_path_to_archive.as_str());

        let mut archive_file_name = archive_path
            .file_name()
            .ok_or(format_error!(
                "No file name found in archive path {full_path_to_archive}"
            ))?
            .to_string_lossy()
            .to_string();

        let (filename, effective_sha256) = if archive.sha256.starts_with("http") {
            let sha256 = download_string(archive.sha256.as_ref())
                .context(format_context!("Failed to download {}", archive.sha256))?;
            if sha256.len() != 64 {
                return Err(format_error!(
                    "Invalid sha256 checksum {sha256} for {}",
                    archive.url
                ));
            }
            (None, Some(sha256))
        } else {
            (None, None)
        };

        archive_file_name = filename.unwrap_or(archive_file_name);
        let effective_sha256 = effective_sha256.unwrap_or(archive.sha256.clone());

        let archive_driver_result =
            easy_archiver::driver::Driver::from_filename(archive_file_name.as_str()).context(
                format_context!("Failed to get driver for {archive_file_name}"),
            );

        let mut archive_driver = None;
        let full_path_to_archive = match archive_driver_result {
            Ok(driver) => {
                archive_driver = Some(driver);
                format!(
                    "{}/{}.{}",
                    full_path_to_archive,
                    effective_sha256,
                    driver.extension()
                )
            }
            Err(_) => {
                format!("{full_path_to_archive}/{archive_file_name}")
            }
        };

        let mut archive = archive.clone();
        archive.sha256 = effective_sha256;

        Ok(Self {
            archive,
            archive_driver,
            full_path_to_archive,
            spaces_key: spaces_key.to_string(),
            allow_gh_for_download: true,
            tools_path: tools_path.to_owned(),
        })
    }

    pub fn allow_gh_for_download(&mut self, value: bool) {
        self.allow_gh_for_download = value;
    }

    pub fn get_path_to_extracted_files(&self) -> String {
        format!("{}_files", self.full_path_to_archive)
    }

    fn get_path_to_extracted_files_json(&self) -> String {
        format!("{}.json", self.get_path_to_extracted_files())
    }

    pub fn is_download_required(&self) -> bool {
        !std::path::Path::new(&self.full_path_to_archive).exists()
    }

    fn is_extract_required(&self) -> bool {
        !std::path::Path::new(self.get_path_to_extracted_files().as_str()).exists()
    }

    pub fn create_links(
        &self,
        mut progress_bar: printer::MultiProgressBar,
        workspace_directory: &str,
        space_directory: &str,
    ) -> anyhow::Result<()> {
        //construct a list of files to link
        let mut files = Vec::new();
        let all_files = self
            .load_files_json()
            .context(format_context!("failed to load json files manifest"))?;
        for file in all_files.iter() {
            let mut is_match = true;
            if let Some(globs) = self.archive.globs.as_ref() {
                is_match = changes::glob::match_globs(globs, file);
            }

            if is_match {
                files.push(file);
            }
        }

        let target_prefix = if let Some(add_prefix) = self.archive.add_prefix.as_ref() {
            if add_prefix.starts_with("//") {
                format!("{workspace_directory}/{add_prefix}").into()
            } else if add_prefix.starts_with('/') {
                add_prefix.clone()
            } else {
                format!("{workspace_directory}/{add_prefix}").into()
            }
        } else {
            format!("{workspace_directory}/{space_directory}").into()
        };

        progress_bar.set_total(files.len() as u64);

        for file in files {
            let source = format!("{}/{}", self.get_path_to_extracted_files(), file);

            progress_bar.set_message(file.as_ref());
            let relative_target_path =
                if let Some(strip_prefix) = self.archive.strip_prefix.as_ref() {
                    file.strip_prefix(strip_prefix.as_ref())
                } else {
                    Some(file.as_ref())
                };

            if let Some(relative_target_path) = relative_target_path {
                let full_target_path = format!("{}/{}", target_prefix, relative_target_path);

                match self.archive.link {
                    ArchiveLink::Hard => {
                        label_logger(&mut progress_bar, "hardlink").trace(
                            format!("Creating hard link {full_target_path} -> {source}").as_str(),
                        );
                        Self::create_hard_link(full_target_path.clone(), source.clone()).context(
                            format_context!("hard link {full_target_path} -> {source}",),
                        )?;
                    }
                    ArchiveLink::None => (),
                }
            } else {
                label_logger(&mut progress_bar, "hardlink").warning(
                    format!(
                        "Failed to strip prefix {:?} from {file}",
                        self.archive.strip_prefix
                    )
                    .as_str(),
                );
            }
            progress_bar.increment(1);
        }

        Ok(())
    }

    pub fn create_hard_link(target_path: String, source: String) -> anyhow::Result<()> {
        let target = std::path::Path::new(target_path.as_str());
        let original = std::path::Path::new(source.as_str());

        // Hold the mutex to ensure operations are atomic
        #[allow(clippy::readonly_write_lock)]
        let _state = get_state().write().unwrap();

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .context(format_context!("{target_path} -> {source}"))?;
        }

        let _ = std::fs::remove_file(target);

        //if the source is a symlink, read the symlink and create a symlink
        if original.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(original, target).context(format_context!(
                "failed to create symlink {original:?} -> {target_path}"
            ))?;

            #[cfg(windows)]
            #[cfg(unix)]
            std::os::windows::fs::symlink_file(link.clone(), target).context(format_context!(
                "failed to create symlink {link:?} -> {target_path}"
            ))?;

            return Ok(());
        }

        std::fs::hard_link(original, target).context(format_context!(
            "If you get 'Operation Not Permitted' on mac try enabling 'Full Disk Access' for the terminal",
        ))?;

        Ok(())
    }

    pub fn sync(
        &self,
        mut progress_bar: printer::MultiProgressBar,
    ) -> anyhow::Result<printer::MultiProgressBar> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .context(format_context!("Failed to create runtime"))?;

        let mut next_progress_bar = if self.is_download_required() {
            if let Some(arguments) = gh::transform_url_to_arguments(
                self.allow_gh_for_download,
                self.archive.url.as_ref(),
                &self.full_path_to_archive,
            ) {
                let gh_command = format!("{}/gh", self.tools_path);
                gh::download(&gh_command, &self.archive.url, arguments, &mut progress_bar)
                    .context(format_context!("Failed to download using gh"))?;

                progress_bar
            } else {
                label_logger(&mut progress_bar, &self.archive.url)
                    .debug(format!("{} Downloading using reqwest", self.archive.url).as_str());

                let join_handle = self
                    .download(&runtime, progress_bar)
                    .context(format_context!("Failed to download using reqwest"))?;
                runtime.block_on(join_handle)??
            }
        } else {
            label_logger(&mut progress_bar, &self.archive.url)
                .debug(format!("{} download not required", self.archive.url).as_str());
            progress_bar
        };

        label_logger(&mut next_progress_bar, &self.archive.url).debug("Extracting archive");

        let next_progress_bar = self.extract(next_progress_bar).context(format_context!(
            "extract failed {}",
            self.full_path_to_archive
        ))?;
        Ok(next_progress_bar)
    }

    pub fn download(
        &self,
        runtime: &tokio::runtime::Runtime,
        progress: printer::MultiProgressBar,
    ) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<printer::MultiProgressBar>>> {
        let full_path_to_archive = self.full_path_to_archive.clone();
        let full_path = std::path::Path::new(&full_path_to_archive);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        download(
            progress,
            self.archive.url.as_ref(),
            full_path_to_archive.as_str(),
            runtime,
        )
    }

    fn save_files_json(&self, files: Files) -> anyhow::Result<()> {
        let file_path = self.get_path_to_extracted_files_json();
        let contents = serde_json::to_string_pretty(&files)?;
        std::fs::write(file_path, contents)?;
        Ok(())
    }

    fn load_files_json(&self) -> anyhow::Result<HashSet<Arc<str>>> {
        let file_path = self.get_path_to_extracted_files_json();
        let contents = std::fs::read_to_string(file_path.as_str())
            .context(format_context!("while reading {file_path}"))?;
        let files: Files = serde_json::from_str(contents.as_str())
            .context(format_context!("while parsing {file_path}"))?;
        Ok(files.files)
    }

    fn extract(
        &self,
        mut progress_bar: printer::MultiProgressBar,
    ) -> anyhow::Result<printer::MultiProgressBar> {
        if !self.is_extract_required() {
            label_logger(&mut progress_bar, &self.archive.url)
                .debug("Extract not required");
            return Ok(progress_bar);
        }

        std::fs::create_dir_all(self.get_path_to_extracted_files().as_str())
            .context(format_context!("creating {}", self.full_path_to_archive))?;

        let mut extracted_files = HashSet::new();

        let next_progress_bar = if self.archive_driver.is_some() {
            let decoder = easy_archiver::Decoder::new(
                &self.full_path_to_archive,
                Some(self.archive.sha256.to_string()),
                &self.get_path_to_extracted_files(),
                progress_bar,
            )
            .context(format_context!(
                "{} -> {}",
                self.full_path_to_archive.as_str(),
                self.get_path_to_extracted_files()
            ))?;

            let extracted = decoder.extract().context(format_context!(
                "{} -> {}",
                self.full_path_to_archive,
                self.get_path_to_extracted_files()
            ))?;

            extracted_files = extracted.files;
            extracted.progress_bar
        } else {
            let path_to_artifact = std::path::Path::new(self.full_path_to_archive.as_str());
            let file_name = path_to_artifact.file_name().ok_or(format_error!(
                "No file name found in archive path {path_to_artifact:?}"
            ))?;

            // the file needs to be moved to the extracted files directory
            // that is where the create links function will look for it
            let target =
                std::path::Path::new(self.get_path_to_extracted_files().as_str()).join(file_name);

            std::fs::rename(path_to_artifact, target.clone())
                .context(format_context!("copy {path_to_artifact:?} -> {target:?}"))?;

            extracted_files.insert(file_name.to_string_lossy().to_string());
            progress_bar
        };
        self.save_files_json(Files {
            files: extracted_files
                .into_iter()
                .map(|file| file.into())
                .collect(),
        })
        .context(format_context!("Failed to save json files manifest"))?;
        Ok(next_progress_bar)
    }

    fn url_to_relative_path(url: &str, filename: &Option<Arc<str>>) -> anyhow::Result<String> {
        let archive_url = url::Url::parse(url)
            .context(format_context!("Failed to parse bare store url {url}"))?;

        let host = archive_url
            .host_str()
            .ok_or(format_error!("No host found in url {}", url))?;
        let scheme = archive_url.scheme();
        let path = archive_url.path();
        let mut relative_path = format!("{scheme}/{host}{path}");
        if let Some(filename) = filename {
            relative_path = format!("{}/{}", relative_path, filename);
        }
        Ok(relative_path)
    }
}
