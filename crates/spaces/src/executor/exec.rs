use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exec {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<Vec<(String, String)>>,
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
        let environment = self.env.clone().unwrap_or_default();

        let options = printer::ExecuteOptions {
            label: name.to_string(),
            arguments,
            environment,
            working_directory: self.working_directory.clone(),
            is_return_stdout: self.redirect_stdout.is_some(),
            ..Default::default()
        };

        let stdout_content = progress
            .execute_process(&self.command, options)
            .context(format_context!("Failed to execute task {}", name))?;

        if let (Some(stdout_content), Some(stdout_location) )= (stdout_content, self.redirect_stdout.as_ref()) {
            std::fs::write(stdout_location, stdout_content)
                .context(format_context!("Failed to write stdout to {}", stdout_location))?;
        }

        Ok(())
    }
}
