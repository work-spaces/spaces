use crate::{Level, Secrets};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{mpsc, Arc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogHeader {
    pub(crate) command: Arc<str>,
    pub(crate) working_directory: Option<Arc<str>>,
    pub(crate) environment: HashMap<Arc<str>, HashMap<Arc<str>, Arc<str>>>,
    pub(crate) arguments: Vec<Arc<str>>,
    pub(crate) shell: Arc<str>,
}

#[derive(Clone, Debug)]
pub struct ExecuteOptions {
    pub label: Arc<str>,
    pub is_return_stdout: bool,
    pub working_directory: Option<Arc<str>>,
    pub environment: Vec<(Arc<str>, Arc<str>)>,
    pub arguments: Vec<Arc<str>>,
    pub log_file_path: Option<Arc<str>>,
    pub clear_environment: bool,
    pub process_started_with_id: Option<fn(&str, u32)>,
    pub log_level: Option<Level>,
    pub timeout: Option<std::time::Duration>,
}

impl Default for ExecuteOptions {
    fn default() -> Self {
        Self {
            label: "working".into(),
            is_return_stdout: false,
            working_directory: None,
            environment: vec![],
            arguments: vec![],
            log_file_path: None,
            clear_environment: false,
            process_started_with_id: None,
            log_level: None,
            timeout: None,
        }
    }
}

impl ExecuteOptions {
    pub(crate) fn process_child_output<OutputType: std::io::Read + Send + 'static>(
        output: OutputType,
    ) -> anyhow::Result<(std::thread::JoinHandle<()>, mpsc::Receiver<String>)> {
        let (tx, rx) = mpsc::channel::<String>();

        let thread = std::thread::spawn(move || {
            use std::io::BufReader;
            let reader = BufReader::new(output);
            for line in reader.lines() {
                let line = line.unwrap();
                tx.send(line).unwrap();
            }
        });

        Ok((thread, rx))
    }

    pub(crate) fn spawn(&self, command: &str) -> anyhow::Result<std::process::Child> {
        use std::process::{Command, Stdio};
        let mut process = Command::new(command);

        if self.clear_environment {
            process.env_clear();
        }

        for argument in &self.arguments {
            process.arg(argument.as_ref());
        }

        if let Some(directory) = &self.working_directory {
            process.current_dir(directory.as_ref());
        }

        for (key, value) in self.environment.iter() {
            process.env(key.as_ref(), value.as_ref());
        }

        let result = process
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .context(format!("while spawning piped {command}"))?;

        if let Some(callback) = self.process_started_with_id.as_ref() {
            callback(self.label.as_ref(), result.id());
        }

        Ok(result)
    }

    pub fn get_full_command(&self, command: &str) -> String {
        format!("{command} {}", self.arguments.join(" "))
    }

    pub fn get_full_command_in_working_directory(&self, command: &str) -> String {
        format!(
            "{} {command} {}",
            if let Some(directory) = &self.working_directory {
                directory
            } else {
                ""
            },
            self.arguments.join(" "),
        )
    }
}

pub(crate) fn sanitize_output(input: &str, max_length: usize) -> String {
    let escaped: Vec<_> = input.chars().flat_map(|c| c.escape_default()).collect();

    let mut result = String::new();
    let mut length = 0usize;
    for character in escaped.into_iter() {
        if length < max_length {
            result.push(character);
            length += 1;
        }
    }
    while result.len() < max_length {
        result.push(' ');
    }

    result
}

pub(crate) fn format_monitor_log_message(
    level: Level,
    source: &str,
    command: &str,
    message: &str,
) -> String {
    if level == Level::Passthrough {
        message.to_string()
    } else {
        format!("[{source}:{command}] {message}")
    }
}

