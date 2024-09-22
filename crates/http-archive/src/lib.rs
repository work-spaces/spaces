use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::io::AsyncWriteExt;
use std::sync::RwLock;
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
pub struct Archive {
    pub url: String,
    pub sha256: String,
    pub link: ArchiveLink,
    pub includes: Option<Vec<String>>,
    pub excludes: Option<Vec<String>>,
    pub strip_prefix: Option<String>,
    pub add_prefix: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Files {
    files: HashSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpArchive {
    pub spaces_key: String,
    archive: Archive,
    archive_driver: Option<easy_archiver::driver::Driver>,
    full_path_to_archive: String,
}

impl HttpArchive {
    pub fn new(bare_store_path: &str, spaces_key: &str, archive: &Archive) -> anyhow::Result<Self> {
        let relative_path = Self::url_to_relative_path(archive.url.as_str())
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

        let mut archive_driver = None;
        let full_path_to_archive = match archive_driver_result {
            Ok(driver) => {
                archive_driver = Some(driver);
                format!(
                    "{}/{}.{}",
                    full_path_to_archive,
                    archive.sha256,
                    driver.extension()
                )
            }
            Err(_) => format!("{full_path_to_archive}/{archive_file_name}"),
        };

        Ok(Self {
            archive: archive.clone(),
            archive_driver,
            full_path_to_archive,
            spaces_key: spaces_key.to_string(),
        })
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
            format!("{workspace_directory}/{add_prefix}")
        } else {
            format!("{workspace_directory}/{space_directory}")
        };
        progress_bar.set_prefix("linking");
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
        let _state = get_state().write().unwrap();

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .context(format_context!("{target_path} -> {source}"))?;
        }

        let _ = std::fs::remove_file(target);

        //if the source is a symlink, read the symlink and create a symlink
        if original.is_symlink() {
            //let link = std::fs::read_link(original).context(format_context!(
            //    "failed to read link {original:?} -> {target_path}"
            //))?;

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

    pub fn sync(&self, progress_bar: printer::MultiProgressBar) -> anyhow::Result<printer::MultiProgressBar> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .context(format_context!("Failed to create runtime"))?;

        let next_progress_bar = if self.is_download_required() {
            let join_handle = self.download(&runtime, progress_bar)?;
            runtime.block_on(join_handle)??
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
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<printer::MultiProgressBar>>> {
        let url = self.archive.url.clone();
        let full_path_to_archive = self.full_path_to_archive.clone();
        let full_path = std::path::Path::new(&full_path_to_archive);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let join_handle = runtime.spawn(async move {
            let client = reqwest::ClientBuilder::new()
                .redirect(reqwest::redirect::Policy::limited(16))
                .build()?;

            let mut response = client.get(&url).send().await?;
            let total_size = response.content_length().unwrap_or(0);
            progress.set_total(total_size);
            progress.set_message(url.as_str());

            let mut output_file = tokio::fs::File::create(full_path_to_archive.as_str()).await?;

            while let Some(chunk) = response.chunk().await? {
                progress.increment(chunk.len() as u64);
                output_file.write_all(&chunk).await?;
            }

            Ok(progress)
        });

        Ok(join_handle)
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
            //let path_to_extracted_files = self.get_path_to_extracted_files();

            //let path_to_files = std::path::Path::new(path_to_extracted_files.as_str());
            //let path_to_destination = path_to_files.join(file_name).to_string_lossy().to_string();

            extracted_files.insert(file_name.to_string_lossy().to_string());
            progress_bar
        };
        self.save_files_json(Files{files:extracted_files})
            .context(format_context!("Failed to save json files manifest"))?;
        Ok(next_progress_bar)
    }

    fn url_to_relative_path(url: &str) -> anyhow::Result<String> {
        let archive_url = url::Url::parse(url)
            .context(format_context!("Failed to parse bare store url {url}"))?;

        let host = archive_url
            .host_str()
            .ok_or(format_error!("No host found in url {}", url))?;
        let scheme = archive_url.scheme();
        let path = archive_url.path();
        Ok(format!("{scheme}/{host}{path}"))
    }
}
