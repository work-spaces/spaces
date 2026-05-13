use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};

pub mod decoder;
pub mod driver;
pub mod encoder;

pub use decoder::Decoder;
pub use driver::UpdateStatus;
pub use encoder::Encoder;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateArchive {
    pub input: String,
    pub name: String,
    pub version: String,
    pub driver: driver::Driver,
    pub platform: Option<String>,
    pub includes: Option<Vec<String>>,
    pub excludes: Option<Vec<String>>,
}

impl CreateArchive {
    pub fn get_output_file(&self) -> String {
        let mut result = format!("{}-v{}", self.name, self.version);
        if let Some(platform) = self.platform.as_ref() {
            result.push_str(format!("-{platform}").as_str());
        }
        result.push('.');
        result.push_str(self.driver.extension().as_str());
        result
    }

    pub fn build_file_list(&self) -> anyhow::Result<Vec<(String, String)>> {
        let input_as_path = std::path::Path::new(self.input.as_str());

        let strip_prefix = if input_as_path.is_dir() {
            self.input.clone()
        } else if let Some(parent) = input_as_path.parent() {
            parent.to_string_lossy().to_string()
        } else {
            "".to_string()
        };

        let walk_dir: Vec<_> = walkdir::WalkDir::new(self.input.as_str())
            .into_iter()
            .filter_map(|entry| entry.ok())
            .collect();

        let mut all_files = Vec::new();

        for item in walk_dir {
            if item.file_type().is_dir() {
                continue;
            }
            let archive_path = item
                .path()
                .strip_prefix(strip_prefix.as_str())
                .context(format_context!("{item:?}"))?
                .to_string_lossy()
                .to_string();

            let file_path = item.path().to_string_lossy().to_string();
            all_files.push((archive_path, file_path));
        }

        let mut files = Vec::new();

        for (archive_path, file_path) in all_files.iter() {
            let mut is_match = true;
            if let Some(includes) = self.includes.as_ref() {
                is_match = false;
                for pattern in includes {
                    if glob_match::glob_match(pattern, archive_path) {
                        is_match = true;
                        break;
                    }
                }
            }
            if is_match {
                files.push((archive_path.clone(), file_path.clone()));
            }
        }

        if let Some(excludes) = self.excludes.as_ref() {
            for pattern in excludes {
                files.retain(|file| !glob_match::glob_match(pattern, &file.0));
            }
        }

        Ok(files)
    }

    pub fn create(
        &self,
        output_directory: &str,
        progress: console::Progress,
    ) -> anyhow::Result<(String, String)> {
        let output_file_name = self.get_output_file();

        std::fs::create_dir_all(output_directory)
            .context(format_context!("failed to create {output_directory}"))?;

        let output_file_path = format!("{output_directory}/{output_file_name}");

        let files = self
            .build_file_list()
            .context(format_error!("Failed to build file list"))?;

        if files.is_empty() {
            return Err(format_error!(
                "No files to archive for {} in {}",
                self.name,
                self.input
            ));
        }

        let mut encoder = Encoder::new(output_directory, output_file_name.as_str(), progress)
            .context(format_context!("{output_file_path}"))?;

        for (archive_path, file_path) in files {
            encoder
                .add_file(archive_path.as_str(), file_path.as_str())
                .context(format_context!("{output_directory}"))?;
        }

        let digestable = encoder
            .compress()
            .context(format_context!("{output_directory}"))?;

        let mut digest = digestable
            .digest()
            .context(format_context!("{output_directory}"))?;

        digest
            .progress_bar
            .set_finalize_lines(console::make_finalize_line(
                console::FinalType::Completed,
                digest.progress_bar.elapsed(),
                &output_file_name,
            ));

        Ok((output_file_path, digest.sha256))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    const FILE_COUNT: usize = 500;
    const LINE_COUNT: usize = 500;

    fn verify_generated_files(output_directory: &str) {
        for i in 0..FILE_COUNT {
            let archive_path = format!("file_{i}.txt");
            let file_path = format!("{output_directory}/{archive_path}");
            let contents = std::fs::read_to_string(file_path.as_str()).unwrap();

            for (number, line) in contents.lines().enumerate() {
                let expected = format!("This is line #{number}");
                assert_eq!(line, expected);
            }
        }
    }

    fn generate_tmp_files() -> Vec<encoder::Entry> {
        let mut result = Vec::new();
        std::fs::create_dir_all("tmp/files").unwrap();
        for i in 0..FILE_COUNT {
            let archive_path = format!("file_{i}.txt");
            let file_path = format!("tmp/files/{archive_path}");
            let path = std::path::Path::new(file_path.as_str());
            let mut file = if !path.exists() {
                Some(std::fs::File::create(file_path.as_str()).unwrap())
            } else {
                None
            };
            result.push(encoder::Entry {
                archive_path,
                file_path,
            });

            if let Some(file) = file.as_mut() {
                for j in 0..LINE_COUNT {
                    file.write_all(format!("This is line #{j}\n").as_bytes())
                        .unwrap();
                }
            }
        }
        result
    }

    fn unique_test_path(name: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("spaces-archiver-{name}-{suffix}"))
    }

