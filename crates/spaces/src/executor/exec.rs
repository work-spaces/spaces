use crate::{info, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Expect {
    Failure,
    Success,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exec {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub working_directory: Option<String>,
    pub redirect_stdout: Option<String>,
    pub expect: Option<Expect>,
}

impl Exec {
    pub fn execute(
        &self,
        name: &str,
        progress: &mut printer::MultiProgressBar,
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

        let log_file_path = workspace::get_log_file(name);

        let options = printer::ExecuteOptions {
            label: name.to_string(),
            arguments,
            environment,
            working_directory: self
                .working_directory
                .clone()
                .map(|cwd| format!("{}/{}", workspace_path, cwd)),
            is_return_stdout: self.redirect_stdout.is_some(),
            log_file_path: Some(log_file_path.clone()),
        };

        progress.log(
            printer::Level::Trace,
            format!("exec {name}: {} {options:?}", self.command).as_str(),
        );

        let result = progress.execute_process(&self.command, options);

        progress.log(
            printer::Level::Message,
            format!("log file for {name}: {log_file_path}").as_str(),
        );

        let stdout_content = match result {
            Ok(content) => {
                progress.log(
                    printer::Level::Info,
                    format!("exec {name} succeeded").as_str(),
                );

                if let Some(Expect::Failure) = self.expect.as_ref() {
                    return Err(format_error!("Expected failure but task succeeded"));
                } else {
                    content
                }
            }
            Err(exec_error) => {
                progress.log(printer::Level::Info, format!("exec {name} failed").as_str());

                if let Some(Expect::Failure) = self.expect.as_ref() {
                    None
                } else {
                    let log_contents = std::fs::read_to_string(&log_file_path)
                        .context(format_context!("Failed to read log file {}", log_file_path))?;

                    if log_contents.len() > 8192 {
                        progress.log(printer::Level::Error, format!("See log file {log_file_path} for details").as_str());  
                    } else {
                        progress.log(printer::Level::Error, log_contents.as_str());
                    }
                    return Err(format_error!(
                        "Expected success but task failed because {exec_error}"
                    ));
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecIf {
    #[serde(rename = "if")]
    pub if_: Exec,
    #[serde(rename = "then")]
    pub then_: Vec<String>,
    #[serde(rename = "else")]
    pub else_: Option<Vec<String>>,
}

impl ExecIf {
    pub fn execute(&self, name: &str, mut progress: printer::MultiProgressBar) -> Vec<String> {
        let condition_result = self.if_.execute(name, &mut progress);
        let mut result = Vec::new();
        match condition_result {
            Ok(_) => {
                progress.log(
                    printer::Level::Trace,
                    format!("exec {name} condition succeeded").as_str(),
                );
                result = self.then_.clone();
            }
            Err(_) => {
                progress.log(
                    printer::Level::Trace,
                    format!("exec {name} condition failed running").as_str(),
                );
                if let Some(else_) = self.else_.as_ref() {
                    result = else_.clone();
                }
            }
        }
        progress.log(
            printer::Level::Trace,
            format!("exec if {name} enable targets: {result:?}",).as_str(),
        );

        result
    }
}
