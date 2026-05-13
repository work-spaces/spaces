use crate::driver::{self, Driver, UpdateStatus, SEVEN_Z_TAR_FILENAME};
use anyhow::Context;
use anyhow_source_location::format_context;

pub struct Entry {
    pub archive_path: String,
    pub file_path: String,
}

enum EncoderDriver {
    Gzip(tar::Builder<Vec<u8>>),
    Bzip2(tar::Builder<Vec<u8>>),
    Xz(tar::Builder<Vec<u8>>),
    Zip(Box<zip::ZipWriter<std::fs::File>>),
    SevenZ(tar::Builder<Vec<u8>>),
}

pub struct Digestable {
    path: String,
    progress_bar: console::Progress,
}

pub struct Digested {
    pub sha256: String,
    pub progress_bar: console::Progress,
}

impl Digestable {
    pub fn digest(self) -> anyhow::Result<Digested> {
        let mut progress_bar = self.progress_bar;

        let digest = driver::digest_file(self.path.as_str(), &mut progress_bar);

        Ok(Digested {
            sha256: digest?,
            progress_bar,
        })
    }
}

pub struct Encoder {
    encoder: EncoderDriver,
    driver: Driver,
    output_directory: String,
    output_filename: String,
    progress: console::Progress,
}

impl Encoder {
    fn get_output_file_path(output_directory: &str, output_filename: &str) -> String {
        format!("{output_directory}/{output_filename}")
    }

    fn get_encoder_output_file_path(&self) -> String {
        Self::get_output_file_path(
            self.output_directory.as_str(),
            self.output_filename.as_str(),
        )
    }

    #[allow(unused)]
    fn update_status(&mut self, update_status: UpdateStatus) {
        driver::update_status(&mut self.progress, update_status);
    }

    pub fn new(
        output_directory: &str,
        output_filename: &str,
        progress: console::Progress,
    ) -> anyhow::Result<Self> {
        let driver = Driver::from_filename(output_filename).ok_or(anyhow::anyhow!(
            "could not determine compression type from {output_filename} suffix"
        ))?;

        let encoder = match driver {
            Driver::Gzip => {
                let archiver = tar::Builder::new(Vec::new());
                EncoderDriver::Gzip(archiver)
            }
            Driver::Zip => {
                let file_path = Self::get_output_file_path(output_directory, output_filename);
                let file = std::fs::File::create(file_path.as_str())
                    .context(format_context!("{file_path}"))?;
                let encoder = zip::ZipWriter::new(file);
                EncoderDriver::Zip(Box::new(encoder))
            }
            Driver::Bzip2 => {
                let archiver = tar::Builder::new(Vec::new());
                EncoderDriver::Bzip2(archiver)
            }
            Driver::Xz => {
                let archiver = tar::Builder::new(Vec::new());
                EncoderDriver::Xz(archiver)
            }
            Driver::SevenZ => {
                let archiver = tar::Builder::new(Vec::new());
                EncoderDriver::SevenZ(archiver)
            }
        };

        Ok(Self {
            encoder,
            driver,
            output_directory: output_directory.to_string(),
            output_filename: output_filename.to_string(),
            progress,
        })
    }

    pub fn add_entries(&mut self, entries: &[Entry]) -> anyhow::Result<()> {
        self.update_status(UpdateStatus {
            detail: Some("Archiving...".to_string()),
            ..Default::default()
        });

        for entry in entries.iter() {
            self.update_status(UpdateStatus {
                detail: Some(entry.archive_path.clone()),
                increment: Some(1),
                total: Some(entries.len() as u64),
                ..Default::default()
            });

            self.add_file(&entry.archive_path, &entry.file_path)
                .context(format_context!("{}", entry.archive_path))?;
        }

        self.update_status(UpdateStatus {
            detail: Some("...".to_string()),
            ..Default::default()
        });

        Ok(())
    }