    #[test]
    fn test_file_list() {
        fn contains(files: &[(String, String)], archive_path: &str) -> bool {
            files.iter().any(|(a, _)| a == archive_path)
        }

        let mut create_archive = CreateArchive {
            input: "test".to_string(),
            name: "test-output".to_string(),
            version: "1.0".to_string(),
            driver: driver::Driver::Gzip,
            platform: None,
            includes: None,
            excludes: Some(vec!["*.txt".to_string()]),
        };

        let files = create_archive.build_file_list().unwrap();
        assert!(contains(&files, "a/a.txt"));
        assert!(contains(&files, "a/b.txt"));
        assert!(contains(&files, "b/a.txt"));
        assert!(contains(&files, "b/b.txt"));
        assert!(!contains(&files, "a.txt"));
        assert!(!contains(&files, "b.txt"));
        assert_eq!(files.len(), 4);

        create_archive.excludes = Some(vec!["a/*".to_string()]);
        let files = create_archive.build_file_list().unwrap();
        assert!(!contains(&files, "a/a.txt"));
        assert!(!contains(&files, "a/b.txt"));
        assert!(contains(&files, "b/a.txt"));
        assert!(contains(&files, "b/b.txt"));
        assert!(contains(&files, "a.txt"));
        assert!(contains(&files, "b.txt"));
        assert_eq!(files.len(), 4);

        create_archive.includes = Some(vec!["a/*".to_string()]);
        create_archive.excludes = None;
        let files = create_archive.build_file_list().unwrap();
        assert!(contains(&files, "a/a.txt"));
        assert!(contains(&files, "a/b.txt"));
        assert!(!contains(&files, "b/a.txt"));
        assert!(!contains(&files, "b/b.txt"));
        assert!(!contains(&files, "a.txt"));
        assert!(!contains(&files, "b.txt"));
        assert_eq!(files.len(), 2);

        create_archive.includes = None;
        create_archive.excludes = None;
        let files = create_archive.build_file_list().unwrap();
        assert!(contains(&files, "a/a.txt"));
        assert!(contains(&files, "a/b.txt"));
        assert!(contains(&files, "b/a.txt"));
        assert!(contains(&files, "b/b.txt"));
        assert!(contains(&files, "a.txt"));
        assert!(contains(&files, "b.txt"));
        assert_eq!(files.len(), 6);

        create_archive.includes = Some(vec!["b/*".to_string()]);
        create_archive.excludes = None;
        let files = create_archive.build_file_list().unwrap();
        assert!(!contains(&files, "a/a.txt"));
        assert!(!contains(&files, "a/b.txt"));
        assert!(contains(&files, "b/a.txt"));
        assert!(contains(&files, "b/b.txt"));
        assert!(!contains(&files, "a.txt"));
        assert!(!contains(&files, "b.txt"));
        assert_eq!(files.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_test() {
        std::fs::create_dir_all("tmp/symlink_src").unwrap();

        let real_file_path = "tmp/symlink_src/real_file.txt";
        std::fs::write(real_file_path, "symlink target content\n").unwrap();

        let symlink_path = "tmp/symlink_src/link_to_file.txt";
        let _ = std::fs::remove_file(symlink_path);
        std::os::unix::fs::symlink("real_file.txt", symlink_path).unwrap();

        let entries = vec![
            encoder::Entry {
                archive_path: "real_file.txt".to_string(),
                file_path: real_file_path.to_string(),
            },
            encoder::Entry {
                archive_path: "link_to_file.txt".to_string(),
                file_path: symlink_path.to_string(),
            },
        ];

        let console = console::Console::new_stdout(console::Verbosity::default()).unwrap();

        const DRIVERS: &[driver::Driver] = &[
            driver::Driver::Gzip,
            driver::Driver::Bzip2,
            driver::Driver::Zip,
            driver::Driver::SevenZ,
            driver::Driver::Xz,
        ];

        for driver in DRIVERS {
            let output_directory = "./tmp";
            let output_filename = format!("symlink_test.{}", driver.extension());

            let progress_bar =
                console::Progress::new(console.clone(), output_filename.as_str(), None, None);

            let mut encoder =
                encoder::Encoder::new(output_directory, &output_filename, progress_bar).unwrap();

            encoder.add_entries(&entries).unwrap();
            let _digest = encoder.compress().unwrap().digest().unwrap();
        }

        for driver in DRIVERS {
            let output_dir = format!("tmp/symlink_extract.{}", driver.extension());
            std::fs::create_dir_all(output_dir.as_str()).unwrap();

            let archive_path_string = format!("tmp/symlink_test.{}", driver.extension());

            let digest = {
                use sha2::{Digest, Sha256};
                let contents = std::fs::read(archive_path_string.as_str()).unwrap();
                let mut hasher = Sha256::new();
                hasher.update(&contents);
                format!("{:x}", hasher.finalize())
            };

            let progress_bar =
                console::Progress::new(console.clone(), archive_path_string.as_str(), None, None);

            let decoder = decoder::Decoder::new(
                archive_path_string.as_str(),
                Some(digest),
                output_dir.as_str(),
                progress_bar,
            )
            .unwrap();
            decoder.extract().unwrap();

            let extracted_link = format!("{}/link_to_file.txt", output_dir);
            let extracted_link_path = std::path::Path::new(&extracted_link);
            assert!(
                extracted_link_path.is_symlink(),
                "Expected symlink at {extracted_link} for driver {driver:?}"
            );
            let target = extracted_link_path.read_link().unwrap();
            assert_eq!(
                target.to_string_lossy(),
                "real_file.txt",
                "Symlink target mismatch for driver {driver:?}"
            );
        }
    }

    #[test]
    fn compress_test() {
        let entries = generate_tmp_files();

        let console = console::Console::new_stdout(console::Verbosity::default()).unwrap();

        const DRIVERS: &[driver::Driver] = &[
            driver::Driver::Gzip,
            driver::Driver::Bzip2,
            driver::Driver::Zip,
            driver::Driver::SevenZ,
            driver::Driver::Xz,
        ];

        for driver in DRIVERS {
            let output_directory = "./tmp";
            let output_filename = format!("test.{}", driver.extension());

            let progress_bar =
                console::Progress::new(console.clone(), output_filename.as_str(), None, None);

            let mut encoder =
                encoder::Encoder::new(output_directory, &output_filename, progress_bar).unwrap();

            encoder.add_entries(&entries).unwrap();

            let _digest = encoder.compress().unwrap().digest().unwrap();
        }

        for driver in DRIVERS {
            let output_dir = format!("tmp/extract_test.{}", driver.extension());
            std::fs::create_dir_all(output_dir.as_str()).unwrap();

            let archive_path_string = format!("tmp/test.{}", driver.extension());

            let digest = {
                use sha2::{Digest, Sha256};
                let contents = std::fs::read(archive_path_string.as_str()).unwrap();
                let mut hasher = Sha256::new();
                hasher.update(&contents);
                format!("{:x}", hasher.finalize())
            };

            let progress_bar =
                console::Progress::new(console.clone(), archive_path_string.as_str(), None, None);

            let decoder = decoder::Decoder::new(
                archive_path_string.as_str(),
                Some(digest),
                output_dir.as_str(),
                progress_bar,
            )
            .unwrap();
            decoder.extract().unwrap();

            verify_generated_files(output_dir.as_str());
        }
    }

    #[test]
    fn create_returns_error_for_empty_input_directory() {
        let root = unique_test_path("empty-input");
        let input = root.join("input");
        let output = root.join("output");
        std::fs::create_dir_all(&input).unwrap();

        let console = console::Console::new_stdout(console::Verbosity::default()).unwrap();
        let progress = console::Progress::new(console, "empty.tar.gz", None, None);

        let create_archive = CreateArchive {
            input: input.to_string_lossy().to_string(),
            name: "empty".to_string(),
            version: "1.0".to_string(),
            driver: driver::Driver::Gzip,
            platform: None,
            includes: None,
            excludes: None,
        };

        let error = create_archive
            .create(output.to_string_lossy().as_ref(), progress)
            .unwrap_err();

        assert!(error.to_string().contains(
            format!(
                "No files to archive for {} in {}",
                create_archive.name, create_archive.input
            )
            .as_str()
        ));

        let _ = std::fs::remove_dir_all(root);
    }
}
