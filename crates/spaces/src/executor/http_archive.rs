use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpArchiveSync {
    pub http_archive: http_archive::HttpArchive,
}

impl HttpArchiveSync {
    pub fn execute(&self, name: &str, progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        self.http_archive
            .sync(progress)
            .context(format_context!("Failed to sync http_archive {}", name))?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpArchiveCreateLinks {
    pub http_archive: http_archive::HttpArchive,
}

impl HttpArchiveCreateLinks {
    pub fn execute(&self, name: &str, progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        let workspace_directory =
            workspace::get_workspace_path().context(format_context!("No workspace directory found"))?;

        self.http_archive
            .create_links(progress, workspace_directory.as_str(), name)
            .context(format_context!("Failed to create hard links for http_archive {}", name))?;

        Ok(())
    }
}