    pub fn add_file(&mut self, archive_path: &str, file_path: &str) -> anyhow::Result<()> {
        match &mut self.encoder {
            EncoderDriver::Gzip(archiver)
            | EncoderDriver::Bzip2(archiver)
            | EncoderDriver::Xz(archiver)
            | EncoderDriver::SevenZ(archiver) => {
                let path = std::path::Path::new(file_path);
                if path.is_symlink() {
                    let target = path
                        .read_link()
                        .context(format_context!("failed to read symlink {file_path}"))?;
                    let mut header = tar::Header::new_gnu();
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_size(0);
                    let metadata = std::fs::symlink_metadata(file_path)
                        .context(format_context!("{file_path}"))?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::{MetadataExt, PermissionsExt};
                        header.set_mode(metadata.permissions().mode());
                        header.set_mtime(metadata.mtime() as u64);
                    }

                    archiver
                        .append_link(&mut header, archive_path, target)
                        .context(format_context!("Failed to append symlink {file_path}"))?;
                } else {
                    let mut file =
                        std::fs::File::open(file_path).context(format_context!("{file_path}"))?;
                    archiver
                        .append_file(archive_path, &mut file)
                        .context(format_context!("appending {archive_path}"))?;
                }
            }
            EncoderDriver::Zip(encoder) => {
                let path = std::path::Path::new(file_path);
                if path.is_symlink() {
                    let target = path
                        .read_link()
                        .context(format_context!("failed to read symlink {file_path}"))?;
                    encoder
                        .add_symlink(
                            archive_path,
                            target.to_string_lossy(),
                            zip::write::SimpleFileOptions::default(),
                        )
                        .context(format_context!("failed to add symlink {file_path}"))?;
                } else {
                    let options = zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated)
                        .unix_permissions(0o755);
                    encoder
                        .start_file(archive_path, options)
                        .context(format_context!("{file_path}"))?;
                    let mut file = std::fs::File::open(file_path).context(format_context!(
                        "Failed to open file for zip archive {file_path}"
                    ))?;
                    std::io::copy(&mut file, encoder.as_mut()).context(format_context!(
                        "Failed to copy file to zip archive {file_path}"
                    ))?;
                }
            }
        }
        Ok(())
    }

    fn encode_in_chunks<Encoder: std::io::Write>(
        archiver: tar::Builder<Vec<u8>>,
        mut encoder: Encoder,
        driver: Driver,
        progress: &mut console::Progress,
    ) -> anyhow::Result<()> {
        let contents = archiver
            .into_inner()
            .context(format_context!("{driver:?}"))?;

        const CHUNK_SIZE: usize = 4096;
        let total_steps = contents.len().div_ceil(CHUNK_SIZE) as u64;

        driver::update_status(
            progress,
            UpdateStatus {
                detail: Some("Compressing...".to_string()),
                ..Default::default()
            },
        );

        for chunk in contents.as_slice().chunks(CHUNK_SIZE) {
            driver::update_status(
                progress,
                UpdateStatus {
                    increment: Some(1),
                    total: Some(total_steps),
                    ..Default::default()
                },
            );

            encoder
                .write_all(chunk)
                .context(format_context!("encoder with driver {driver:?} failed"))?;
        }
        Ok(())
    }

    pub fn compress(self) -> anyhow::Result<Digestable> {
        let driver = self.driver;
        let output_directory = self.output_directory.clone();
        let output_path = self.get_encoder_output_file_path();
        let output_path_result = output_path.clone();
        let mut progress_bar = self.progress;

        match self.encoder {
            EncoderDriver::Gzip(archiver) => {
                let output_file = std::fs::File::create(output_path.as_str())
                    .context(format_context!("cannot create {output_path}"))?;
                let encoder =
                    flate2::write::GzEncoder::new(output_file, flate2::Compression::default());
                Self::encode_in_chunks(archiver, encoder, driver, &mut progress_bar)?;
            }
            EncoderDriver::Zip(encoder) => {
                encoder.finish().context(format_context!("{output_path}"))?;
            }
            EncoderDriver::Xz(archiver) => {
                let output_file = std::fs::File::create(output_path.as_str())
                    .context(format_context!("{output_path}"))?;
                let encoder = xz2::write::XzEncoder::new(output_file, 9);
                Self::encode_in_chunks(archiver, encoder, driver, &mut progress_bar)?;
            }
            EncoderDriver::Bzip2(archiver) => {
                let output_file = std::fs::File::create(output_path.as_str())
                    .context(format_context!("{output_path}"))?;
                let encoder =
                    bzip2::write::BzEncoder::new(output_file, bzip2::Compression::default());
                Self::encode_in_chunks(archiver, encoder, driver, &mut progress_bar)?;
            }
            EncoderDriver::SevenZ(archiver) => {
                let contents = archiver.into_inner().context("tar.7z")?;

                driver::update_status(
                    &mut progress_bar,
                    UpdateStatus {
                        detail: Some("Compressing...".to_string()),
                        total: Some(200),
                        ..Default::default()
                    },
                );

                let handle = std::thread::spawn(move || -> anyhow::Result<()> {
                    let output_file = std::fs::File::create(output_path.as_str())
                        .context(format_context!("{output_path}"))?;

                    let temporary_tar_path = format!("{output_directory}/{SEVEN_Z_TAR_FILENAME}");
                    // create a temporary tar file
                    std::fs::write(temporary_tar_path.as_str(), contents)
                        .context(format_context!("{temporary_tar_path}"))?;

                    sevenz_rust::compress(temporary_tar_path.as_str(), output_file)
                        .context(format_context!("{temporary_tar_path} -> {output_path}"))?;

                    //std::fs::remove_file(temporary_tar_path.as_str()).context(format_context!(""))?;

                    Ok(())
                });

                driver::wait_handle(handle, &mut progress_bar).context(format_context!(""))?;
            }
        }
        Ok(Digestable {
            path: output_path_result,
            progress_bar,
        })
    }
}
