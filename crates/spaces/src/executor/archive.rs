use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Archive {
    pub create_archive: easy_archiver::CreateArchive,
}

impl Archive {
    pub fn execute(&self, name: &str, progress: printer::MultiProgressBar) -> anyhow::Result<()> {
        let workspace_directory = workspace::get_workspace_path()
            .context(format_context!("failed to get workspace directory"))?;

        let output_directory = format!("{workspace_directory}/build/{name}");

        std::fs::create_dir_all(output_directory.as_str()).context(format_context!(
            "failed to create output directory {output_directory}"
        ))?;

        self.create_archive
            .create(output_directory.as_str(), progress)
            .context(format_context!(
                "failed to create archive {output_directory}"
            ))?;

        Ok(())
    }
}
