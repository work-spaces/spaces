use crate::info;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exec {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub working_directory: Option<String>,
    pub redirect_stdout: Option<String>,
}

impl Exec {
    pub fn execute(
        &self,
        name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let arguments = self.args.clone().unwrap_or_default();
        let environment_map = self.env.clone().unwrap_or_default();
        let workspace_path =
            info::get_workspace_path().context(format_context!("No workspace directory found"))?;

        let mut environment = Vec::new();
        for (key, value) in environment_map {
            environment.push((key, value));
        }

        let options = printer::ExecuteOptions {
            label: name.to_string(),
            arguments,
            environment,
            working_directory: self
                .working_directory
                .clone()
                .map(|cwd| format!("{}/{}", workspace_path, cwd)),
            is_return_stdout: self.redirect_stdout.is_some(),
            ..Default::default()
        };

        let stdout_content = progress
            .execute_process(&self.command, options)
            .context(format_context!("Failed to execute task {}", name))?;

        if let (Some(stdout_content), Some(stdout_location)) =
            (stdout_content, self.redirect_stdout.as_ref())
        {
            std::fs::write(stdout_location, stdout_content).context(format_context!(
                "Failed to write stdout to {}",
                stdout_location
            ))?;
        }

        Ok(())
    }
}
