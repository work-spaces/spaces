use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::RwLock;
use tokio::io::AsyncWriteExt;
use url::Url;

struct State {}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    STATE.set(RwLock::new(State {}));
    STATE.get()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ArchiveLink {
    None,
    Hard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub url: String,
    pub sha256: String,
    pub link: ArchiveLink,
    pub includes: Option<Vec<String>>,
    pub excludes: Option<Vec<String>>,
    pub strip_prefix: Option<String>,
    pub add_prefix: Option<String>,
    pub filename: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Files {
    files: HashSet<String>,
}

fn transform_url_to_gh_arguments(
    allow_gh_for_download: bool,
    url: &str,
    full_path_to_archive: &str,
) -> Option<Vec<String>> {
    if !allow_gh_for_download {
        return None;
    }

    // use which to see if gh is installed
    if which::which("gh").is_err() {
        return None;
    }

    // Parse the URL
    let parsed_url = Url::parse(url).ok()?;

    // Ensure the URL is for GitHub releases
    if parsed_url.domain()? != "github.com" {
        return None;
    }

    // Split the path to extract owner, repo, and tag
    let mut path_segments = parsed_url.path_segments()?;
    let owner = path_segments.next()?;
    let repo = path_segments.next()?;
    let release_segment = path_segments.next()?;

    // Ensure it's a release download URL
    if release_segment != "releases" {
        return None;
    }

    // Check if it has "download/tag" structure
    let download_segment = path_segments.next()?;
    let tag = if download_segment == "download" {
        path_segments.next()?
    } else {
        return None;
    };
    let pattern = path_segments.next()?;

    // Return the GitHub CLI command arguments
    Some(vec![
        "release".to_string(),
        "download".to_string(),
        tag.to_string(),
        format!("--repo={}/{}", owner, repo),
        format!("--pattern={}", pattern),
        format!("--output={full_path_to_archive}"),
    ])
}

pub fn download(
    url: &str,
    destination: &str,
    runtime: &tokio::runtime::Runtime,
    mut progress: printer::MultiProgressBar,
) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<printer::MultiProgressBar>>> {
    progress.log(
        printer::Level::Trace,
        format!("Downloading using reqwest {url} -> {destination}").as_str(),
    );

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

        progress.log(
            printer::Level::Debug,
            format!("Reqwest request: {request:?}").as_str(),
        );

        let mut response = request
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format_error!("Failed to download {url}. got response {response:?}"));
        }

        progress.log(
            printer::Level::Debug,
            format!("Response: {response:?}").as_str(),
        );

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
pub fn download_string(url: &str) -> anyhow::Result<String> {
    let response =
        reqwest::blocking::get(url).context(format_context!("Failed to download {url}"))?;
    let content = response
        .text()
        .context(format_context!("Failed to read response from {url}"))?;
    Ok(content)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpArchive {
    pub spaces_key: String,
    archive: Archive,
    archive_driver: Option<easy_archiver::driver::Driver>,
    full_path_to_archive: String,
    allow_gh_for_download: bool,
}

impl HttpArchive {
    pub fn new(bare_store_path: &str, spaces_key: &str, archive: &Archive) -> anyhow::Result<Self> {
        let relative_path = Self::url_to_relative_path(archive.url.as_str(), &archive.filename)
            .context(format_context!("no relative path for {}", archive.url))?;

        let full_path_to_archive = format!("{bare_store_path}/{relative_path}");

        let archive_path = std::path::Path::new(full_path_to_archive.as_str());

        let archive_file_name = archive_path
            .file_name()
            .ok_or(format_error!(
                "No file name found in archive path {full_path_to_archive}"
            ))?
            .to_string_lossy()
            .to_string();

        let archive_driver_result =
            easy_archiver::driver::Driver::from_filename(archive_file_name.as_str()).context(
                format_context!("Failed to get driver for {archive_file_name}"),
            );

        let effective_sha256 = if archive.sha256.starts_with("http") {
            let sha256 = download_string(archive.sha256.as_str())
                .context(format_context!("Failed to download {}", archive.sha256))?;
            if sha256.len() != 64 {
                return Err(format_error!(
                    "Invalid sha256 checksum {sha256} for {}",
                    archive.url
                ));
            }
            sha256
        } else {
            archive.sha256.clone()
        };

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
            Err(_) => format!("{full_path_to_archive}/{archive_file_name}"),
        };

        let mut archive = archive.clone();
        archive.sha256 = effective_sha256;

        Ok(Self {
            archive,
            archive_driver,
            full_path_to_archive,
            spaces_key: spaces_key.to_string(),
            allow_gh_for_download: true,
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
            if let Some(includes) = self.archive.includes.as_ref() {
                is_match = false;
                for pattern in includes {
                    if glob_match::glob_match(pattern, file) {
                        is_match = true;
                        break;
                    }
                }
            }
            if is_match {
                files.push(file);
            }
        }

        if let Some(excludes) = self.archive.excludes.as_ref() {
            for pattern in excludes {
                files.retain(|file| !glob_match::glob_match(pattern, file));
            }
        }

        let target_prefix = if let Some(add_prefix) = self.archive.add_prefix.as_ref() {
            if add_prefix.starts_with("//") {
                format!("{workspace_directory}/{add_prefix}")
            } else if add_prefix.starts_with("/") {
                add_prefix.to_owned()
            } else {
                format!("{workspace_directory}/{add_prefix}")
            }
        } else {
            format!("{workspace_directory}/{space_directory}")
        };

        progress_bar.set_total(files.len() as u64);

        for file in files {
            let source = format!("{}/{}", self.get_path_to_extracted_files(), file);

            progress_bar.set_message(file.as_str());
            let relative_target_path =
                if let Some(strip_prefix) = self.archive.strip_prefix.as_ref() {
                    file.strip_prefix(strip_prefix)
                } else {
                    Some(file.as_str())
                };

            if let Some(relative_target_path) = relative_target_path {
                let full_target_path = format!("{}/{}", target_prefix, relative_target_path);

                match self.archive.link {
                    ArchiveLink::Hard => {
                        Self::create_hard_link(full_target_path.clone(), source.clone()).context(
                            format_context!("hard link {full_target_path} -> {source}",),
                        )?;
                    }
                    ArchiveLink::None => (),
                }
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

        let next_progress_bar = if self.is_download_required() {
            if let Some(arguments) = transform_url_to_gh_arguments(
                self.allow_gh_for_download,
                self.archive.url.as_str(),
                &self.full_path_to_archive,
            ) {
                let options = printer::ExecuteOptions {
                    arguments,
                    ..Default::default()
                };

                progress_bar.log(
                    printer::Level::Trace,
                    format!("Downloading using gh {options:?}").as_str(),
                );

                progress_bar
                    .execute_process("gh", options)
                    .context(format_context!(
                        "failed to download {} using gh",
                        self.archive.url
                    ))?;

                progress_bar
            } else {
                let join_handle = self
                    .download(&runtime, progress_bar)
                    .context(format_context!("Failed to download using reqwest"))?;
                runtime.block_on(join_handle)??
            }
        } else {
            progress_bar
        };

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
            self.archive.url.as_str(),
            full_path_to_archive.as_str(),
            runtime,
            progress,
        )
    }

    fn save_files_json(&self, files: Files) -> anyhow::Result<()> {
        let file_path = self.get_path_to_extracted_files_json();
        let contents = serde_json::to_string_pretty(&files)?;
        std::fs::write(file_path, contents)?;
        Ok(())
    }

    fn load_files_json(&self) -> anyhow::Result<HashSet<String>> {
        let file_path = self.get_path_to_extracted_files_json();
        let contents = std::fs::read_to_string(file_path.as_str())
            .context(format_context!("while reading {file_path}"))?;
        let files: Files = serde_json::from_str(contents.as_str())
            .context(format_context!("while parsing {file_path}"))?;
        Ok(files.files)
    }

    fn extract(
        &self,
        progress_bar: printer::MultiProgressBar,
    ) -> anyhow::Result<printer::MultiProgressBar> {
        if !self.is_extract_required() {
            return Ok(progress_bar);
        }

        std::fs::create_dir_all(self.get_path_to_extracted_files().as_str())
            .context(format_context!("creating {}", self.full_path_to_archive))?;

        let mut extracted_files = HashSet::new();

        let next_progress_bar = if self.archive_driver.is_some() {
            let decoder = easy_archiver::Decoder::new(
                self.full_path_to_archive.as_str(),
                Some(self.archive.sha256.clone()),
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
            files: extracted_files,
        })
        .context(format_context!("Failed to save json files manifest"))?;
        Ok(next_progress_bar)
    }

    fn url_to_relative_path(url: &str, filename: &Option<String>) -> anyhow::Result<String> {
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
