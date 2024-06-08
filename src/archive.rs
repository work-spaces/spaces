use std::io::Read;

use crate::{
    context,
    context::{anyhow_error, format_error_context},
    manifest,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Serialize, Deserialize)]
struct Files {
    files: HashSet<String>,
}

pub struct HttpArchive {
    pub spaces_key: String,
    archive: manifest::Archive,
    full_path_to_archive: String,
    files: HashSet<String>,
    exectuables: Option<manifest::Executables>,
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
            exectuables: None,
            context,
        })
    }

    fn get_path_to_extracted_files(&self) -> String {
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

    fn create_links(&mut self, space_directory: &str) -> anyhow::Result<()> {
        let (_, target_path) = self.get_link_paths(space_directory);
        if std::path::Path::new(target_path.as_str()).exists() {
            return Ok(());
        }

        match self.archive.link {
            manifest::ArchiveLink::Soft => {
                self.create_soft_link(space_directory).with_context(|| {
                    format_error_context!("Failed to create soft links for {}", self.archive.url)
                })?;
            }
            manifest::ArchiveLink::Hard => {
                self.create_hard_links(space_directory).with_context(|| {
                    format_error_context!("Failed to create hard links for {}", self.archive.url)
                })?;
            }
        }

        Ok(())
    }

    fn create_soft_link(&self, space_directory: &str) -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let (source, target_path) = self.get_link_paths(space_directory);
        let target = std::path::Path::new(target_path.as_str());
        let original = std::path::Path::new(source.as_str());

        symlink(original, target)
            .with_context(|| format_error_context!("symlinking {} to {}", target_path, source))?;

        Ok(())
    }

    fn create_hard_link(target_path: String, source: String) -> anyhow::Result<()> {
        let target = std::path::Path::new(target_path.as_str());
        let original = std::path::Path::new(source.as_str());

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("when creating parent for hardlink {target_path} -> {source}")
            })?;
        }

        let _ = std::fs::remove_file(target);

        std::fs::hard_link(original, target)
            .with_context(|| format_error_context!("hardlinking {} -> {}", target_path, source))?;

        Ok(())
    }

    fn create_hard_links(&mut self, space_directory: &str) -> anyhow::Result<()> {
        let (source, target_path) = self.get_link_paths(space_directory);

        if self.files.is_empty() {
            self.load_files_json()?;
        }

        for file in self.files.iter() {
            let target_path = format!("{}/{}", target_path, file);
            let source = format!("{}/{}", source, file);

            Self::create_hard_link(target_path, source)
                .with_context(|| format_error_context!("while hardlinking archive file"))?;
        }

        if let Some(executables) = self.exectuables.as_ref() {
            if let Some(platform_archive) = executables.get_platform_archive() {
                let http_executables_archive = HttpArchive::new(
                    self.context.clone(),
                    space_directory,
                    &platform_archive.archive,
                )?;

                let source = http_executables_archive.get_path_to_extracted_files();

                for executable_path in platform_archive.executables.iter() {
                    let target_path = format!("{}/{}", target_path, executable_path);
                    let source = format!("{}/{}", source, executable_path);
                    Self::create_hard_link(target_path, source)
                        .with_context(|| format_error_context!("while hardlinking executable"))?;
                }
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
        let mut next_progress_bar = if self.is_download_required() {
            let join_handle = self.download(&context.async_runtime, progress_bar)?;
            context.async_runtime.block_on(join_handle)??
        } else {
            progress_bar
        };

        self.extract(&mut next_progress_bar)?;

        if let Some(platform_archive) = self
            .exectuables
            .as_ref()
            .and_then(|e| e.get_platform_archive())
        {
            let mut plaform_http_archive = HttpArchive::new(
                context.clone(),
                format!("{}_executables", self.spaces_key).as_str(),
                &platform_archive.archive,
            )?;

            let mut platform_progress_bar = if plaform_http_archive.is_download_required() {
                let join_handle =
                    plaform_http_archive.download(&context.async_runtime, next_progress_bar)?;
                context.async_runtime.block_on(join_handle)??
            } else {
                next_progress_bar
            };

            plaform_http_archive.extract(&mut platform_progress_bar)?;
        }

        self.create_links(full_path)?;

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

    fn extract_tar_archive(
        &mut self,
        progress: &mut printer::MultiProgressBar,
        contents: Vec<u8>,
    ) -> anyhow::Result<()> {
        let tar_contents_handle;
        {
            tar_contents_handle = std::thread::spawn(move || {
                let decoder = flate2::read::GzDecoder::new(contents.as_slice());
                std::io::BufReader::new(decoder)
                    .bytes()
                    .collect::<Result<Vec<u8>, std::io::Error>>()
                    .unwrap()
            });
            progress.set_prefix("Extracting");
            loop {
                if tar_contents_handle.is_finished() {
                    break;
                }
                progress.increment(1);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }

        let tar_contents = tar_contents_handle
            .join()
            .map_err(|_| anyhow_error!("Extract thread failed"))?;

        let mut tar_archive = tar::Archive::new(tar_contents.as_slice());
        let entries = tar_archive.entries()?;

        let output_folder = self.get_path_to_extracted_files();
        if !std::path::Path::new(output_folder.as_str()).exists() {
            std::fs::create_dir_all(output_folder.as_str())?;
        }

        for file in entries {
            if let Ok(mut file) = file {
                let file_path = file.path()?;
                let file_path_str = file_path
                    .to_str()
                    .ok_or(anyhow_error!("Internal Error: can't get path for tar file"))?;
                progress.set_message(file_path_str);

                let path = format!("{output_folder}/{file_path_str}",);

                match file.header().entry_type() {
                    tar::EntryType::Directory => {
                        let _ = std::fs::create_dir_all(path.as_str());
                    }
                    tar::EntryType::Regular => {
                        let file_name = std::path::Path::new(&path)
                            .file_name()
                            .ok_or(anyhow_error!("Internal Error: No file name found"))?
                            .to_str()
                            .ok_or(anyhow_error!("Internal Error: File is not a str"))?;

                        if !file_name.starts_with("._") {
                            self.files.insert(file_path_str.to_string());

                            use std::os::unix::fs::PermissionsExt;
                            let mut file_contents = Vec::new();
                            let _ = file.read_to_end(&mut file_contents);
                            if let Some(parent) = std::path::Path::new(&path).parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            let _ = std::fs::write(path.as_str(), file_contents.as_slice());
                            let mode = file.header().mode().unwrap_or(0o644);
                            let permissions = std::fs::Permissions::from_mode(mode);
                            let _ = std::fs::set_permissions(path.as_str(), permissions);
                        }
                    }
                    _ => {
                        //println!("Skipping {:?}", file.header().entry_type());
                    }
                }
                if file.header().entry_type() == tar::EntryType::Directory {
                    let _ = std::fs::create_dir_all(path.as_str());
                }
            }
            progress.increment(1);
        }
        Ok(())
    }

    fn extract_zip_archive(
        &mut self,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        use std::fs::File;
        use std::path::Path;
        use zip::read::ZipArchive;

        let destination_string = self.get_path_to_extracted_files();
        let destination = Path::new(destination_string.as_str());
        let archive_path = &self.full_path_to_archive;

        let error_context = format!("in zip archive {}", self.archive.url);

        let reader = File::open(archive_path)?;
        let mut archive = ZipArchive::new(reader)
            .with_context(|| format_error_context!("{}", error_context.clone()))?;

        progress.set_prefix("Extracting");

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .with_context(|| format_error_context!("{}", error_context.clone()))?;
            if let Some(file_name) = file.enclosed_name() {
                if let Some(file_name) = file_name.to_str() {
                    if file_name.starts_with("__MACOSX") {
                        continue;
                    }
                    if file_name.starts_with(".DS_Store") {
                        continue;
                    }

                    if file.is_file() {
                        self.files.insert(file_name.to_string());
                        progress.set_message(file_name);
                    }
                }

                let outpath = destination.join(file_name);
                if let Some(parent) = outpath.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                if file.is_file() {
                    let mut outfile = File::create(&outpath)
                        .with_context(|| format!("{} creating {outpath:?}", error_context))?;

                    use std::os::unix::fs::PermissionsExt;
                    outfile
                        .set_permissions(PermissionsExt::from_mode(
                            file.unix_mode().unwrap_or(0o644),
                        ))
                        .with_context(|| {
                            format!("{} setting permissions {outpath:?}", error_context)
                        })?;

                    std::io::copy(&mut file, &mut outfile)
                        .with_context(|| format!("{} copying {outpath:?}", error_context))?;
                }

                progress.increment(1);
            }
        }
        progress.set_message("Done!");

        Ok(())
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

    fn extract(&mut self, progress: &mut printer::MultiProgressBar) -> anyhow::Result<()> {
        if !self.is_extract_required() {
            self.exectuables =
                manifest::Executables::new(self.get_path_to_extracted_files().as_str())?;
            self.load_files_json()
                .with_context(|| format!("Missing {}", self.get_path_to_extracted_files_json()))?;
            return Ok(());
        }

        // check the digest
        let contents = {
            let full_path_to_archive = self.full_path_to_archive.clone();

            let contents = std::fs::read(full_path_to_archive)?;

            let digest_handle = std::thread::spawn(move || {
                let digest = sha256::digest(&contents);
                (digest, contents)
            });

            loop {
                if digest_handle.is_finished() {
                    break;
                }
                progress.increment(1);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            let (digest, contents) = digest_handle
                .join()
                .map_err(|_| anyhow_error!("Digest thread failed"))?;

            if digest != self.archive.sha256 {
                std::fs::remove_file(self.full_path_to_archive.as_str()).with_context(|| {
                    "Internal Error: failed to delete file with bad sha256".to_string()
                })?;

                return Err(anyhow_error!("Digest mismatch"));
            }
            contents
        };

        if self.archive.url.ends_with(".zip") {
            self.extract_zip_archive(progress)?;
        } else if self.archive.url.ends_with(".tar.gz") {
            self.extract_tar_archive(progress, contents)?;
        } else {
            return Err(anyhow_error!(
                "Unsupported archive format for {}",
                self.archive.url
            ));
        }

        self.save_files_json()?;
        self.exectuables = manifest::Executables::new(self.get_path_to_extracted_files().as_str())?;

        Ok(())
    }

    fn url_to_relative_path(url: &str) -> anyhow::Result<String> {
        let archive_url = url::Url::parse(url)
            .with_context(|| format!("Failed to parse bare store url {url}"))?;

        let host = archive_url
            .host_str()
            .ok_or(anyhow_error!("No host found in url {}", url))?;
        let scheme = archive_url.scheme();
        let path = archive_url.path();
        Ok(format!("{scheme}/{host}{path}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context;

    #[test]
    fn test_http_archive() {
        let mut context = std::sync::Arc::new(context::Context::default());

        let mut printer = context.printer.write().expect("Internal Error: No printer");

        let _archive = manifest::Archive {
            url: "https://github.com/StratifyLabs/SDK/releases/download/v8.3.1/arm-none-eabi-8-2019-q3-update-macos-x86_64.tar.gz".to_string(),
            sha256: "930dcd8b837916c82608bdf198d9f34f71deefd432024fe98271449b742a3623".to_string(),
            link: manifest::ArchiveLink::Hard,
        };

        let archive = manifest::Archive {
            url: "https://github.com/StratifyLabs/SDK/releases/download/v8.3.1/stratifyos-arm-none-eabi-libstd-8.3.1.zip".to_string(),
            sha256: "2b9cbca5867c70bf1f890f1dc25adfbe7ff08ef6ea385784b0e5877a298b7ff1".to_string(),
            link: manifest::ArchiveLink::Hard,
        };

        let mut multi_progress = printer::MultiProgress::new(&mut printer);
        let mut progress_bar = multi_progress.add_progress("test", Some(100), None);

        let mut http_archive = HttpArchive::new(context.clone(), "toolchain", &archive).unwrap();

        if http_archive.is_download_required() {
            let mut download_progress = multi_progress.add_progress("downloading", Some(100), None);
            let mut wait_progress = multi_progress.add_progress("waiting", None, None);
            let runtime = &context.async_runtime;

            let handle = http_archive.download(runtime, download_progress).unwrap();

            while !handle.is_finished() {
                wait_progress.increment(1);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        http_archive.extract(&mut progress_bar).unwrap();

        if http_archive.archive.link == manifest::ArchiveLink::Soft {
            http_archive.create_soft_link("tmp/toolchain").unwrap();
        } else {
            http_archive.create_hard_links("tmp").unwrap();
        }
    }
}
