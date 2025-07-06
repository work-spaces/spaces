use crate::{singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use strum::Display;

#[derive(Debug, Clone, Default)]
struct State {
    processes: HashMap<String, u32>,
}

static STATE: state::InitCell<lock::StateLock<State>> = state::InitCell::new();

fn logger<'a>(progress: &'a mut printer::MultiProgressBar, name: &str) -> logger::Logger<'a> {
    logger::Logger::new_progress(progress, name.into())
}

fn get_state() -> &'static lock::StateLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(lock::StateLock::new(State::default()));
    STATE.get()
}

fn handle_process_started(rule: &str, process_id: u32) {
    let mut state = get_state().write();
    state.processes.insert(rule.to_string(), process_id);
}

fn handle_process_ended(rule: &str) {
    let mut state = get_state().write();
    state.processes.remove(rule);
}

fn get_process_id(rule: &str) -> Option<u32> {
    let state = get_state().read();
    state.processes.get(rule).copied()
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default, Display)]
pub enum Expect {
    Failure,
    #[default]
    Success,
    Any,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exec {
    pub command: Arc<str>,
    pub args: Option<Vec<Arc<str>>>,
    pub env: Option<HashMap<Arc<str>, Arc<str>>>,
    pub working_directory: Option<Arc<str>>,
    pub redirect_stdout: Option<Arc<str>>,
    pub expect: Option<Expect>,
    pub log_level: Option<printer::Level>,
    pub timeout: Option<f64>,
}

impl Exec {
    pub fn execute(
        &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let mut arguments = self.args.clone().unwrap_or_default();
        let workspace_env = workspace.read().get_env();

        let mut environment_map = workspace_env
            .get_vars()
            .context(format_context!("Failed to get env vars"))?;

        let absolute_path_to_workspace = workspace.read().get_absolute_path();
        let (working_directory, pwd) = if let Some(directory) = self.working_directory.as_ref() {
            if let Some(relative_workspace_path) = directory.strip_prefix("//") {
                let absolute_path: Arc<str> =
                    format!("{absolute_path_to_workspace}/{relative_workspace_path}").into();
                (Some(absolute_path.clone()), absolute_path)
            } else {
                // the working directory member gets santized when the rule is created
                // and always starts with //
                (None, absolute_path_to_workspace.clone())
            }
        } else {
            (None, absolute_path_to_workspace.clone())
        };

        environment_map.insert("PWD".into(), pwd);

        for (key, value) in self.env.clone().unwrap_or_default() {
            environment_map.insert(key, value);
        }

        // overwrite values passed on the command line
        let args_env = singleton::get_args_env();
        for (key, value) in args_env {
            environment_map.insert(key, value);
        }

        let command_line_target = workspace.read().target.clone();

        let mut log_level = self.log_level;
        if let Some(target) = command_line_target {
            if target.as_ref() == name {
                let trailing_args = workspace.read().trailing_args.clone();
                let is_trailing_args_empty = trailing_args.is_empty();
                arguments.extend(trailing_args);
                if log_level.is_none() && !is_trailing_args_empty {
                    log_level = Some(printer::Level::Passthrough);
                }
            }
        }

        let environment = environment_map.into_iter().collect::<Vec<_>>();

        let log_file_path = if singleton::get_is_ci() {
            None
        } else {
            Some(workspace.read().get_log_file(name))
        };

        logger(progress, name).debug(format!("Workspace ENV: {workspace_env:?}",).as_str());
        logger(progress, name).debug(format!("Env: {environment:?}",).as_str());

        let options = printer::ExecuteOptions {
            label: name.into(),
            arguments,
            environment,
            working_directory,
            is_return_stdout: self.redirect_stdout.is_some(),
            log_file_path: log_file_path.clone(),
            clear_environment: true,
            process_started_with_id: Some(handle_process_started),
            log_level,
            timeout: self.timeout.map(std::time::Duration::from_secs_f64),
        };

        logger(progress, name).debug(
            format!(
                "Executing: {} {}",
                self.command,
                options.arguments.join(" ")
            )
            .as_str(),
        );

        let result = progress.execute_process(&self.command, options);

        handle_process_ended(name);

        logger(progress, name).message(format!("log file: {log_file_path:?}").as_str());

        let stdout_content = match result {
            Ok(content) => {
                logger(progress, name).info("succeeded");

                if let Some(Expect::Failure) = self.expect.as_ref() {
                    return Err(format_error!("Expected failure but task succeeded"));
                } else {
                    content
                }
            }
            Err(exec_error) => {
                logger(progress, name).info("Failed");
                if let Some(Expect::Failure) = self.expect.as_ref() {
                    None
                } else if let Some(Expect::Any) = self.expect.as_ref() {
                    None
                } else {
                    // if the command failed to execute, there won't be a log file
                    if let Some(log_file_path) = log_file_path {
                        if std::path::Path::new(log_file_path.as_ref()).exists() {
                            let log_contents =
                                std::fs::read_to_string(log_file_path.as_ref()).context(
                                    format_context!("Failed to read log file {}", log_file_path),
                                )?;
                            if log_contents.len() > 10 * 1024 * 1024 {
                                logger(progress, name).error(
                                    format!("See log file {log_file_path} for details").as_str(),
                                );
                            } else {
                                logger(progress, name).error(log_contents.as_str());
                            }
                        }
                    } else {
                        logger(progress, name).error(
                            "No log file is available (log files disabled with the --ci option)",
                        );
                    }
                    return Err(format_error!(
                        "Expected success but task failed because:\n {exec_error:?}"
                    ));
                }
            }
        };

        if let (Some(stdout_content), Some(stdout_location)) =
            (stdout_content, self.redirect_stdout.as_ref())
        {
            let parent_path = std::path::Path::new(stdout_location.as_ref())
                .parent()
                .context(format_context!(
                    "Failed to get parent directory of {}",
                    stdout_location
                ))?;

            std::fs::create_dir_all(parent_path).context(format_context!(
                "Failed to create parent directory {:?} for stdout file {}",
                parent_path,
                stdout_location
            ))?;

            std::fs::write(stdout_location.as_ref(), stdout_content).context(format_context!(
                "Failed to write stdout to {}",
                stdout_location
            ))?;
        }

        Ok(())
    }

