use crate::workspace;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};

fn get_oras_command() -> String {
    format!("{}/sysroot/bin/oras", workspace::get_spaces_tools_path())
}

struct ManifestDetails {
    filename: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrasArchive {
    pub url: String,
    pub artifact: String,
    pub tag: String,
    pub manifest_digest_path: String,
    pub manifest_artifact_path: String,
    pub add_prefix: Option<String>,
    pub strip_prefix: Option<String>,
    pub globs: Option<Vec<String>>,
}

impl OrasArchive {
    fn get_artifact_label(&self) -> String {
        format!("{}/{}:{}", self.url, self.artifact, self.tag)
    }

    pub fn download(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
        output_folder: &str,
    ) -> anyhow::Result<()> {
        let artifact_label = self.get_artifact_label();

        let options = printer::ExecuteOptions {
            arguments: vec![
                "pull".to_string(),
                "--no-tty".to_string(),
                format!("--output={}", output_folder),
                artifact_label.clone(),
            ],
            ..Default::default()
        };

        progress_bar.log(
            printer::Level::Trace,
            format!("{artifact_label} Downloading using oras {options:?}").as_str(),
        );

        progress_bar
            .execute_process(&get_oras_command(), options)
            .context(format_context!(
                "failed to download {artifact_label} using oras",
            ))?;

        Ok(())
    }

    fn get_manifest_details(
        &self,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<ManifestDetails> {
        let artifact_label = self.get_artifact_label();
        let options = printer::ExecuteOptions {
            arguments: vec![
                "manifest".to_string(),
                "fetch".to_string(),
                artifact_label.clone(),
            ],
            is_return_stdout: true,
            ..Default::default()
        };

        let manifest = progress
            .execute_process(get_oras_command().as_str(), options)
            .context(format_context!(
                "failed to download {artifact_label} using oras",
            ))?;

        if let Some(manifest) = manifest {
            let value: serde_json::Value = serde_json::from_str(&manifest)
                .context(format_context!("failed to parse manifest from {artifact_label}"))?;
            let mut sha256_option = None;
            let mut filename_option = None;

            if let Some(digest) = value.pointer(&self.manifest_digest_path) {
                if let Some(digest) = digest.as_str() {
                    if let Some(sha256) = digest.strip_prefix("sha256:") {
                        sha256_option = Some(sha256.to_string());
                    }
                }
            }

            if let Some(filename) = value.pointer(&self.manifest_artifact_path) {
                if let Some(filename) = filename.as_str() {
                    filename_option = Some(filename.to_string());
                }
            }

            if let (Some(sha256), Some(filename)) = (sha256_option, filename_option) {
                return Ok(ManifestDetails { filename, sha256 });
            }

            return Err(format_error!(
                "Failed to find sha256 or filename in manifest {self:?}"
            ));
        }
        Err(format_error!(
            "Internal error: oras failed to return manifest"
        ))
    }

    pub fn execute(
        &self,
        name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        // download the manifest and get the digest

        let manifest_details = self
            .get_manifest_details(&mut progress)
            .context(format_context!("Failed to fetch manifest"))?;

        let archive = http_archive::Archive {
            url: format!("oras://{}/{}", self.url, manifest_details.filename),
            sha256: manifest_details.sha256,
            add_prefix: self.add_prefix.clone(),
            strip_prefix: self.strip_prefix.clone(),
            globs: self.globs.clone(),
            ..Default::default()
        };

        let tools_path = format!("{}/sysroot/bin", workspace::get_spaces_tools_path());
        let store_path = workspace::get_store_path();
        let http_archive = http_archive::HttpArchive::new(&store_path, name, &archive, &tools_path)
            .context(format_context!("Failed to create http_archive {archive:?}"))?;

        let full_path = std::path::Path::new(&http_archive.full_path_to_archive);

        if !full_path.exists() {
            let parent = full_path
                .parent()
                .context(format_context!("Failed to get parent of {full_path:?}"))?
                .to_string_lossy()
                .to_string();
            // need to ensure the archive is downloaded before using http_archive which doesn't know how to download
            self.download(&mut progress, &parent)
                .context(format_context!("Failed to download using oras"))?;

            let full_path_to_download =
                std::path::Path::new(&parent).join(&manifest_details.filename);
            //rename the file name to the name http_archive expects
            std::fs::rename(full_path_to_download.clone(), full_path).context(format_context!(
                "Failed to rename {full_path_to_download:?} to {full_path:?}"
            ))?;
        }

        // sync will skip the download because the file is already there
        let next_progress_bar = http_archive
            .sync(progress)
            .context(format_context!("Failed to sync http_archive {}", name))?;

        let workspace_directory = workspace::absolute_path();

        http_archive
            .create_links(next_progress_bar, workspace_directory.as_str(), name)
            .context(format_context!(
                "Failed to create hard links for oras http_archive {}",
                name
            ))?;

        Ok(())
    }
}