pub(crate) fn monitor_process(
    command: &str,
    mut child_process: std::process::Child,
    sender: &mpsc::Sender<String>,
    options: &ExecuteOptions,
    secrets: &Secrets,
) -> anyhow::Result<Option<String>> {
    let start_time = std::time::Instant::now();

    let child_stdout = child_process
        .stdout
        .take()
        .ok_or(anyhow::anyhow!("Internal Error: Child has no stdout"))?;

    let child_stderr = child_process
        .stderr
        .take()
        .ok_or(anyhow::anyhow!("Internal Error: Child has no stderr"))?;

    let log_level_stdout = options.log_level;
    let log_level_stderr = options.log_level;

    let (stdout_thread, stdout_rx) = ExecuteOptions::process_child_output(child_stdout)?;
    let (stderr_thread, stderr_rx) = ExecuteOptions::process_child_output(child_stderr)?;

    let handle_stdout = |sender: &mpsc::Sender<String>,
                         writer: Option<&mut std::fs::File>,
                         content: Option<&mut String>|
     -> anyhow::Result<()> {
        let mut stdout = String::new();
        while let Ok(message) = stdout_rx.try_recv() {
            let redacted = secrets.redact(message.into());
            if writer.is_some() || content.is_some() {
                stdout.push_str(redacted.as_ref());
                stdout.push('\n');
            }
            let _ = sender.send(redacted.to_string());
            if let Some(level) = log_level_stdout.as_ref() {
                let log_message =
                    format_monitor_log_message(*level, "stdout", command, redacted.as_ref());
                let _ = sender.send(log_message);
            }
        }

        if let Some(content) = content {
            content.push_str(stdout.as_str());
        }

        if let Some(writer) = writer {
            let _ = writer.write_all(stdout.as_bytes());
        }
        Ok(())
    };

    let handle_stderr = |sender: &mpsc::Sender<String>,
                         writer: Option<&mut std::fs::File>,
                         content: &mut String|
     -> anyhow::Result<()> {
        let mut stderr = String::new();
        while let Ok(message) = stderr_rx.try_recv() {
            let redacted = secrets.redact(message.into());
            stderr.push_str(redacted.as_ref());
            stderr.push('\n');
            let _ = sender.send(redacted.to_string());
            if let Some(level) = log_level_stderr.as_ref() {
                let log_message =
                    format_monitor_log_message(*level, "stderr", command, redacted.as_ref());
                let _ = sender.send(log_message);
            }
        }
        content.push_str(stderr.as_str());

        if let Some(writer) = writer {
            let _ = writer.write_all(stderr.as_bytes());
        }
        Ok(())
    };

    let mut stderr_content = String::new();
    let mut stdout_content = String::new();

    let mut output_file =
        create_log_file(command, options, secrets).context("Failed to create log file")?;

    let exit_status;

    loop {
        if let Some(status) = child_process
            .try_wait()
            .context("while waiting for child process")?
        {
            exit_status = Some(status);
            break;
        }

        let stdout_content = if options.is_return_stdout {
            Some(&mut stdout_content)
        } else {
            None
        };

        handle_stdout(sender, output_file.as_mut(), stdout_content)
            .context("failed to handle stdout")?;
        handle_stderr(sender, output_file.as_mut(), &mut stderr_content)
            .context("failed to handle stderr")?;
        std::thread::sleep(std::time::Duration::from_millis(100));

        let now = std::time::Instant::now();
        if let Some(timeout) = options.timeout
            && now - start_time > timeout
        {
            child_process.kill().context("Failed to kill process")?;
        }
    }

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    {
        let stdout_content = if options.is_return_stdout {
            Some(&mut stdout_content)
        } else {
            None
        };

        handle_stdout(sender, output_file.as_mut(), stdout_content)
            .context("while handling stdout")?;
    }

    handle_stderr(sender, output_file.as_mut(), &mut stderr_content)
        .context("while handling stderr")?;

    if let Some(exit_status) = exit_status
        && !exit_status.success()
    {
        let stderr_message = if output_file.is_some() {
            String::new()
        } else {
            format!(": {stderr_content}")
        };
        if let Some(code) = exit_status.code() {
            let exit_message = format!("Command `{command}` failed with exit code: {code}");
            return Err(anyhow::anyhow!("{exit_message}{stderr_message}"));
        } else {
            return Err(anyhow::anyhow!(
                "Command `{command}` failed with unknown exit code{stderr_message}"
            ));
        }
    }

    Ok(if options.is_return_stdout {
        Some(stdout_content)
    } else {
        None
    })
}

pub(crate) fn create_log_file(
    command: &str,
    options: &ExecuteOptions,
    secrets: &Secrets,
) -> anyhow::Result<Option<std::fs::File>> {
    if let Some(log_path) = options.log_file_path.as_ref() {
        let mut file = std::fs::File::create(log_path.as_ref())
            .context(format!("while creating {log_path}"))?;

        let mut environment = HashMap::new();
        const INHERITED: &str = "inherited";
        const GIVEN: &str = "given";
        environment.insert(INHERITED.into(), HashMap::new());
        environment.insert(GIVEN.into(), HashMap::new());
        let env_inherited = environment.get_mut(INHERITED).unwrap();
        if !options.clear_environment {
            for (key, value) in std::env::vars() {
                let redacted = secrets.redact(value.into());
                env_inherited.insert(key.into(), redacted);
            }
        }
        let env_given = environment.get_mut(GIVEN).unwrap();
        for (key, value) in options.environment.iter() {
            let redacted = secrets.redact(value.clone());
            env_given.insert(key.clone(), redacted);
        }

        let arguments = options.arguments.join(" ");
        let arguments_escaped: Vec<_> =
            arguments.chars().flat_map(|c| c.escape_default()).collect();
        let args = arguments_escaped.into_iter().collect::<String>();
        let shell = format!("{command} {args}").into();

        let redacted_arguments = options
            .arguments
            .iter()
            .map(|arg| secrets.redact(arg.clone()))
            .collect();

        let log_header = LogHeader {
            command: command.into(),
            working_directory: options.working_directory.clone(),
            environment,
            arguments: redacted_arguments,
            shell,
        };

        let log_header_serialized = serde_yaml::to_string(&log_header)
            .context("Internal Error: failed to yamlize log header")?;

        let divider = "=".repeat(80);

        file.write_all(format!("{log_header_serialized}{divider}\n").as_bytes())
            .context(format!("while writing {log_path}"))?;

        Ok(Some(file))
    } else {
        Ok(None)
    }
}
