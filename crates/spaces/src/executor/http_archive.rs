use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

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

        let workspace_directory = workspace.read().get_absolute_path();

        self.http_archive
            .create_links(next_progress_bar, workspace_directory.as_ref(), name)
            .context(format_context!(
                "Failed to create hard links for http_archive {}",
                name
            ))?;

        workspace.write().add_member(self.http_archive.get_member());

        Ok(())
    }

}
