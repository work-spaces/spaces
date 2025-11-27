use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use utils::http_archive;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpArchive {
    pub http_archive: http_archive::HttpArchive,
}

impl HttpArchive {
    pub fn execute(
        &self,
        progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let next_progress_bar = self
            .http_archive
            .sync(progress)
            .context(format_context!("Failed to sync http_archive {}", name))?;

        let mut workspace_write_lock = workspace.write();

        let workspace_directory = workspace_write_lock.get_absolute_path();
        self.http_archive
            .create_links(
                next_progress_bar,
                workspace_directory.as_ref(),
                name,
                &mut workspace_write_lock.settings.checkout.links,
            )
            .context(format_context!(
                "Failed to create hard links for http_archive {}",
                name
            ))?;

        workspace_write_lock.add_member(self.http_archive.get_member());

        Ok(())
    }
}
