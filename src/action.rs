use crate::{anyhow_error, context, format_error_context, manifest};
use anyhow::Context;

fn substitute(context: std::sync::Arc<context::Context>, input: String) -> anyhow::Result<String> {
    let mut output = input.clone();
    let substitutions = &context.substitutions;
    for (key, (value, _)) in substitutions.iter() {
        if let Some(value) = value {
            output = output.replace(key, value);
        }
    }

    if let Some(location) = output.find(context::SPACES_TOML) {
        let len = context::SPACES_TOML.len();
        let token_start = location + len;
        let token_end = output[token_start..].find('}').ok_or(anyhow_error!(
            "Invalid toml token {input} didn't find closing `}}`"
        ))?;
        let replace = &output[location..token_start + token_end + 1];

        let token_path = &output[token_start..token_start + token_end];
        let path = std::path::Path::new(token_path);
        let file_path = path.parent().ok_or(anyhow_error!(
            "Invalid toml token {input} no file specified for {path:?}"
        ))?;
        let toml_access = path
            .file_name()
            .ok_or(anyhow_error!(
                "Invalid toml token {input} no / separator for {path:?}"
            ))?
            .to_string_lossy();

        if let Some(ext) = file_path.extension() {
            if ext != "toml" {
                Err(anyhow_error!(
                    "Invalid toml token {input} does not end with .toml: {file_path:?}"
                ))?;
            }
        } else {
            Err(anyhow_error!(
                "Invalid toml token {input} cannot determine extension for: {file_path:?}"
            ))?;
        }

        let contents = std::fs::read_to_string(file_path)
            .with_context(|| format_error_context!("Failed to read file: {file_path:?}"))?;
        let toml: toml::Value = toml::from_str(contents.as_str())
            .with_context(|| format_error_context!("Failed to parse toml file: {file_path:?}"))?;
        let parts = toml_access.split('.');
        let mut toml_value = &toml;
        for part in parts {
            toml_value = toml_value
                .get(part)
                .ok_or(anyhow_error!("Invalid toml token while inspecting {part} in {input}"))?;
        }
        let toml_string_value = toml_value.as_str().ok_or(anyhow_error!(
            "Invalid toml token {input} must be string, not {toml_value:?}"
        ))?;

        output = output.replace(replace, toml_string_value);
    }

    Ok(output)
}

fn execute_action(
    context: std::sync::Arc<context::Context>,
    printer: &mut printer::Printer,
    actions: Vec<manifest::Action>,
) -> anyhow::Result<()> {
    for step in actions {
        //execute substitutions in the action

        let environment = if let Some(step_env) = step.environment {
            let mut result = Vec::new();
            for (key, value) in step_env {
                result.push((
                    substitute(context.clone(), key.clone()).with_context(|| {
                        format_error_context!("substitution failed ENV key {key}")
                    })?,
                    substitute(context.clone(), value.clone()).with_context(|| {
                        format_error_context!("substitution failed ENV value {value}")
                    })?,
                ));
            }
            result
        } else {
            Vec::new()
        };

        let working_directory = if let Some(directory) = step.working_directory {
            Some(
                substitute(context.clone(), directory.clone()).with_context(|| {
                    format_error_context!("substitution faile for working directory {directory}")
                })?,
            )
        } else {
            None
        };

        let arguments = if let Some(args) = step.arguments {
            let mut result = Vec::new();
            for arg in args {
                result.push(substitute(context.clone(), arg.clone()).with_context(|| {
                    format_error_context!("substitution failed for argument {arg}")
                })?);
            }
            result
        } else {
            Vec::new()
        };

        let options = printer::ExecuteOptions {
            working_directory,
            environment,
            arguments,
            label: step.name.clone(),
        };

        if step.display == Some(true) {
            printer
                .execute_process(step.command.as_str(), &options)
                .with_context(|| {
                    format_error_context!(
                        "failed: {}",
                        options.get_full_command_in_working_directory(step.command.as_str())
                    )
                })?;
        } else {
            let mut multi_progress = printer::MultiProgress::new(printer);
            let mut progress = multi_progress.add_progress(step.name.as_str(), None, None);
            progress
                .execute_process(step.command.as_str(), &options)
                .with_context(|| {
                    format_error_context!(
                        "failed: {}",
                        options.get_full_command_in_working_directory(step.command.as_str())
                    )
                })?;
        }
    }
    Ok(())
}

pub fn run_action(context: context::Context, name: String) -> anyhow::Result<()> {
    let context = std::sync::Arc::new(context);

    let mut printer = context
        .printer
        .write()
        .expect("Internal Error: Printer is not set");

    let full_path = context.current_directory.clone();
    let workspace = manifest::Workspace::new(&full_path)
        .with_context(|| format_error_context!("{full_path} when running action in workspace"))?;

    if let Some(actions) = workspace.actions {
        if let Some(action_list) = actions.get(name.as_str()) {
            execute_action(context.clone(), &mut printer, action_list.clone())?;
            Ok(())
        } else {
            Err(anyhow_error!("No actions found with this name"))?
        }
    } else {
        Err(anyhow_error!("No actions found in the workspace"))?
    }
}

pub fn show_actions(context: context::Context) -> anyhow::Result<()> {
    let full_path = context.current_directory.clone();

    let mut printer = context
        .printer
        .write()
        .expect("Internal Error: Printer is not set");

    let workspace = manifest::Workspace::new(&full_path)
        .with_context(|| format_error_context!("{full_path} when building workspace"))?;

    if let Some(actions) = workspace.actions {
        printer.info("actions", &actions)?;
        Ok(())
    } else {
        Err(anyhow_error!("No actions found in the workspace"))?
    }
}
