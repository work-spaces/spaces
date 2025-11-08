use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::{environment::GlobalsBuilder, values::none::NoneType};
use starstd::{get_rule_argument, Arg, Function};
use std::collections::HashSet;

use crate::{executor, rules, singleton, task};

const ADD_ARCHIVE_EXAMPLE: &str = r#"run.add_archive(
    rule = {"name": name, "type": "Optional", "deps": ["sysroot-python:venv"]},
    archive = {
        "input": "build/install",
        "name": "my_archive",
        "version": "1.0",
        "driver": "tar.gz",
    },
)"#;

const ADD_EXEC_EXAMPLE: &str = r#"run.add_exec(
    rule = {"name": name, "type": "Setup", "deps": ["sysroot-python:venv"]},
    exec = {
        "command": "pip3",
        "args": ["install"] + packages,
    },
)"#;

const ADD_TARGET_EXAMPLE: &str = r#"run.add_target(
    rule = {"name": "my_rule", "deps": ["my_other_rule"]},
)"#;

pub const FUNCTIONS: &[Function] = &[
    Function {
        name: "add_archive",
        description: "Adds a rule that will archive a directory.",
        return_type: "None",
        args: &[
            get_rule_argument(),
            Arg {
                name: "archive",
                description: "dict with",
                dict: &[
                    ("input", "path to the directory to archive"),
                    ("name", "name of the archive"),
                    ("version", "archive version (will be part of the filename)"),
                    ("driver", "The compression driver: tar.gz|tar.bz2|zip|tar.7z|tar.xz"),
                    ("platform", "Optionally specify the platform"),
                    ("includes", "Optional list of glob patterns applied to the input directory for inclusion"),
                    ("excludes", "Optional list of glob patterns applied to the input directory for exclusion"),
                ],
            },
        ],
        example: Some(ADD_ARCHIVE_EXAMPLE)},
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
        name: "add_target",
        description: "Adds a target. There is no specific action for the target, but this rule can be useful for organizing dependencies.",
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

fn add_rule_to_all(rule: &rule::Rule) -> anyhow::Result<()> {
    if let Some(rule::RuleType::Run) = rule.type_.as_ref() {
        let workspace = singleton::get_workspace()
            .context(format_context!("Internal Error: workspace not available"))?;
        let mut workspace = workspace.write();
        workspace
            .settings
            .bin
            .run_all
            .insert(rules::get_sanitized_rule_name(rule.name.clone()));
    }
    Ok(())
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn abort(message: &str) -> anyhow::Result<NoneType> {
        Err(format_error!("Run Aborting: {}", message))
    }

    fn add_target(
        #[starlark(require = named)] rule: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add target rule"))?;

        add_rule_to_all(&rule)
            .context(format_context!("Internal Error: Failed to add rule to all"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Run,
            executor::Task::Target,
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_exec(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] exec: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for exec rule"))?;

        inputs::validate_input_globs(&rule.inputs)
            .context(format_context!("invalid inputs globs with {}", rule.name))?;

        add_rule_to_all(&rule)
            .context(format_context!("Internal Error: Failed to add rule to all"))?;

        let mut exec: executor::exec::Exec = serde_json::from_value(exec.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        if let Some(working_directory) = exec.working_directory.as_mut() {
            *working_directory = rules::get_sanitized_working_directory(working_directory.clone());
        }

        if let Some(redirect_stdout) = exec.redirect_stdout.as_mut() {
            *redirect_stdout = format!("build/{redirect_stdout}").into();
        }
        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Run,
            executor::Task::Exec(exec),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_kill_exec(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] kill: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for kill rule"))?;

        inputs::validate_input_globs(&rule.inputs)
            .context(format_context!("invalid inputs globs with {}", rule.name))?;

        add_rule_to_all(&rule)
            .context(format_context!("Internal Error: Failed to add rule to all"))?;

        let mut kill_exec: executor::exec::Kill = serde_json::from_value(kill.to_json_value()?)
            .context(format_context!("bad options for kill"))?;
        kill_exec.target = rules::get_sanitized_rule_name(kill_exec.target.clone());

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Run,
            executor::Task::Kill(kill_exec),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] archive: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let mut rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
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

        let mut create_archive: archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let rule_name = rule.name.clone();
        let mut inputs = HashSet::new();

        let input = create_archive
            .input
            .strip_prefix("//")
            .unwrap_or(&create_archive.input)
            .to_owned();
        create_archive.input = input;

        inputs.insert(format!("+//{}/**", create_archive.input).into());
        rule.inputs = Some(inputs);

        let mut outputs = HashSet::new();
        outputs.insert(
            format!(
                "build/{}/{}",
                rules::get_sanitized_rule_name(rule_name.clone()),
                create_archive.get_output_file()
            )
            .into(),
        );
        rule.outputs = Some(outputs);

        let archive = executor::archive::Archive { create_archive };

        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Run,
            executor::Task::CreateArchive(archive),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }
}