    pub fn to_markdown(&self) -> String {
        use printer::markdown;
        let mut result = String::new();

        let has_args = if let Some(args) = self.args.as_ref() {
            !args.is_empty()
        } else {
            false
        };

        let invoke = if has_args {
            format!(
                "{} \\\n  {}",
                self.command,
                self.args
                    .as_ref()
                    .map(|args| args.join(" \\\n  "))
                    .unwrap_or_default()
            )
        } else {
            format!("{}", self.command)
        };

        result.push_str("Shell Command:\n\n");

        result.push_str(&markdown::code_block("sh", invoke.as_str()));
        let mut items: Vec<Arc<str>> = Vec::new();

        if let Some(working_directory) = self.working_directory.as_ref() {
            items.push(format!("Working Directory: `{working_directory}`\n").into());
        }
        if let Some(redirect_stdout) = self.redirect_stdout.as_ref() {
            items.push(format!("Redirect Stdout: `{redirect_stdout}`\n").into());
        }
        if let Some(timeout) = self.timeout {
            items.push(format!("Timeout: `{timeout}`\n").into());
        }

        items.push(format!("Expect: `{}`", self.expect.unwrap_or_default()).into());

        result.push_str(&markdown::list(items));

        if let Some(env) = self.env.as_ref() {
            let mut env_lines: Vec<Arc<str>> = Vec::new();
            for (key, value) in env {
                env_lines.push(format!("`{key}`: `{value}`").into());
            }
            result.push_str(&markdown::list(env_lines));
        }

        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Signal {
    Hup,
    Int,
    Quit,
    Abort,
    Kill,
    Alarm,
    Terminate,
    User1,
    User2,
}

impl Signal {
    fn to_kill_arg(self) -> Arc<str> {
        let value = match self {
            Signal::Hup => "HUP",
            Signal::Int => "INT",
            Signal::Quit => "QUIT",
            Signal::Abort => "ABRT",
            Signal::Kill => "KILL",
            Signal::Alarm => "ALRM",
            Signal::Terminate => "TERM",
            Signal::User1 => "USR1",
            Signal::User2 => "USR2",
        };
        value.into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kill {
    pub signal: Signal,
    pub target: Arc<str>,
    pub expect: Option<Expect>,
}

impl Kill {
    pub fn execute(
        &self,
        name: &str,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        if let Some(process_id) = get_process_id(self.target.as_ref()) {
            let options = printer::ExecuteOptions {
                label: name.into(),
                arguments: vec![
                    "-s".into(),
                    self.signal.to_kill_arg(),
                    format!("{process_id}").into(),
                ],
                ..Default::default()
            };

            let result = progress.execute_process("kill", options);
            match self.expect.as_ref() {
                Some(Expect::Success) => {
                    if result.is_err() {
                        return Err(format_error!("Expected success but kill failed {self:?}"));
                    }
                }
                Some(Expect::Failure) => {
                    if result.is_ok() {
                        return Err(format_error!(
                            "Expected failure but kill succeeded {self:?}"
                        ));
                    }
                }
                _ => {}
            }
        } else if let Some(Expect::Success) = self.expect.as_ref() {
            return Err(format_error!("No process found for {name}"));
        }

        Ok(())
    }

    pub fn to_markdown(&self) -> String {
        use printer::markdown;
        let mut result = String::new();
        let invoke = format!("$ kill -s {} {}", self.signal.to_kill_arg(), self.target);
        result.push_str(&markdown::code_block("sh", invoke.as_str()));
        let mut items: Vec<Arc<str>> = Vec::new();
        items.push(format!("Expect: `{}`", self.expect.unwrap_or_default()).into());
        result.push_str(&markdown::list(items));
        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecIf {
    #[serde(rename = "if")]
    pub if_: Exec,
    #[serde(rename = "then")]
    pub then_: Vec<Arc<str>>,
    #[serde(rename = "else")]
    pub else_: Option<Vec<Arc<str>>>,
}

impl ExecIf {
    pub fn execute(
        &self,
        mut progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> Vec<Arc<str>> {
        let condition_result = self.if_.execute(&mut progress, workspace.clone(), name);
        let mut result = Vec::new();
        match condition_result {
            Ok(_) => {
                logger(&mut progress, name).trace("exec condition succeeded");
                result.clone_from(&self.then_);
            }
            Err(_) => {
                logger(&mut progress, name).trace("exec condition failed running");
                if let Some(else_) = self.else_.as_ref() {
                    result.clone_from(else_);
                }
            }
        }
        logger(&mut progress, name).trace(format!("exec if enable targets: {result:?}",).as_str());

        result
    }
}
