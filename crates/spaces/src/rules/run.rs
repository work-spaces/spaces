use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::{environment::GlobalsBuilder, values::none::NoneType};
use starstd::{get_rule_argument, Arg, Function};
use std::collections::HashSet;

use crate::{executor, rules};

pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "add_exec",
        description: "Adds a rule that will execute a process.",
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "exec",
                description: "dict with",
                dict: &[
                    ("command", "name of the command to execute"),
                    ("args", "optional list of arguments"),
                    ("env", "optional dict of environment variables"),
                    ("working_directory", "optional working directory (default is the workspace)"),
                    ("expect", "Failure: expect non-zero return code|Success: expect zero return code"),
                ],
            },
        ],
        example: Some(r#"run.add_exec(
    rule = {"name": name, "type": "Setup", "deps": ["sysroot-python:venv"]},
    exec = {
        "command": "pip3",
        "args": ["install"] + packages,
    },
)"#)},
Function {
    name: "add_exec_if",
    description: "Adds a rule to execute if a condition is met.",
    return_type: "None",
    args: &[
        get_rule_argument(),
        Arg {
            name: "exec_if",
            description: "dict with",
            dict: &[
                ("if", "this is an `exec` object used with add_exec()"),
                ("then", "list of optional targets to enable if the command has the expected result"),
                ("else", "optional list of optional targets to enable if the command has the unexpected result"),
            ],
        },
    ],
    example: Some(r#"run.add_exec(
    rule = {"name": create_file, "type": "Optional" },
    exec = {
        "command": "touch",
        "args": ["some_file"],
    },
)

run.add_exec_if(
rule = {"name": check_file, "deps": []},
exec_if = {
    "if": {
        "command": "ls",
        "args": [
            "some_file",
        ],
        "expect": "Failure",
    },
    "then": ["create_file"],
},
)"#)},
Function {
    name: "add_target",
    description: "Adds a target. There is no specific action for the target, but this rule can be useful for organizing depedencies.",
    return_type: "None",
    args: &[
        get_rule_argument(),
    ],
    example: Some(r#"run.add_target(
    rule = {"name": "my_rule", "deps": ["my_other_rule"]},
)"#)}
];

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn add_target(
        #[starlark(require = named)] rule: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let state = rules::get_state().read().unwrap();
        let rule_name = rule.name.clone();
        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Run,
                executor::Task::Target,
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_exec(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] exec: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let mut exec: executor::exec::Exec = serde_json::from_value(exec.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        if let Some(redirect_stdout) = exec.redirect_stdout.as_mut() {
            *redirect_stdout = format!(
                "{}/{}",
                rules::get_path_to_build_checkout(rule.name.as_str())?,
                redirect_stdout
            );
        }

        let state = rules::get_state().read().unwrap();
        let rule_name = rule.name.clone();
        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Run,
                executor::Task::Exec(exec),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_exec_if(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] exec_if: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let mut exec_if: executor::exec::ExecIf = serde_json::from_value(exec_if.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        if let Some(redirect_stdout) = exec_if.if_.redirect_stdout.as_mut() {
            *redirect_stdout = format!(
                "{}/{}",
                rules::get_path_to_build_checkout(rule.name.as_str())?,
                redirect_stdout
            );
        }

        let state = rules::get_state().read().unwrap();

        for target in exec_if.then_.iter_mut() {
            *target = state.get_sanitized_rule_name(target);
        }

        if let Some(else_targets) = exec_if.else_.as_mut() {
            for target in else_targets.iter_mut() {
                *target = state.get_sanitized_rule_name(target);
            }
        }

        let rule_name = rule.name.clone();
        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Run,
                executor::Task::ExecIf(exec_if),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] archive: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let mut rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add_archive rule"))?;

        if rule.inputs.is_some() {
            return Err(anyhow::anyhow!(
                "inputs are populated automatically by add_archive"
            ));
        }

        if rule.outputs.is_some() {
            return Err(anyhow::anyhow!(
                "inputs are populated automatically by add_archive"
            ));
        }

        let create_archive: easy_archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let state = rules::get_state().read().unwrap();
        let rule_name = rule.name.clone();
        if false {
            let mut inputs = HashSet::new();
            inputs.insert(format!("{}/**", create_archive.input));
            rule.inputs = Some(inputs);

            let mut outputs = HashSet::new();
            outputs.insert(format!(
                "build/{}/{}",
                state.get_sanitized_rule_name(rule_name.as_str()),
                create_archive.get_output_file()
            ));
            rule.outputs = Some(outputs);
        }

        let archive = executor::archive::Archive { create_archive };

        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Run,
                executor::Task::CreateArchive(archive),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }
}
