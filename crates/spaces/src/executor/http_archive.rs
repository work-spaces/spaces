use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpArchive {
    pub http_archive: http_archive::HttpArchive,
}

impl HttpArchive {
    pub fn execute(&self, name: &str, progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        let next_progress_bar = self
            .http_archive
            .sync(progress)
            .context(format_context!("Failed to sync http_archive {}", name))?;

        let workspace_directory = workspace::absolute_path();

        self.http_archive
            .create_links(next_progress_bar, workspace_directory.as_str(), name)
            .context(format_context!(
                "Failed to create hard links for http_archive {}",
                name
            ))?;

        Ok(())
    }
}
