use anyhow_source_location::{format_context, format_error};
use std::collections::HashSet;
use std::io::Read;

use crate::driver::{self, Driver, UpdateStatus, SEVEN_Z_TAR_FILENAME};

use anyhow::Context;

enum DecoderDriver {
    Gzip(flate2::read::GzDecoder<std::fs::File>),
    Bzip2(bzip2::read::BzDecoder<std::fs::File>),
    Xz(xz2::read::XzDecoder<std::fs::File>),
    Zip(zip::ZipArchive<std::fs::File>),
    SevenZ,
}

pub struct Decoder {
    decoder: DecoderDriver,
    output_directory: String,
    input_file_name: String,
    reader_size: u64,
    driver: Driver,
    sha256: Option<String>,
    progress_bar: console::Progress,
}

pub struct Extracted {
    pub progress_bar: console::Progress,
    pub files: HashSet<String>,
}

impl Decoder {
    pub fn new(
        input_file_path: &str,
        sha256: Option<String>,
        destination_directory: &str,
        progress_bar: console::Progress,
    ) -> anyhow::Result<Self> {
        let driver =
            Driver::from_filename(input_file_path).context(format_context!("{input_file_path}"))?;

        let reader_size = std::path::Path::new(input_file_path)
            .metadata()
            .context(format_context!("{input_file_path}"))?
            .len();

        let input_file =
            std::fs::File::open(input_file_path).context(format_context!("{input_file_path}"))?;

        let decoder = match driver {
            Driver::Gzip => DecoderDriver::Gzip(flate2::read::GzDecoder::new(input_file)),
            Driver::Zip => DecoderDriver::Zip(
                zip::ZipArchive::new(input_file)
                    .context(format_context!("open zip failed: {input_file_path}"))?,
            ),
            Driver::Bzip2 => DecoderDriver::Bzip2(bzip2::read::BzDecoder::new(input_file)),
            Driver::Xz => DecoderDriver::Xz(xz2::read::XzDecoder::new(input_file)),
            Driver::SevenZ => DecoderDriver::SevenZ,
        };

        let output_directory = destination_directory.to_string();

        Ok(Self {
            decoder,
            output_directory,
            reader_size,
            input_file_name: input_file_path.to_string(),
            driver,
            sha256,
            progress_bar,
        })
    }

