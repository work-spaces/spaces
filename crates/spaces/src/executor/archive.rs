use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub create_archive: easy_archiver::CreateArchive,
}

impl Archive {
    pub fn execute(
        &self,
        mut progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let workspace_directory = workspace.read().get_absolute_path();
        let output_directory = format!("{workspace_directory}/build/{name}");

        std::fs::create_dir_all(output_directory.as_str()).context(format_context!(
            "failed to create output directory {output_directory}"
        ))?;

        logger::Logger::new_progress(&mut progress, name.into())
            .debug(format!("Creating archive {output_directory}").as_str());

        let (output_file_path, digest) = self
            .create_archive
            .create(output_directory.as_str(), progress)
            .context(format_context!(
                "failed to create archive {output_directory}"
            ))?;

        let output_file_as_path = std::path::Path::new(output_file_path.as_str());
        let output_sha_suffix_as_path = output_file_as_path
            .with_extension("")
            .with_extension("sha256.txt");

        std::fs::write(output_sha_suffix_as_path.clone(), digest).context(format_context!(
            "failed to write sha256 file {output_sha_suffix_as_path:?}"
        ))?;

        Ok(())
    }
}
