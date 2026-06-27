use crate::{singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use utils::{ecode, lock, logger, placeholder, rule};

pub use rule::Expect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UseWorkspaceEnv {
    No,
    Yes,
}

#[derive(Debug, Clone, Default)]
struct State {
    processes: HashMap<String, u32>,
}

static STATE: state::InitCell<lock::StateLock<State>> = state::InitCell::new();

fn logger(console: console::Console, name: &str) -> logger::Logger {
    logger::Logger::new(console, name.into())
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

fn expand_exit_value_tokens(
    value: &str,
    workspace: &workspace::WorkspaceArc,
) -> anyhow::Result<Arc<str>> {
    placeholder::expand_placeholders(
        value,
        rule::EXIT_VALUE_MARKER,
        |dep_rule| {
            let exit_code = workspace
                .read()
                .settings
                .bin
                .exit_codes
                .get(dep_rule)
                .copied()
                .ok_or_else(|| format_error!("No exit value stored for rule '{}'", dep_rule))?;
            Ok(exit_code.to_string())
        },
        || format_error!("Unclosed $RUN_LOAD_EXIT_VALUE{{}} token in: {}", value),
    )
    .map(Into::into)
}

fn expand_file_tokens(
    value: &str,
    workspace_root: &str,
    working_directory: &str,
) -> anyhow::Result<Arc<str>> {
    placeholder::expand_placeholders(
        value,
        rule::FILE_CONTENT_MARKER,
        |file_path| {
            let abs_path = if let Some(ws_relative) = file_path.strip_prefix("//") {
                format!("{workspace_root}/{ws_relative}")
            } else {
                format!("{working_directory}/{file_path}")
            };
            std::fs::read_to_string(&abs_path).with_context(|| {
                format_context!("Failed to read $RUN_LOAD_FILE_CONTENTS{{{}}}", file_path)
            })
        },
        || format_error!("Unclosed $RUN_LOAD_FILE_CONTENTS{{}} token in: {}", value),
    )
    .map(Into::into)
}

fn expand_env_tokens(
    value: &str,
    env_vars: &HashMap<Arc<str>, Arc<str>>,
) -> anyhow::Result<Arc<str>> {
    placeholder::expand_placeholders(
        value,
        rule::ENV_MARKER,
        |key| {
            env_vars
                .get(key)
                .cloned()
                .ok_or_else(|| format_error!("No env value stored for key '{}'", key))
        },
        || format_error!("Unclosed $RUN_LOAD_ENV{{}} token in: {}", value),
    )
    .map(Into::into)
}

fn expand_exec_tokens(
    rule_name: &str,
    value: &str,
    workspace: &workspace::WorkspaceArc,
    workspace_root: &str,
    working_directory: &str,
    env_vars: &HashMap<Arc<str>, Arc<str>>,
    context: &str,
) -> anyhow::Result<Arc<str>> {
    let expanded = expand_file_tokens(value, workspace_root, working_directory).map_err(|err| {
        ecode::anyhow(
            ecode::Ecode::ExecExecutorOperationFailed,
            &format!(
                "{rule_name} Failed to expand $RUN_LOAD_FILE_CONTENTS tokens in {context}\n{err:?}"
            ),
        )
    })?;

    let expanded = expand_exit_value_tokens(&expanded, workspace).map_err(|err| {
        ecode::anyhow(
            ecode::Ecode::ExecExecutorOperationFailed,
            &format!(
                "{rule_name} Failed to expand $RUN_LOAD_EXIT_VALUE tokens in {context}\n{err:?}"
            ),
        )
    })?;

    expand_env_tokens(&expanded, env_vars).map_err(|err| {
        ecode::anyhow(
            ecode::Ecode::ExecExecutorOperationFailed,
            &format!("{rule_name} Failed to expand $RUN_LOAD_ENV tokens in {context}\n{err:?}"),
        )
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exec {
    pub command: Arc<str>,
    pub args: Option<Vec<Arc<str>>>,
    pub env: Option<BTreeMap<Arc<str>, Arc<str>>>,
    pub working_directory: Option<Arc<str>>,
    pub redirect_stdout: Option<Arc<str>>,
    pub expect: Option<Expect>,
    pub log_level: Option<console::Level>,
    pub timeout: Option<f64>,
}

impl Exec {
    fn log_failed_execution(&self, console: console::Console, name: &str, err: &anyhow::Error) {
        singleton::set_is_error_already_reported();
        let mut container = console::bootstrap::Container::new();
        container.add(console::bootstrap::VerticalSpacer::new(1));
        container.add(
            console::bootstrap::Banner::new(format!(
                "{} Failed ",
                console::bootstrap::icon_danger()
            ))
            .width(console::components::Width::Large)
            .variant(console::components::Variant::Danger),
        );

        let args = self.args.as_deref().unwrap_or_default();
        container.add(
            console::components::DescriptionList::new()
                .variant(console::components::Variant::Primary)
                .item("rule:", name)
                .item("command:", format!("{} {}", self.command, args.join(" ")))
                .item(
                    "directory:",
                    self.working_directory.as_deref().unwrap_or("//"),
                )
                .compact(true),
        );

        container.add(
            console::bootstrap::Header::new(console::bootstrap::HeaderLevel::H3, "stderr")
                .variant(console::components::Variant::Default),
        );

        let mut error_quote =
            console::bootstrap::Blockquote::new().variant(console::bootstrap::Variant::Danger);
        for line in err.chain() {
            error_quote.push_line(line.to_string());
        }

        container.add(error_quote);

        // Divider that visually separates the metadata above from the log body below.
        container.add(
            console::components::Divider::new()
                .style(console::components::DividerStyle::Double)
                .width(console::components::Width::Large),
        );

        console.emit_container(&container);
    }

    pub fn execute(
        &self,
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
        use_workspace_env: UseWorkspaceEnv,
    ) -> anyhow::Result<()> {
        let mut arguments = self.args.clone().unwrap_or_default();
        let all_env_vars = workspace.read().get_frozen_env_vars();

        let mut exec_env_vars: HashMap<Arc<str>, Arc<str>> =
            if use_workspace_env == UseWorkspaceEnv::Yes {
                all_env_vars.as_ref().clone()
            } else {
                all_env_vars
                    .iter()
                    .filter(|(key, _)| key.as_ref() == "PATH")
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            };

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

        exec_env_vars.insert("PWD".into(), pwd.clone());

        let workspace_root = absolute_path_to_workspace.as_ref();
        let working_dir = pwd.as_ref();

        for (key, value) in self.env.clone().unwrap_or_default() {
            let context = format!("env var {key}");
            let expanded = expand_exec_tokens(
                name,
                &value,
                &workspace,
                workspace_root,
                working_dir,
                &exec_env_vars,
                &context,
            )?;
            exec_env_vars.insert(key, expanded);
        }

        for arg in arguments.iter_mut() {
            let context = format!("arg {arg}");
            *arg = expand_exec_tokens(
                name,
                arg,
                &workspace,
                workspace_root,
                working_dir,
                &exec_env_vars,
                &context,
            )?;
        }

        let command_line_target = workspace.read().target.clone();

        let mut log_level = self.log_level;
        if let Some(target) = command_line_target {
            let trailing_args = {
                let workspace_read = workspace.read();
                if let Some(mapped_rule) = workspace_read
                    .settings
                    .bin
                    .trailing_args_rule_map
                    .get(target.as_ref())
                {
                    if mapped_rule.as_ref() == name {
                        workspace_read.trailing_args.clone()
                    } else {
                        Vec::new()
                    }
                } else if target.as_ref() == name {
                    workspace_read.trailing_args.clone()
                } else {
                    Vec::new()
                }
            };

            let is_trailing_args_empty = trailing_args.is_empty();
            arguments.extend(trailing_args);
            if log_level.is_none() && !is_trailing_args_empty {
                log_level = Some(console::Level::Passthrough);
            }
        }

        let environment = exec_env_vars.into_iter().collect::<Vec<_>>();

        let log_file_path = if singleton::get_is_logging_disabled() {
            None
        } else {
            Some(workspace.read().get_log_file(name))
        };

        logger(progress.console.clone(), name)
            .debug(format!("log file: {log_file_path:?}",).as_str());
        logger(progress.console.clone(), name).debug(format!("Env: {environment:?}",).as_str());

        let options = console::ExecuteOptions {
            label: name.into(),
            arguments,
            environment,
            working_directory,
            is_return_stdout: self.redirect_stdout.is_some(),
            is_return_stderr: false,
            log_file_path: log_file_path.clone(),
            clear_environment: true,
            process_started_with_id: Some(handle_process_started),
            log_level,
            timeout: self.timeout.map(std::time::Duration::from_secs_f64),
            allow_failure: true,
        };

        let rule_name_for_file = name.replace(['/', ':'], "_");
        let lock_file_path = format!(
            ".spaces/locks/{}.{}",
            rule_name_for_file,
            lock::LOCK_FILE_SUFFIX
        );
        let mut file_lock = lock::FileLock::new(std::path::Path::new(&lock_file_path).into());
        file_lock.lock(progress.console.clone()).map_err(|err| {
            ecode::anyhow(
                ecode::Ecode::ExecExecutorOperationFailed,
                &format!("Failed to acquire lock for rule {name}\n{err:?}"),
            )
        })?;

        logger(progress.console.clone(), name).info(
            format!(
                "Executing: {} {}",
                self.command,
                options.arguments.join(" ")
            )
            .as_str(),
        );

        let result = progress
            .execute_process(&self.command, options)
            .map_err(|err| {
                self.log_failed_execution(progress.console.clone(), name, &err);
                ecode::anyhow(
                    ecode::Ecode::ExecExecutorOperationFailed,
                    &format!("Error executing {name}\n{err:?}"),
                )
            })?;

        handle_process_ended(name);
        workspace
            .write()
            .settings
            .bin
            .exit_codes
            .insert(name.into(), result.exit_code);

        logger(progress.console.clone(), name)
            .debug(format!("log file: {log_file_path:?}").as_str());

        let stdout_content = if result.exit_code == 0 {
            logger(progress.console.clone(), name).info("succeeded");
            if let Some(Expect::Failure) = self.expect.as_ref() {
                return Err(ecode::anyhow(
                    ecode::Ecode::ExecExecutorOperationFailed,
                    "Expected failure but task succeeded",
                ));
            }
            result.stdout
        } else {
            logger(progress.console.clone(), name).info("Failed");
            if let Some(Expect::Failure) | Some(Expect::Any) = self.expect.as_ref() {
                None
            } else {
                singleton::set_is_error_already_reported();
                if let Some(log_file_path) = log_file_path {
                    if std::path::Path::new(log_file_path.as_ref()).exists() {
                        let mut log_container = console::bootstrap::Container::new();
                        let log_contents = std::fs::read_to_string(log_file_path.as_ref())
                            .map_err(|err| {
                                ecode::anyhow(
                                    ecode::Ecode::ExecExecutorOperationFailed,
                                    &format!("Failed to read log file {}\n{err:?}", log_file_path),
                                )
                            })?;
                        let summary_container = console::format_log_file_summary(
                            name,
                            &log_contents,
                            log_file_path.as_ref(),
                            result.exit_code,
                        );
                        log_container.add(console::bootstrap::VerticalSpacer::new(1));
                        log_container.extend(summary_container);
                        progress.console.emit_container(&log_container);
                    }
                } else {
                    logger(progress.console.clone(), name).error(
                        "No log file is available (log files disabled with the --ci option)",
                    );
                }
                return Err(ecode::anyhow(
                    ecode::Ecode::ExecExecutorOperationFailed,
                    &format!(
                        "Command `{}` failed with exit code: {}",
                        self.command, result.exit_code
                    ),
                ));
            }
        };

        if let (Some(stdout_content), Some(stdout_location)) =
            (stdout_content, self.redirect_stdout.as_ref())
        {
            let parent_path = std::path::Path::new(stdout_location.as_ref())
                .parent()
                .ok_or_else(|| {
                    ecode::anyhow(
                        ecode::Ecode::ExecExecutorOperationFailed,
                        &format!("Failed to get parent directory of {}", stdout_location),
                    )
                })?;

            std::fs::create_dir_all(parent_path).map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::ExecExecutorOperationFailed,
                    &format!(
                        "Failed to create parent directory {:?} for stdout file {}\n{err:?}",
                        parent_path, stdout_location
                    ),
                )
            })?;

            std::fs::write(stdout_location.as_ref(), stdout_content).map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::ExecExecutorOperationFailed,
                    &format!("Failed to write stdout to {}\n{err:?}", stdout_location),
                )
            })?;
        }

        Ok(())
    }

    pub fn to_markdown(&self) -> String {
        use utils::markdown;
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
    pub fn execute(&self, name: &str, progress: &mut console::Progress) -> anyhow::Result<()> {
        if let Some(process_id) = get_process_id(self.target.as_ref()) {
            let options = console::ExecuteOptions {
                label: name.into(),
                arguments: vec![
                    "-s".into(),
                    self.signal.to_kill_arg(),
                    format!("{process_id}").into(),
                ],
                allow_failure: true,
                ..Default::default()
            };

            let result = progress.execute_process("kill", options).map_err(|err| {
                ecode::anyhow(
                    ecode::Ecode::ExecExecutorOperationFailed,
                    &format!("Failed to execute kill\n{err:?}"),
                )
            })?;
            match self.expect.as_ref() {
                Some(Expect::Success) if result.exit_code != 0 => {
                    return Err(ecode::anyhow(
                        ecode::Ecode::ExecExecutorOperationFailed,
                        &format!("Expected success but kill failed {self:?}"),
                    ));
                }
                Some(Expect::Failure) if result.exit_code == 0 => {
                    return Err(ecode::anyhow(
                        ecode::Ecode::ExecExecutorOperationFailed,
                        &format!("Expected failure but kill succeeded {self:?}"),
                    ));
                }
                _ => {}
            }
        } else if let Some(Expect::Success) = self.expect.as_ref() {
            return Err(ecode::anyhow(
                ecode::Ecode::ExecExecutorOperationFailed,
                &format!("No process found for {name}"),
            ));
        }

        Ok(())
    }

    pub fn to_markdown(&self) -> String {
        use utils::markdown;
        let mut result = String::new();
        let invoke = format!("$ kill -s {} {}", self.signal.to_kill_arg(), self.target);
        result.push_str(&markdown::code_block("sh", invoke.as_str()));
        let mut items: Vec<Arc<str>> = Vec::new();
        items.push(format!("Expect: `{}`", self.expect.unwrap_or_default()).into());
        result.push_str(&markdown::list(items));
        result
    }
}
