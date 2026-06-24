use crate::workspace;

use serde::{Deserialize, Serialize};
use utils::{ecode, http_archive};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpArchive {
    pub http_archive: http_archive::HttpArchive,
}

impl HttpArchive {
    pub fn execute(
        &self,
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let mut lock_file = self.http_archive.get_file_lock();
        let console = progress.console.clone();
        lock_file.lock(console.clone()).map_err(|err| {
            ecode::anyhow(
                ecode::Ecode::FailedToCreateOrAcquireLockFile,
                &format!(
                    "{name} - Failed to lock the spaces store for {}\n{err:?}",
                    self.http_archive.archive.url
                ),
            )
        })?;

        self.http_archive.sync(console.clone()).map_err(|err| {
            ecode::anyhow(
                ecode::Ecode::FailedToLoadJsonFilesManifest,
                &format!(
                    "{name} - Failed to sync http archive {}\n{err:?}",
                    self.http_archive.archive.url
                ),
            )
        })?;

        let mut workspace_write_lock = workspace.write();

        let workspace_directory = workspace_write_lock.get_absolute_path();
        self.http_archive
            .create_links(
                console.clone(),
                workspace_directory.as_ref(),
                name,
                &mut workspace_write_lock.settings.checkout.links,
            )
            .map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::HttpArchiveExecutorOperationFailed,
                    &format!(
                        "{name} - Failed to create links for\n{}\n{err:?}",
                        self.http_archive.archive.url
                    ),
                )
            })?;

        workspace_write_lock.add_member(self.http_archive.get_member());

        Ok(())
    }
}
