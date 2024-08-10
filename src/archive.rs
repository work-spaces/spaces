use crate::{context, manifest};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Serialize, Deserialize)]
struct Files {
    files: HashSet<String>,
}

#[derive(Clone)]
pub struct HttpArchive {
    pub spaces_key: String,
    archive: manifest::Archive,
    full_path_to_archive: String,
    files: HashSet<String>,
    context: std::sync::Arc<context::Context>,
}

impl HttpArchive {
    pub fn new(
        context: std::sync::Arc<context::Context>,
        spaces_key: &str,
        archive: &manifest::Archive,
    ) -> anyhow::Result<Self> {
        let full_path_to_archive =
            context.get_bare_store_path(Self::url_to_relative_path(archive.url.as_str())?.as_str());

        let full_path_to_archive = format!("{}/{}", full_path_to_archive, archive.sha256);

        Ok(Self {
            archive: archive.clone(),
            full_path_to_archive,
            spaces_key: spaces_key.to_string(),
            files: HashSet::new(),
            context,
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

    fn get_link_paths(&self, space_directory: &str) -> (String, String) {
        let target_path = format!("{space_directory}/{}", self.spaces_key);
        let source = self.get_path_to_extracted_files();
        (source, target_path)
    }

    pub fn create_links(&mut self, space_directory: &str) -> anyhow::Result<()> {
        match self.archive.link {
            manifest::ArchiveLink::Hard => {
                self.create_hard_links(space_directory)
                    .context(format_context!(
                        "Failed to create hard links for {}",
                        self.archive.url
                    ))?;
            }
            manifest::ArchiveLink::None => (),
        }

        Ok(())
    }

    fn create_hard_link(target_path: String, source: String) -> anyhow::Result<()> {
        let target = std::path::Path::new(target_path.as_str());
        let original = std::path::Path::new(source.as_str());

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .context(format_context!("{target_path} -> {source}"))?;
        }

        let _ = std::fs::remove_file(target);

        std::fs::hard_link(original, target).context(format_context!(
            "{} -> {}",
            target_path,
            source
        ))?;

        Ok(())
    }

    fn create_hard_links(&mut self, space_directory: &str) -> anyhow::Result<()> {
        let (source, target_path) = self.get_link_paths(space_directory);

        if self.files.is_empty() {
            self.load_files_json()?;
        }

        for file in self.files.iter() {
            if let Some(target_path) = self.transform_extracted_destination(&target_path, file) {
                let source = format!("{}/{}", source, file);

                if let Some(parent) = std::path::Path::new(&target_path).parent() {
                    std::fs::create_dir_all(parent).context(format_context!(
                        "while creating parent directory for hard link {target_path}"
                    ))?;
                }

                Self::create_hard_link(target_path, source).context(format_context!(""))?;
            }
        }
        Ok(())
    }

    pub fn sync(
        &mut self,
        context: std::sync::Arc<context::Context>,
        full_path: &str,
        progress_bar: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let next_progress_bar = if self.is_download_required() {
            let join_handle = self.download(&context.async_runtime, progress_bar)?;
            context.async_runtime.block_on(join_handle)??
        } else {
            progress_bar
        };

        self.extract(next_progress_bar)
            .context(format_context!("failed to extract archive for {full_path}"))?;

        //self.create_links(full_path)?;

        Ok(())
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

    fn transform_extracted_destination(
        &self,
        target_path: &str,
        relative_path: &str,
    ) -> Option<String> {
        let mut path = relative_path.to_owned();

        if let Some(strip_prefix) = self.archive.strip_prefix.as_ref() {
            path = path.strip_prefix(strip_prefix).unwrap_or(&path).to_string();
        }

        if let Some(files) = self.archive.files.as_ref() {
            //this needs to check for a glob_match
            let mut is_match = false;
            for pattern in files {
                if glob_match::glob_match(pattern, &path) {
                    is_match = true;
                    break;
                }
            }
            if !is_match {
                return None;
            }
        }

        let target_path_with_prefix = if let Some(add_prefix) = self.archive.add_prefix.as_ref() {
            format!("{add_prefix}/{target_path}")
        } else {
            target_path.to_string()
        };

        let target_path = self
            .context
            .template_model
            .render_template_string(target_path_with_prefix.as_str())
            .ok()?;

        Some(format!("{target_path}/{path}"))
    }

    fn save_files_json(&self) -> anyhow::Result<()> {
        let files = Files {
            files: self.files.clone(),
        };

        let file_path = self.get_path_to_extracted_files_json();
        let contents = serde_json::to_string_pretty(&files)?;
        std::fs::write(file_path, contents)?;
        Ok(())
    }

    fn load_files_json(&mut self) -> anyhow::Result<()> {
        let file_path = self.get_path_to_extracted_files_json();
        let contents = std::fs::read_to_string(file_path)?;
        let files: Files = serde_json::from_str(contents.as_str())?;
        self.files = files.files;

        Ok(())
    }

    fn extract(
        &mut self,
        progress_bar: printer::MultiProgressBar,
    ) -> anyhow::Result<printer::MultiProgressBar> {
        if !self.is_extract_required() {
            self.load_files_json().context(format_context!(
                "Missing {}",
                self.get_path_to_extracted_files_json()
            ))?;
            return Ok(progress_bar);
        }

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

        self.save_files_json()?;

        Ok(extracted.progress_bar)
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

pub fn create(
    execution_context: context::ExecutionContext,
    manifest_path: String,
) -> anyhow::Result<()> {
    let config = manifest::CreateArchive::new(&manifest_path)
        .context(format_context!("While loading config path {manifest_path}"))?;

    let mut printer = execution_context.printer;

    let walk_dir: Vec<_> = walkdir::WalkDir::new(config.input.as_str())
        .into_iter()
        .filter_map(|entry| entry.ok())
        .collect();

    let output_path_string = config.get_output_file();
    let output_path = std::path::Path::new(output_path_string.as_str());
    let output_directory = output_path
        .parent()
        .context(format_context!("{output_path_string}"))?
        .to_string_lossy()
        .to_string();
    let output_file_name = output_path
        .file_name()
        .context(format_context!("{output_path_string}"))?
        .to_string_lossy()
        .to_string();

    std::fs::create_dir_all(output_directory.clone())?;

    let mut multi_progress = printer::MultiProgress::new(&mut printer);
    let progress_bar = multi_progress.add_progress("Archiving", Some(100), None);

    let mut encoder = easy_archiver::Encoder::new(
        output_directory.as_str(),
        output_file_name.as_str(),
        progress_bar,
    )
    .context(format_context!("{output_path_string}"))?;

    for item in walk_dir {
        let archive_path = item
            .path()
            .strip_prefix(config.input.as_str())
            .context(format_context!("{item:?}"))?
            .to_string_lossy()
            .to_string();

        let file_path = item.path().to_string_lossy().to_string();

        encoder
            .add_file(archive_path.as_str(), file_path.as_str())
            .context(format_context!("{output_directory}"))?;
    }

    let digestable = encoder
        .compress()
        .context(format_context!("{output_directory}"))?;

    let digest = digestable
        .digest()
        .context(format_context!("{output_directory}"))?;

    printer.info(config.output.as_str(), &digest.sha256)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context;

    #[test]
    fn test_http_archive() {
        let context = std::sync::Arc::new(context::Context::new().unwrap());

        let mut printer = context.printer.write().expect("Internal Error: No printer");

        let _archive = manifest::Archive {
            url: "https://github.com/StratifyLabs/SDK/releases/download/v8.3.1/arm-none-eabi-8-2019-q3-update-macos-x86_64.tar.gz".to_string(),
            sha256: "930dcd8b837916c82608bdf198d9f34f71deefd432024fe98271449b742a3623".to_string(),
            link: manifest::ArchiveLink::Hard,
            files: None,
            add_prefix: None,
            strip_prefix: None
        };

        let archive = manifest::Archive {
            url: "https://github.com/StratifyLabs/SDK/releases/download/v8.3.1/stratifyos-arm-none-eabi-libstd-8.3.1.zip".to_string(),
            sha256: "2b9cbca5867c70bf1f890f1dc25adfbe7ff08ef6ea385784b0e5877a298b7ff1".to_string(),
            link: manifest::ArchiveLink::Hard,
            files: None,
            add_prefix: None,
            strip_prefix: None
        };

        let mut multi_progress = printer::MultiProgress::new(&mut printer);
        let progress_bar = multi_progress.add_progress("test", Some(100), None);

        let mut http_archive = HttpArchive::new(context.clone(), "toolchain", &archive).unwrap();

        if http_archive.is_download_required() {
            let download_progress = multi_progress.add_progress("downloading", Some(100), None);
            let mut wait_progress = multi_progress.add_progress("waiting", None, None);
            let runtime = &context.async_runtime;

            let handle = http_archive.download(runtime, download_progress).unwrap();

            while !handle.is_finished() {
                wait_progress.increment(1);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        http_archive.extract(progress_bar).unwrap();

        if http_archive.archive.link == manifest::ArchiveLink::Hard {
            http_archive.create_hard_links("tmp").unwrap();
        }
    }
}
