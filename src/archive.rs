use std::io::Read;

use crate::{config::Printer, manifest};
use anyhow::Context;
use tokio::io::AsyncWriteExt;

fn get_platform(archive: &manifest::Archive) -> Option<&manifest::Platform> {
    cfg_if::cfg_if! {
        if #[cfg(all(target_os = "windows", target_arch = "x86_64"))] {
            return archive.windows_x86_64.as_ref();
        } else if #[cfg(all(target_os = "windows", target_arch = "aarch64"))] {
            return archive.windows_aarch64.as_ref();
        } else if #[cfg(all(target_os = "linux", target_arch = "x86_64"))] {
            return archive.linux_x86_64.as_ref();
        } else if #[cfg(all(target_os = "linux", target_arch = "aarch64"))] {
            return archive.linux_aarch64.as_ref();
        } else if #[cfg(all(target_os = "macos", target_arch = "aarch64"))] {
            return archive.macos_aarch64.as_ref();
        } else if #[cfg(all(target_os = "macos", target_arch = "x86_64"))] {
            return archive.macos_x86_64.as_ref();
        } else {
            return None;
        }
    }
}

pub struct HttpArchive {
    pub spaces_key: String,
    pub archive: manifest::Platform,
    pub full_path_to_archive: String,
}

impl HttpArchive {
    pub fn new(
        printer: &mut Printer,
        spaces_key: &str,
        archive: &manifest::Archive,
    ) -> anyhow::Result<Self> {
        let archive = get_platform(archive)
            .ok_or(anyhow::anyhow!("No platform found"))?
            .clone();

        let full_path_to_archive = printer
            .context()
            .get_bare_store_path(Self::url_to_relative_path(archive.url.as_str())?.as_str());

        let full_path_to_archive = format!("{}/{}", full_path_to_archive, archive.sha256);

        printer.info("path", &full_path_to_archive)?;

        Ok(Self {
            archive,
            full_path_to_archive,
            spaces_key: spaces_key.to_string(),
        })
    }

    fn get_path_to_extracted_files(&self) -> String {
        format!("{}_files", self.full_path_to_archive)
    }

    pub fn is_download_required(&self) -> bool {
        !std::path::Path::new(&self.full_path_to_archive).exists()
    }

    fn is_extract_required(&self) -> bool {
        !std::path::Path::new(self.get_path_to_extracted_files().as_str()).exists()
    }

    pub fn create_soft_link(&self, space_directory: &str) -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let target_path = format!("{space_directory}/{}", self.spaces_key);
        let target = std::path::Path::new(target_path.as_str());

        let source = self.get_path_to_extracted_files();
        let original = std::path::Path::new(source.as_str());

        symlink(original, target)?;
        Ok(())
    }

    pub fn download(
        &self,
        runtime: &tokio::runtime::Runtime,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<()>>> {
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

            let mut output_file = tokio::fs::File::create(full_path_to_archive.as_str()).await?;

            while let Some(chunk) = response.chunk().await? {
                progress.increment(chunk.len() as u64);
                output_file.write(&chunk).await?;
            }

            Ok(())
        });

        Ok(join_handle)
    }

    pub fn extract(&self, printer: &mut Printer) -> anyhow::Result<()> {
        if !self.is_extract_required() {
            return Ok(());
        }

        // check the digest
        let contents = {
            let full_path_to_archive = self.full_path_to_archive.clone();

            let contents = std::fs::read(&full_path_to_archive)?;

            let mut progress = printer::Progress::new(printer, "digesting", None)?;
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
                .map_err(|_| anyhow::anyhow!("Digest thread failed"))?;

            if digest != self.archive.sha256 {
                std::fs::remove_file(self.full_path_to_archive.as_str()).with_context(|| {
                    format!("Internal Error: failed to delete file with bad sha256")
                })?;

                return Err(anyhow::anyhow!("Digest mismatch"));
            }
            contents
        };

        let tar_contents_handle;
        {
            let mut progress = printer::Progress::new(printer, "extracting", None)?;

            tar_contents_handle = std::thread::spawn(move || {
                let decoder = flate2::read::GzDecoder::new(contents.as_slice());
                std::io::BufReader::new(decoder)
                    .bytes()
                    .collect::<Result<Vec<u8>, std::io::Error>>()
                    .unwrap()
            });

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
            .map_err(|_| anyhow::anyhow!("Extract thread failed"))?;

        let mut tar_archive = tar::Archive::new(tar_contents.as_slice());
        let entries = tar_archive.entries()?;

        let mut progress = printer::Progress::new(printer, "writing", None)?;

        let output_folder = self.get_path_to_extracted_files();
        if !std::path::Path::new(output_folder.as_str()).exists() {
            std::fs::create_dir_all(output_folder.as_str())?;
        }

        for file in entries {
            if let Ok(mut file) = file {
                let file_path = file.path()?;
                let file_path_str = file_path.to_str().ok_or(anyhow::anyhow!(
                    "Internal Error: can't get path for tar file"
                ))?;
                progress.set_message(file_path_str);

                let path = format!("{output_folder}/{file_path_str}",);

                match file.header().entry_type() {
                    tar::EntryType::Directory => {
                        let _ = std::fs::create_dir_all(path.as_str());
                    }
                    tar::EntryType::Regular => {
                        let file_name = std::path::Path::new(&path)
                            .file_name()
                            .ok_or(anyhow::anyhow!("Internal Error: No file name found"))?
                            .to_str()
                            .ok_or(anyhow::anyhow!("Internal Error: File is not a str"))?;

                        if !file_name.starts_with("._") {
                            use std::os::unix::fs::PermissionsExt;
                            let mut file_contents = Vec::new();
                            let _ = file.read_to_end(&mut file_contents);
                            let _ = std::fs::write(path.as_str(), file_contents.as_slice());
                            let mode = file.header().mode().unwrap_or(0o644);
                            let permissions = std::fs::Permissions::from_mode(mode);
                            let _ = std::fs::set_permissions(path.as_str(), permissions);
                        }
                    }
                    _ => {
                        println!("Skipping {:?}", file.header().entry_type());
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

    fn url_to_relative_path(url: &str) -> anyhow::Result<String> {
        let archive_url = url::Url::parse(url)
            .with_context(|| format!("Failed to parse bare store url {url}"))?;

        let host = archive_url
            .host_str()
            .ok_or(anyhow::anyhow!("No host found in url {}", url))?;
        let scheme = archive_url.scheme();
        let path = archive_url.path();
        Ok(format!("{scheme}/{host}{path}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    #[test]
    fn test_http_archive() {
        let mut printer = Printer::new_stdout(config::Config::new().unwrap());

        let archive = manifest::Archive {
            url: "https://github.com/StratifyLabs/SDK/releases/download/v8.3.1/arm-none-eabi-8-2019-q3-update-macos-x86_64.tar.gz".to_string(),
            sha256: "930dcd8b837916c82608bdf198d9f34f71deefd432024fe98271449b742a3623".to_string(),
        };

        let http_archive = HttpArchive::new(&mut printer, "toolchain", &archive).unwrap();

        if http_archive.is_download_required() {
            let mut multi_progress = printer::MultiProgress::new(&mut printer);

            let download_progress = multi_progress.add_progress("Downloading", Some(100));
            let mut wait_progress = multi_progress.add_progress("Waiting", None);
            let runtime = &printer.context().async_runtime;

            let handle = http_archive.download(runtime, download_progress).unwrap();

            while !handle.is_finished() {
                wait_progress.increment(1);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        http_archive.extract(&mut printer).unwrap();

        http_archive.create_soft_link("tmp/toolchain").unwrap();
    }
}
