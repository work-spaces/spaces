use crate::workspace;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use utils::{ecode, http_archive, logger};

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
        progress_bar: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        output_folder: &str,
    ) -> anyhow::Result<()> {
        let artifact_label = self.get_artifact_label();

        let options = console::ExecuteOptions {
            arguments: vec![
                "pull".into(),
                "--no-tty".into(),
                format!("--output={output_folder}").into(),
                artifact_label.clone(),
            ],
            ..Default::default()
        };

        let logger = logger::Logger::new(
            progress_bar.console.clone(),
            self.get_artifact_label().clone(),
        );
        logger.debug(format!("Downloading using oras {}", options.arguments.join(" ")).as_str());

        progress_bar
            .execute_process(
                &get_oras_command(&workspace.read().get_spaces_tools_path()),
                options,
            )
            .map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::FailedToCreateOrAcquireLockFile,
                    &format!("failed to download {artifact_label} using oras\n{err:?}",),
                )
            })?;

        Ok(())
    }

    fn get_manifest_details(
        &self,
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
    ) -> anyhow::Result<ManifestDetails> {
        let artifact_label = self.get_artifact_label();
        let options = console::ExecuteOptions {
            arguments: vec!["manifest".into(), "fetch".into(), artifact_label.clone()],
            is_return_stdout: true,
            allow_failure: true,
            ..Default::default()
        };

        let manifest = progress
            .execute_process(
                get_oras_command(&workspace.read().get_spaces_tools_path()).as_ref(),
                options,
            )
            .map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::FailedToCreateOrAcquireLockFile,
                    &format!("failed to fetch manifest for {artifact_label} using oras\n{err:?}",),
                )
            })?;

        if manifest.exit_code != 0 {
            return Err(ecode::anyhow(
                ecode::Ecode::FailedToCreateOrAcquireLockFile,
                &format!(
                    "oras manifest fetch for {artifact_label} failed with exit code {}",
                    manifest.exit_code
                ),
            ));
        }

        if let Some(manifest) = manifest.stdout {
            let value: serde_json::Value = serde_json::from_str(&manifest).map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::FailedToCreateOrAcquireLockFile,
                    &format!("failed to parse manifest from {artifact_label}\n{err:?}"),
                )
            })?;
            let mut sha256_option: Option<Arc<str>> = None;
            let mut filename_option: Option<Arc<str>> = None;

            if let Some(digest) = value.pointer(&self.manifest_digest_path)
                && let Some(digest) = digest.as_str()
                && let Some(sha256) = digest.strip_prefix("sha256:")
            {
                sha256_option = Some(sha256.into());
            }

            if let Some(filename) = value.pointer(&self.manifest_artifact_path)
                && let Some(filename) = filename.as_str()
            {
                filename_option = Some(filename.into());
            }

            if let (Some(sha256), Some(filename)) = (sha256_option, filename_option) {
                return Ok(ManifestDetails { filename, sha256 });
            }

            return Err(ecode::anyhow(
                ecode::Ecode::FailedToCreateOrAcquireLockFile,
                &format!("Failed to find sha256 or filename in manifest {self:?}"),
            ));
        }
        Err(ecode::anyhow(
            ecode::Ecode::FailedToCreateOrAcquireLockFile,
            "Internal error: oras failed to return manifest",
        ))
    }

    pub fn execute(
        &self,
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        // download the manifest and get the digest
        let console = progress.console.clone();

        let manifest_details = self
            .get_manifest_details(progress, workspace.clone())
            .map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::OrasExecutorOperationFailed,
                    &format!("Failed to fetch manifest for {name}\n{err:?}"),
                )
            })?;

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
            .map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::OrasExecutorOperationFailed,
                    &format!("Failed to create http_archive {archive:?}\n{err:?}"),
                )
            })?;

        let full_path = std::path::Path::new(&http_archive.full_path_to_archive);

        let mut lock_file = http_archive.get_file_lock();
        lock_file.lock(console.clone()).map_err(|err| {
            ecode::anyhow(
                ecode::Ecode::OrasExecutorOperationFailed,
                &format!(
                    "{name} - Failed to lock the spaces store for {}\n{err:?}",
                    http_archive.archive.url
                ),
            )
        })?;

        if !full_path.exists() {
            let parent = full_path
                .parent()
                .ok_or_else(|| {
                    ecode::anyhow(
                        ecode::Ecode::OrasExecutorOperationFailed,
                        &format!("Failed to get parent of {full_path:?}"),
                    )
                })?
                .to_string_lossy()
                .to_string();
            // need to ensure the archive is downloaded before using http_archive which doesn't know how to download
            self.download(progress, workspace.clone(), &parent)
                .map_err(|err| {
                    ecode::anyhow(
                        ecode::Ecode::OrasExecutorOperationFailed,
                        &format!("Failed to download using oras for {name}\n{err:?}"),
                    )
                })?;

            let full_path_to_download =
                std::path::Path::new(&parent).join(manifest_details.filename.as_ref());
            //rename the file name to the name http_archive expects
            std::fs::rename(full_path_to_download.clone(), full_path).map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::OrasExecutorOperationFailed,
                    &format!(
                        "Failed to rename {full_path_to_download:?} to {full_path:?}\n{err:?}"
                    ),
                )
            })?;
        }

        // sync will skip the download because the file is already there
        http_archive.sync(console.clone()).map_err(|err| {
            ecode::anyhow(
                ecode::Ecode::OrasExecutorOperationFailed,
                &format!("Failed to sync http_archive {}\n{err:?}", name),
            )
        })?;

        let mut workspace_write_lock = workspace.write();
        let workspace_directory = workspace_write_lock.absolute_path.clone();

        http_archive
            .create_links(
                console.clone(),
                workspace_directory.as_ref(),
                name,
                &mut workspace_write_lock.settings.checkout.links,
            )
            .map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::OrasExecutorOperationFailed,
                    &format!(
                        "Failed to create hard links for oras http_archive {}\n{err:?}",
                        name
                    ),
                )
            })?;

        Ok(())
    }
}