    fn extract_to_tar_bytes<Decoder: std::io::Read>(
        mut decoder: Decoder,
        reader_size: u64,
        _driver: Driver,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<Vec<u8>> {
        let mut result = Vec::with_capacity(reader_size as usize);
        let mut buffer = [0; 8192];

        driver::update_status(
            progress_bar,
            UpdateStatus {
                detail: Some("Decompressing...".to_string()),
                total: Some(200),
                ..Default::default()
            },
        );

        while let Ok(bytes_read) = decoder.read(&mut buffer) {
            if bytes_read == 0 {
                break;
            }
            result.extend_from_slice(&buffer[..bytes_read]);

            driver::update_status(
                progress_bar,
                UpdateStatus {
                    increment: Some(1),
                    ..Default::default()
                },
            );
        }

        Ok(result)
    }

    pub fn extract(self) -> anyhow::Result<Extracted> {
        let reader_size = self.reader_size;
        let driver = self.driver;
        let input_file: String = self.input_file_name.clone();
        let output_directory = self.output_directory.clone();

        let mut progress_bar = self.progress_bar;

        if let Some(digest) = self.sha256.as_ref() {
            let actual_digest = driver::digest_file(input_file.as_str(), &mut progress_bar)?;
            if actual_digest != *digest {
                return Err(format_error!(
                    "digest mismatch: expected: {} actual: {}",
                    digest,
                    actual_digest
                ));
            }
        }

        let tar_bytes = match self.decoder {
            DecoderDriver::Gzip(decoder) => Some(Self::extract_to_tar_bytes(
                decoder,
                reader_size,
                driver,
                &mut progress_bar,
            )?),
            DecoderDriver::Zip(mut decoder) => {
                let file_names: Vec<String> = decoder.file_names().map(|e| e.to_string()).collect();

                driver::update_status(
                    &mut progress_bar,
                    UpdateStatus {
                        detail: Some("Extracting...".to_string()),
                        total: Some(file_names.len() as u64),
                        ..Default::default()
                    },
                );

                for file in file_names {
                    let mut zip_file = decoder
                        .by_name(file.as_str())
                        .context(format_context!("{file:?}"))?;

                    driver::update_status(
                        &mut progress_bar,
                        UpdateStatus {
                            detail: Some(file.clone()),
                            increment: Some(1),
                            ..Default::default()
                        },
                    );

                    let mut buffer = Vec::new();
                    let destination_path = format!("{}/{}", self.output_directory, zip_file.name());
                    let dest_path = std::path::Path::new(destination_path.as_str());
                    if zip_file.is_dir() {
                        std::fs::create_dir_all(dest_path)
                            .context(format_context!("failed to create {destination_path}"))?;
                    } else if zip_file.is_symlink() {
                        #[cfg(unix)]
                        {
                            let dest_parent = dest_path
                                .parent()
                                .context(format_context!("{destination_path}"))?;
                            std::fs::create_dir_all(dest_parent)
                                .context(format_context!("failed to create {dest_parent:?}"))?;

                            zip_file.read_to_end(&mut buffer).context(format_context!(
                                "failed to read symlink target for {destination_path}"
                            ))?;
                            let target = std::str::from_utf8(&buffer).context(format_context!(
                                "symlink target is not valid UTF-8 for {destination_path}"
                            ))?;
                            // Remove any existing entry so reruns succeed.
                            let _ = std::fs::remove_file(dest_path);
                            std::os::unix::fs::symlink(target, dest_path).context(
                                format_context!(
                                    "failed to create symlink {destination_path} -> {target}"
                                ),
                            )?;
                        }
                    } else if zip_file.is_file() {
                        let dest_parent = dest_path
                            .parent()
                            .context(format_context!("{destination_path}"))?;
                        std::fs::create_dir_all(dest_parent)
                            .context(format_context!("failed to create {dest_parent:?}"))?;

                        let mut file = std::fs::File::create(destination_path.as_str())
                            .context(format_context!("failed to create {destination_path}"))?;
                        use std::io::Write;
                        zip_file.read_to_end(&mut buffer).context(format_context!(
                            "failed to read zip for {destination_path}"
                        ))?;
                        file.write(buffer.as_slice())
                            .context(format_context!("failed to write {destination_path}"))?;
                    }
                }

                None
            }
            DecoderDriver::Bzip2(decoder) => Some(Self::extract_to_tar_bytes(
                decoder,
                reader_size,
                driver,
                &mut progress_bar,
            )?),
            DecoderDriver::Xz(decoder) => Some(Self::extract_to_tar_bytes(
                decoder,
                reader_size,
                driver,
                &mut progress_bar,
            )?),
            DecoderDriver::SevenZ => {
                driver::update_status(
                    &mut progress_bar,
                    UpdateStatus {
                        detail: Some("Decompressing (7z)...".to_string()),
                        total: Some(200),
                        ..Default::default()
                    },
                );

                let handle = std::thread::spawn(move || -> anyhow::Result<Vec<u8>> {
                    let temporary_file_path = format!("{output_directory}/{SEVEN_Z_TAR_FILENAME}");
                    let input_file = std::fs::File::open(input_file.as_str())
                        .context(format_context!("{input_file}"))?;
                    sevenz_rust::decompress(input_file, output_directory.as_str()).context(
                        format_context!("{temporary_file_path} -> {output_directory}"),
                    )?;
                    let result = std::fs::read(temporary_file_path.as_str())
                        .context(format_context!("{temporary_file_path}"));

                    std::fs::remove_file(temporary_file_path.as_str())
                        .context(format_context!("{temporary_file_path}"))?;

                    result
                });

                let tar_contents =
                    driver::wait_handle(handle, &mut progress_bar).context(format_context!(""))?;

                Some(tar_contents)
            }
        };

        let output_directory = self.output_directory.clone();

        if let Some(tar_bytes) = tar_bytes {
            let handle = std::thread::spawn(move || -> anyhow::Result<()> {
                let mut archive = tar::Archive::new(tar_bytes.as_slice());
                archive
                    .unpack(output_directory.as_str())
                    .context(format_context!("{output_directory}"))?;

                Ok(())
            });

            driver::update_status(
                &mut progress_bar,
                UpdateStatus {
                    detail: Some("Unpacking...".to_string()),
                    ..Default::default()
                },
            );

            driver::wait_handle(handle, &mut progress_bar).context(format_context!(""))?;
        }

        let walk_dir: Vec<_> = walkdir::WalkDir::new(self.output_directory.as_str())
            .into_iter()
            .filter_map(|entry| entry.ok())
            .collect();

        let prefix = format!("{}/", self.output_directory);
        let mut files = HashSet::new();
        for entry in walk_dir {
            if entry.file_type().is_dir() {
                continue;
            }
            let full_path = entry.path().to_string_lossy().to_string();
            if let Some(relative_path) = full_path.strip_prefix(prefix.as_str()) {
                files.insert(relative_path.to_string());
            }
        }

        Ok(Extracted {
            progress_bar,
            files,
        })
    }
}
