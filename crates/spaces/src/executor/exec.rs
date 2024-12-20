use crate::{singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
struct State {
    processes: HashMap<String, u32>,
}

static STATE: state::InitCell<lock::StateLock<State>> = state::InitCell::new();

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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Expect {
    Failure,
    Success,
    Any,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let arguments = self.args.clone().unwrap_or_default();
        let workspace_env = workspace.read().get_env();

        let mut environment_map = workspace_env
            .get_vars()
            .context(format_context!("Failed to get env vars"))?;

        for (key, value) in self.env.clone().unwrap_or_default() {
            environment_map.insert(key, value);
        }

        let workspace_path = workspace.read().get_absolute_path();
        let environment = environment_map.into_iter().collect::<Vec<_>>();

        let log_file_path = if singleton::get_is_ci() {
            None
        } else {
            Some(workspace.read().get_log_file(name))
        };

        let options = printer::ExecuteOptions {
            label: name.to_string(),
            arguments,
            environment,
            working_directory: self
                .working_directory
                .clone()
                .map(|cwd| format!("{workspace_path}/{cwd}")),
            is_return_stdout: self.redirect_stdout.is_some(),
            log_file_path: log_file_path.clone(),
            clear_environment: true,
            process_started_with_id: Some(handle_process_started),
        };

        progress.log(
            printer::Level::Debug,
            format!("exec {name}: {} {options:?}", self.command).as_str(),
        );

        let result = progress.execute_process(&self.command, options);

        handle_process_ended(name);

        progress.log(
            printer::Level::Message,
            format!("log file for {name}: {log_file_path:?}").as_str(),
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
                } else if let Some(Expect::Any) = self.expect.as_ref() {
                    None
                } else {
                    // if the command failed to execute, there won't be a log file
                    if let Some(log_file_path) = log_file_path {
                        if std::path::Path::new(log_file_path.as_str()).exists() {
                            let log_contents = std::fs::read_to_string(&log_file_path).context(
                                format_context!("Failed to read log file {}", log_file_path),
                            )?;
                            if log_contents.len() > 8192 {
                                progress.log(
                                    printer::Level::Error,
                                    format!("See log file {log_file_path} for details").as_str(),
                                );
                            } else {
                                progress.log(printer::Level::Error, log_contents.as_str());
                            }
                        }
                    } else {
                        progress.log(
                            printer::Level::Error,
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
            std::fs::write(stdout_location, stdout_content).context(format_context!(
                "Failed to write stdout to {}",
                stdout_location
            ))?;
        }

        Ok(())
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
    fn to_kill_arg(&self) -> &str {
        match self {
            Signal::Hup => "HUP",
            Signal::Int => "INT",
            Signal::Quit => "QUIT",
            Signal::Abort => "ABRT",
            Signal::Kill => "KILL",
            Signal::Alarm => "ALRM",
            Signal::Terminate => "TERM",
            Signal::User1 => "USR1",
            Signal::User2 => "USR2",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kill {
    pub signal: Signal,
    pub target: String,
    pub expect: Option<Expect>,
}

impl Kill {
    pub fn execute(
        &self,
        name: &str,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        if let Some(process_id) = get_process_id(self.target.as_str()) {
            let options = printer::ExecuteOptions {
                label: name.to_string(),
                arguments: vec![
                    "-s".to_string(),
                    self.signal.to_kill_arg().to_string(),
                    format!("{}", process_id),
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
        } else {
            match self.expect.as_ref() {
                Some(Expect::Success) => {
                    return Err(format_error!("No process found for {name}"));
                }
                _ => {}
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecIf {
    #[serde(rename = "if")]
    pub if_: Exec,
    #[serde(rename = "then")]
    pub then_: Vec<String>,
    #[serde(rename = "else")]
    pub else_: Option<Vec<String>>,
}

impl ExecIf {
    pub fn execute(&self, 
        mut progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str) -> Vec<String> {
        let condition_result = self.if_.execute(&mut progress, workspace.clone(), name);
        let mut result = Vec::new();
        match condition_result {
            Ok(_) => {
                progress.log(
                    printer::Level::Trace,
                    format!("exec {name} condition succeeded").as_str(),
                );
                result.clone_from(&self.then_);
            }
            Err(_) => {
                progress.log(
                    printer::Level::Trace,
                    format!("exec {name} condition failed running").as_str(),
                );
                if let Some(else_) = self.else_.as_ref() {
                    result.clone_from(else_);
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
