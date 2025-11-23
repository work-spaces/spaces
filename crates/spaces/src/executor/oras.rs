use crate::workspace;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

fn get_oras_command(tools_path: &str) -> Arc<str> {
    format!("{tools_path}/sysroot/bin/oras").into()
}

struct ManifestDetails {
    filename: Arc<str>,
    sha256: Arc<str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrasArchive {
    pub url: Arc<str>,
    pub artifact: Arc<str>,
    pub tag: Arc<str>,
    pub manifest_digest_path: Arc<str>,
    pub manifest_artifact_path: Arc<str>,
    pub add_prefix: Option<Arc<str>>,
    pub strip_prefix: Option<Arc<str>>,
    pub globs: Option<HashSet<Arc<str>>>,
}

impl OrasArchive {
    fn get_artifact_label(&self) -> Arc<str> {
        format!("{}/{}:{}", self.url, self.artifact.to_lowercase(), self.tag).into()
    }

    pub fn download(
        &self,
        progress_bar: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        output_folder: &str,
    ) -> anyhow::Result<()> {
        let artifact_label = self.get_artifact_label();

        let options = printer::ExecuteOptions {
            arguments: vec![
                "pull".into(),
                "--no-tty".into(),
                format!("--output={output_folder}").into(),
                artifact_label.clone(),
            ],
            ..Default::default()
        };

        let mut logger =
            logger::Logger::new_progress(progress_bar, self.get_artifact_label().clone());
        logger.debug(format!("Downloading using oras {}", options.arguments.join(" ")).as_str());

        progress_bar
            .execute_process(
                &get_oras_command(&workspace.read().get_spaces_tools_path()),
                options,
            )
            .context(format_context!(
                "failed to download {artifact_label} using oras",
            ))?;

        Ok(())
    }

    fn get_manifest_details(
        &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
    ) -> anyhow::Result<ManifestDetails> {
        let artifact_label = self.get_artifact_label();
        let options = printer::ExecuteOptions {
            arguments: vec!["manifest".into(), "fetch".into(), artifact_label.clone()],
            is_return_stdout: true,
            ..Default::default()
        };

        let manifest = progress
            .execute_process(
                get_oras_command(&workspace.read().get_spaces_tools_path()).as_ref(),
                options,
            )
            .context(format_context!(
                "failed to download {artifact_label} using oras",
            ))?;

        if let Some(manifest) = manifest {
            let value: serde_json::Value = serde_json::from_str(&manifest).context(
                format_context!("failed to parse manifest from {artifact_label}"),
            )?;
            let mut sha256_option: Option<Arc<str>> = None;
            let mut filename_option: Option<Arc<str>> = None;

            if let Some(digest) = value.pointer(&self.manifest_digest_path) {
                if let Some(digest) = digest.as_str() {
                    if let Some(sha256) = digest.strip_prefix("sha256:") {
                        sha256_option = Some(sha256.into());
                    }
                }
            }

            if let Some(filename) = value.pointer(&self.manifest_artifact_path) {
                if let Some(filename) = filename.as_str() {
                    filename_option = Some(filename.into());
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
        mut progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        // download the manifest and get the digest

        let manifest_details = self
            .get_manifest_details(&mut progress, workspace.clone())
            .context(format_context!("Failed to fetch manifest"))?;

        let archive = http_archive::Archive {
            url: format!("oras://{}/{}", self.url, manifest_details.filename).into(),
            sha256: manifest_details.sha256,
            add_prefix: self.add_prefix.clone(),
            strip_prefix: self.strip_prefix.clone(),
            globs: self.globs.clone(),
            ..Default::default()
        };

        let tools_path = format!("{}/sysroot/bin", workspace.read().get_spaces_tools_path());
        let store_path = workspace.read().get_store_path();
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
            self.download(&mut progress, workspace.clone(), &parent)
                .context(format_context!("Failed to download using oras"))?;

            let full_path_to_download =
                std::path::Path::new(&parent).join(manifest_details.filename.as_ref());
            //rename the file name to the name http_archive expects
            std::fs::rename(full_path_to_download.clone(), full_path).context(format_context!(
                "Failed to rename {full_path_to_download:?} to {full_path:?}"
            ))?;
        }

        // sync will skip the download because the file is already there
        let next_progress_bar = http_archive
            .sync(progress)
            .context(format_context!("Failed to sync http_archive {}", name))?;

        let mut workspace_write_lock = workspace.write();
        let workspace_directory = workspace_write_lock.absolute_path.clone();

        http_archive
            .create_links(
                next_progress_bar,
                workspace_directory.as_ref(),
                name,
                &mut workspace_write_lock.settings.checkout.links,
            )
            .context(format_context!(
                "Failed to create hard links for oras http_archive {}",
                name
            ))?;

        Ok(())
    }
}
