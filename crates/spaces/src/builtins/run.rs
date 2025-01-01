use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::{environment::GlobalsBuilder, values::none::NoneType};
use starstd::{get_rule_argument, Arg, Function};
use std::collections::HashSet;

use crate::{executor, rules, inputs};

const ADD_EXEC_EXAMPLE: &str = r#"run.add_exec(
    rule = {"name": name, "type": "Setup", "deps": ["sysroot-python:venv"]},
    exec = {
        "command": "pip3",
        "args": ["install"] + packages,
    },
)"#;

const ADD_EXEC_IF_EXAMPLE: &str = r#"run.add_exec(
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
    }
)"#;

const ADD_TARGET_EXAMPLE: &str = r#"run.add_target(
    rule = {"name": "my_rule", "deps": ["my_other_rule"]},
)"#;

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
                    ("expect", "Failure: expect non-zero return code|Success: expect zero return code|Any: don't check the return code"),
                    ("redirect_stdout", "optional file to redirect stdout to"),
                ],
            },
        ],
        example: Some(ADD_EXEC_EXAMPLE)},
    Function {
        name: "add_kill_exec",
        description: "Adds a rule that will kill the execution of another rule.",
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "kill",
                description: "dict with",
                dict: &[
                    ("signal", "Hup|Int|Quit|Abort|Kill|Alarm|Terminate|User1|User2"),
                    ("target", "the name of the rule to kill"),
                    ("expect", "Failure: expect non-zero return code|Success: expect zero return code|Any: don't check the return code"),
                ],
            },
        ],
        example: Some(ADD_EXEC_EXAMPLE)},
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
        example: Some(ADD_EXEC_IF_EXAMPLE)},
    Function {
        name: "add_target",
        description: "Adds a target. There is no specific action for the target, but this rule can be useful for organizing depedencies.",
        return_type: "None",
        args: &[
            get_rule_argument(),
        ],
        example: Some(ADD_TARGET_EXAMPLE)},
    Function {
        name: "abort",
        description: "Abort script evaluation with a message.",
        return_type: "None",
        args: &[
            Arg {
                name: "message",
                description: "Abort message to show the user.",
                dict: &[],
            },
        ],
        example: Some(r#"run.abort("Failed to do something")"#)}
];

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn abort(message: &str) -> anyhow::Result<NoneType> {
        Err(format_error!("Run Aborting: {}", message))
    }

    fn add_target(
        #[starlark(require = named)] rule: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add target rule"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(rules::Task::new(
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
            .context(format_context!("bad options for exec rule"))?;

        inputs::validate_input_globs(&rule.inputs)
            .context(format_context!("invalid inputs globs with {}", rule.name))?;

        let mut exec: executor::exec::Exec = serde_json::from_value(exec.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        if let Some(redirect_stdout) = exec.redirect_stdout.as_mut() {
            *redirect_stdout = format!(
                "{}/{}",
                rules::get_path_to_build_checkout(rule.name.clone())?,
                redirect_stdout
            ).into();
        }
        let rule_name = rule.name.clone();
        rules::insert_task(rules::Task::new(
            rule,
            rules::Phase::Run,
            executor::Task::Exec(exec),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_kill_exec(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] kill: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for kill rule"))?;

        inputs::validate_input_globs(&rule.inputs)
            .context(format_context!("invalid inputs globs with {}", rule.name))?;

        let mut kill_exec: executor::exec::Kill = serde_json::from_value(kill.to_json_value()?)
            .context(format_context!("bad options for kill"))?;
        kill_exec.target = rules::get_sanitized_rule_name(kill_exec.target.clone());

        let rule_name = rule.name.clone();
        rules::insert_task(rules::Task::new(
            rule,
            rules::Phase::Run,
            executor::Task::Kill(kill_exec),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_exec_if(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] exec_if: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for exec_if rule"))?;

        inputs::validate_input_globs(&rule.inputs)
            .context(format_context!("invalid inputs globs with {}", rule.name))?;

        let mut exec_if: executor::exec::ExecIf = serde_json::from_value(exec_if.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        if let Some(redirect_stdout) = exec_if.if_.redirect_stdout.as_mut() {
            *redirect_stdout = format!(
                "{}/{}",
                rules::get_path_to_build_checkout(rule.name.clone())?,
                redirect_stdout
            ).into();
        }

        for target in exec_if.then_.iter_mut() {
            *target = rules::get_sanitized_rule_name(target.clone());
        }

        if let Some(else_targets) = exec_if.else_.as_mut() {
            for target in else_targets.iter_mut() {
                *target = rules::get_sanitized_rule_name(target.clone());
            }
        }

        let rule_name = rule.name.clone();
        rules::insert_task(rules::Task::new(
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
                "outputs are populated automatically by add_archive"
            ));
        }

        let create_archive: easy_archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let rule_name = rule.name.clone();
        let mut inputs = HashSet::new();
        inputs.insert(format!("+{}/**", create_archive.input).into());
        rule.inputs = Some(inputs);

        let mut outputs = HashSet::new();
        outputs.insert(format!(
            "build/{}/{}",
            rules::get_sanitized_rule_name(rule_name.clone()),
            create_archive.get_output_file()
        ).into());
        rule.outputs = Some(outputs);

        let archive = executor::archive::Archive { create_archive };

        rules::insert_task(rules::Task::new(
            rule,
            rules::Phase::Run,
            executor::Task::CreateArchive(archive),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }
}
