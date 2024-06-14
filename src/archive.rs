use std::io::Read;

use crate::{
    context::{self, anyhow_error, format_error_context},
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

    fn create_links(&mut self, space_directory: &str) -> anyhow::Result<()> {
        match self.archive.link {
            manifest::ArchiveLink::Hard => {
                self.create_hard_links(space_directory).with_context(|| {
                    format_error_context!("Failed to create hard links for {}", self.archive.url)
                })?;
            }
            manifest::ArchiveLink::None => (),
        }

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
            if let Some(target_path) = self.transform_extracted_destination(&target_path, file) {
                let source = format!("{}/{}", source, file);

                if let Some(parent) = std::path::Path::new(&target_path).parent() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format_error_context!(
                            "while creating parent directory for hard link {target_path}"
                        )
                    })?;
                }

                Self::create_hard_link(target_path, source)
                    .with_context(|| format_error_context!("while hardlinking archive file"))?;
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

        self.extract(&mut next_progress_bar)
            .with_context(|| format_error_context!("failed to extract archive for {full_path}"))?;

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

        let mut target_path = target_path.to_owned();

        if let Some(add_prefix) = self.archive.add_prefix.as_ref() {
            if let (sysroot, true) = (
                self.context
                    .get_sysroot()
                    .expect("Internal Error: sysroot not set"),
                add_prefix.starts_with(manifest::SPACES_SYSROOT),
            ) {
                target_path = add_prefix.replace(manifest::SPACES_SYSROOT, sysroot.as_str());
            }
        }

        Some(format!("{target_path}/{path}"))
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
            .map_err(|_| anyhow_error!("Internal Error: Extract thread failed"))?;

        let output_folder = self.get_path_to_extracted_files();
        if !std::path::Path::new(output_folder.as_str()).exists() {
            std::fs::create_dir_all(output_folder.as_str()).with_context(|| {
                format_error_context!("Failed to create output folder {output_folder}")
            })?;
        }

        let mut tar_archive = tar::Archive::new(tar_contents.as_slice());
        let entries = tar_archive.entries()?;

        for file in entries {
            if let Ok(mut file) = file {
                let file_path = file.path()?;

                progress.set_message(file_path.to_str().unwrap_or("archive entry"));
                let path = std::path::Path::new(&output_folder).join(file_path);
                let path_string = path.display().to_string();

                match file.header().entry_type() {
                    tar::EntryType::Directory => {
                        let _ = std::fs::create_dir_all(&path);
                    }
                    tar::EntryType::Regular => {
                        let file_name = std::path::Path::new(&path)
                            .file_name()
                            .ok_or(anyhow_error!("Internal Error: No file name found"))?
                            .to_str()
                            .ok_or(anyhow_error!("Internal Error: File is not a str"))?;

                        if !file_name.starts_with("._") {
                            self.files.insert(path_string);

                            let mut file_contents = Vec::new();
                            let _ = file.read_to_end(&mut file_contents);
                            if let Some(parent) = std::path::Path::new(&path).parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            let _ = std::fs::write(&path, file_contents.as_slice());

                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::PermissionsExt;
                                let mode = file.header().mode().unwrap_or(0o644);
                                let permissions = std::fs::Permissions::from_mode(mode);
                                let _ = std::fs::set_permissions(&path, permissions);
                            }
                        }
                    }
                    _ => {
                        //println!("Skipping {:?}", file.header().entry_type());
                    }
                }

                if file.header().entry_type() == tar::EntryType::Directory {
                    let _ = std::fs::create_dir_all(&path);
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

                if file.is_file() {
                    let outpath = destination.join(file_name);
                    if let Some(parent) = outpath.parent() {
                        std::fs::create_dir_all(parent)?;
                    }

                    let mut outfile = File::create(&outpath)
                        .with_context(|| format!("{} creating {outpath:?}", error_context))?;

                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        outfile
                            .set_permissions(PermissionsExt::from_mode(
                                file.unix_mode().unwrap_or(0o644),
                            ))
                            .with_context(|| {
                                format!("{} setting permissions {outpath:?}", error_context)
                            })?;
                    }

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
            self.load_files_json()
                .with_context(|| format!("Missing {}", self.get_path_to_extracted_files_json()))?;
            return Ok(());
        }

        // check the digest
        let contents = {
            let full_path_to_archive = self.full_path_to_archive.clone();

            let contents = std::fs::read(full_path_to_archive.as_str())?;

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
                .map_err(|_| anyhow_error!("Internal error: Digest thread failed"))?;

            if digest != self.archive.sha256 {
                std::fs::remove_file(self.full_path_to_archive.as_str()).with_context(|| {
                    "Internal Error: failed to delete file with bad sha256".to_string()
                })?;

                return Err(anyhow_error!(
                    "Digest mismatch for {full_path_to_archive} != {digest}"
                ));
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

fn create_platform_archive(
    progress: &mut printer::MultiProgressBar,
    name: &str,
    path_to_files: &str,
    platform_archive: &manifest::Archive,
    platform: manifest::Platform,
) -> anyhow::Result<(manifest::Platform, String)> {
    //verify all the files exist
    if let Some(files) = platform_archive.files.as_ref() {
        for file in files.iter() {
            let full_path = format!("{path_to_files}/{file}");
            if !std::path::Path::new(full_path.as_str()).exists() {
                return Err(anyhow_error!("File {full_path} not found for {platform:?}"));
            }
        }

        let archive_name = format!("{name}-{platform}.zip");
        let mut archive = zip::ZipWriter::new(std::fs::File::create(archive_name.as_str())?);

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);

        for file in files.iter() {
            let full_path = format!("{path_to_files}/{file}");
            let relative_path_string = file;

            let file_contents = std::fs::read(full_path.as_str())
                .with_context(|| format_error_context!("Failed to read {full_path:?}"))?;

            archive
                .start_file(relative_path_string.to_owned(), options)
                .with_context(|| {
                    format_error_context!("Failed to start archive file {relative_path_string}")
                })?;

            use std::io::Write;
            archive
                .write_all(file_contents.as_slice())
                .with_context(|| {
                    format_error_context!(
                        "Failed to write contents of archive file {relative_path_string}"
                    )
                })?;

            progress.increment(1);
        }
        archive.finish()?;
        Ok((platform, archive_name))
    } else {
        Err(anyhow_error!("No files found for {platform:?}"))
    }
}

fn create_platform_archives(
    multi_progress: &mut printer::MultiProgress,
    config: &manifest::CreateArchive,
) -> anyhow::Result<()> {
    let mut deps = manifest::Deps::new(config.input.as_str())?
        .ok_or(anyhow_error!("no need to create platform archives"))
        .with_context(|| format_error_context!("while looking at deps in {}", config.output))?;

    let overlay_archive = deps
        .platform_archives
        .as_mut()
        .and_then(|e| e.get_mut(&config.platform_archives))
        .ok_or(anyhow_error!(
            "deps {} does not have a {}",
            config.output,
            config.platform_archives
        ))?;

    let mut handles = Vec::new();
    let combinations = &[
        (config.macos_x86_64.clone(), manifest::Platform::MacosX86_64),
        (
            config.macos_aarch64.clone(),
            manifest::Platform::MacosAarch64,
        ),
        (
            config.windows_x86_64.clone(),
            manifest::Platform::WindowsX86_64,
        ),
        (
            config.windows_aarch64.clone(),
            manifest::Platform::WindowsAarch64,
        ),
        (config.linux_x86_64.clone(), manifest::Platform::LinuxX86_64),
        (
            config.linux_aarch64.clone(),
            manifest::Platform::LinuxX86_64,
        ),
    ];

    for (platform_path, platform) in combinations.iter() {
        if let (Some(path), Some(platform_archive)) = (
            platform_path,
            overlay_archive.get_archive_from_platform(*platform),
        ) {
            let mut progress = multi_progress.add_progress(
                platform.to_string().as_str(),
                Some(
                    platform_archive
                        .files
                        .as_ref()
                        .map(|e| e.len())
                        .unwrap_or(100) as u64,
                ),
                None,
            );

            let platform = *platform;
            let path = path.to_owned();
            let name = config.output.to_owned();

            let handle = std::thread::spawn(move || {
                let result = create_platform_archive(
                    &mut progress,
                    name.as_str(),
                    path.as_str(),
                    &platform_archive,
                    platform,
                );
                result
            });

            handles.push(handle);
        }
    }

    for handle in handles {
        if let Ok(Ok((platform, archive))) = handle.join() {
            let contents = std::fs::read(archive.as_str())?;
            let digest = sha256::digest(contents);

            if let Some(archive) = overlay_archive.get_archive_from_platform_mut(platform) {
                archive.sha256 = digest;
            }
        } else {
            // failed to create archive
        }
    }

    deps.save(config.input.as_str()).with_context(|| {
        format_error_context!("Failed to save overlay deps for {}", config.input)
    })?;

    Ok(())
}

pub fn create(context: context::Context, config_path: String) -> anyhow::Result<()> {
    let config = manifest::CreateArchive::new(&config_path)
        .with_context(|| format_error_context!("While loading config path {config_path}"))?;

    let walk_dir: Vec<_> = walkdir::WalkDir::new(config.input.as_str())
        .into_iter()
        .filter_map(|entry| entry.ok())
        .collect();

    let mut printer = context
        .printer
        .write()
        .expect("Internal Error: Printer is not set");

    let arhive_name = format!("{}.zip", config.output);

    {
        let mut multi_progress = printer::MultiProgress::new(&mut printer);
        create_platform_archives(&mut multi_progress, &config)?;
    }

    {
        let mut multi_progress = printer::MultiProgress::new(&mut printer);
        let mut archive = zip::ZipWriter::new(std::fs::File::create(arhive_name.as_str())?);

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let walk_dir_list = walk_dir.iter().collect::<Vec<_>>();

        let mut progress = multi_progress.add_progress(
            config.output.as_str(),
            Some(walk_dir_list.len() as u64),
            None,
        );

        for entry in walk_dir_list {
            if entry.file_type().is_file() {
                let relative_path = entry
                    .path()
                    .strip_prefix(config.input.as_str())
                    .with_context(|| {
                        format_error_context!(
                            "Internal error: {:?} not stripped from {entry:?}",
                            config.input
                        )
                    })?;
                let relative_path_string = relative_path.to_str().with_context(|| {
                    format_error_context!("Internal error: {relative_path:?} not valid utf-8 str")
                })?;

                progress.set_message(relative_path_string);
                let full_path = entry.path();

                let file_contents = if let Some(files) = config.executables.as_ref() {
                    let is_executable = files
                        .iter()
                        .any(|entry| entry.as_str() == relative_path_string);

                    if is_executable {
                        "spaces platform placeholder"
                            .to_string()
                            .as_bytes()
                            .to_vec()
                    } else {
                        std::fs::read(full_path).with_context(|| {
                            format_error_context!("Failed to read {full_path:?}")
                        })?
                    }
                } else {
                    std::fs::read(full_path)
                        .with_context(|| format_error_context!("Failed to read {full_path:?}"))?
                };

                archive
                    .start_file(relative_path_string.to_owned(), options)
                    .with_context(|| {
                        format_error_context!("Failed to start archive file {relative_path_string}")
                    })?;

                use std::io::Write;
                archive
                    .write_all(file_contents.as_slice())
                    .with_context(|| {
                        format_error_context!(
                            "Failed to write contents of archive file {relative_path_string}"
                        )
                    })?;
            }
            progress.increment(1);
        }
        archive.finish()?;
    }

    let contents = std::fs::read(arhive_name.as_str())?;
    let digest = sha256::digest(contents);
    printer.info(config.output.as_str(), &digest)?;

    Ok(())
}

pub fn inspect(context: context::Context, path: String) -> anyhow::Result<()> {
    use std::fs::File;
    use zip::read::ZipArchive;

    let mut printer = context
        .printer
        .write()
        .expect("Internal Error: Printer is not set");

    let mut files: Vec<String> = Vec::new();

    {
        let mut multi_progress = printer::MultiProgress::new(&mut printer);

        let mut progress = multi_progress.add_progress(path.as_str(), Some(100), None);

        let archive_path = path.as_str();

        let reader = File::open(archive_path)?;
        let mut archive = ZipArchive::new(reader)
            .with_context(|| format_error_context!("failed to read zip file {archive_path}"))?;

        progress.set_prefix("Extracting");

        for i in 0..archive.len() {
            let file = archive
                .by_index(i)
                .with_context(|| format_error_context!("failed to get index for archive"))?;

            if let Some(file_name) = file.enclosed_name() {
                if let Some(file_name) = file_name.to_str() {
                    if file_name.starts_with("__MACOSX") {
                        continue;
                    }
                    if file_name.starts_with(".DS_Store") {
                        continue;
                    }

                    if file.is_file() {
                        files.push(format!("{}, {} bytes", file_name, file.size()));
                        progress.set_message(file_name);
                    }
                }
                progress.increment(1);
            }
        }
        progress.set_message("Done!");
    }

    printer.info("files", &files)?;

    Ok(())
}

pub fn create_binary_archive(
    context: context::Context,
    input: String,
    output: String,
    name: String,
    version: String,
) -> anyhow::Result<()> {
    let mut printer = context
        .printer
        .write()
        .expect("Internal Error: Printer is not set");

    let platform = manifest::Platform::get_platform()
        .ok_or(anyhow_error!("This platform is not supported"))?;

    let destination = format!("{output}/{name}-v{version}-{platform}.zip");

    if !std::path::Path::new(output.as_str()).exists() {
        return Err(anyhow_error!("Output directory {output} does not exist"));
    }

    {
        let file_contents = std::fs::read(input.as_str())
            .with_context(|| format_error_context!("failed to read input file {input}"))?;

        let input_path = std::path::Path::new(input.as_str());
        let input_file_name = input_path
            .file_name()
            .ok_or(anyhow_error!("Can't file a file name for {input}"))?
            .to_string_lossy();

        let mut archive = zip::ZipWriter::new(
            std::fs::File::create(destination.as_str())
                .with_context(|| format_error_context!("while creating {destination}"))?,
        );

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);

        archive
            .start_file(input_file_name.to_owned(), options)
            .with_context(|| format_error_context!("Failed to start archive file {input}"))?;

        use std::io::Write;
        archive
            .write_all(file_contents.as_slice())
            .with_context(|| {
                format_error_context!("Failed to write contents of archive file {input}")
            })?;

        archive.finish()?;
    }

    let contents = std::fs::read(destination.as_str())?;
    let digest = sha256::digest(contents);
    printer.info(&destination, &digest)?;

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
        let mut progress_bar = multi_progress.add_progress("test", Some(100), None);

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

        http_archive.extract(&mut progress_bar).unwrap();

        if http_archive.archive.link == manifest::ArchiveLink::Hard {
            http_archive.create_hard_links("tmp").unwrap();
        }
    }
}
