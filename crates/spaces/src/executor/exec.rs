use crate::{info, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Expect {
    Failure,
    Success
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exec {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub working_directory: Option<String>,
    pub redirect_stdout: Option<String>,
    pub expect: Option<Expect>
}

impl Exec {
    pub fn execute(
        &self,
        name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let arguments = self.args.clone().unwrap_or_default();
        let workspace_env = info::get_env();

        let mut environment_map = HashMap::new();

        environment_map.insert("PATH".to_string(), workspace_env.paths.join(":"));
        for (key, value) in workspace_env.vars {
            environment_map.insert(key, value);
        }
        for (key, value) in self.env.clone().unwrap_or_default() {
            environment_map.insert(key, value);
        }

        let workspace_path = workspace::absolute_path();

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
            log_file_path: Some(workspace::get_log_file(name)),
            ..Default::default()
        };

        let result = progress
        .execute_process(&self.command, options)
        .context(format_context!("Failed to execute task {}", name));

        let stdout_content = match result {
            Ok(content) => {
                if let Some(Expect::Failure) = self.expect.as_ref() {
                    return Err(format_error!("Expected failure but task succeeded"));
                } else {
                    content
                }
            }
            Err(_) => {
                if let Some(Expect::Success) = self.expect.as_ref() {
                    return Err(format_error!("Expected success but task failed"));
                } else {
                    None
                }
            }
        };
        
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
