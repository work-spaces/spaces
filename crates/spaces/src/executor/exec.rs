use anyhow::Context;
use anyhow_source_location::format_context;

#[derive(Debug, Clone)]
pub struct Exec {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub working_directory: Option<String>,
    pub redirect_stdout: Option<String>,
}

impl Exec {
    pub fn new(
        command: &str,
        args: Vec<String>,
        working_directory: Option<String>,
        env: Vec<(String, String)>,
        redirect_stdout: Option<&str>,
    ) -> Self {
        Exec {
            command: command.to_string(),
            working_directory,
            args,
            env,
            redirect_stdout: redirect_stdout.map(|s| s.to_string()),
        }
    }

    pub fn execute(
        &self,
        name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let options = printer::ExecuteOptions {
            label: name.to_string(),
            arguments: self.args.clone(),
            environment: self.env.clone(),
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
