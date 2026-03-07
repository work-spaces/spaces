use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::{environment::GlobalsBuilder, values::none::NoneType};

use utils::{rule, targets};

use crate::{executor, rules, singleton, task};

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

/// These are the functions available in the `run` module.
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Abort script evaluation with a message.
    ///
    /// ```python
    /// run.abort("Failed to do something")
    /// ```
    ///
    /// # Arguments
    /// * `message`: Abort message to show the user.
    fn abort(message: &str) -> anyhow::Result<NoneType> {
        if singleton::is_lsp_mode() {
            Ok(NoneType)
        } else {
            Err(format_error!("Run Aborting: {}", message))
        }
    }

    /// Adds a rule that depends on other rules.
    ///
    /// There is no specific action for the rule, but this rule can be useful for organizing dependencies.
    ///
    /// This function is not properly named. `target` is not used correctly in this context.
    ///
    /// ```python
    /// run.add_target(
    ///     rule = {
    ///         "name": "my_rule",
    ///         "deps": ["my_other_rule"],
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: Rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
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

    /// Adds a rule that will execute a process.
    ///
    /// ```python
    /// run.add_exec(
    ///     rule = {
    ///         "name": name,
    ///         "type": "Setup",
    ///         "deps": ["sysroot-python:venv"],
    ///     },
    ///     exec = {
    ///         "command": "pip3",
    ///         "args": ["install"] + packages,
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: Rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `exec`: Execution details containing `command` (`str`), `args` (`list`), `env` (`dict`), `working_directory` (`str`), `expect` (`Failure`|`Success`|`Any`), and `redirect_stdout` (`str`).
    fn add_exec(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] exec: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for exec rule"))?;

        if let Some(inputs) = rule.inputs.as_ref() {
            for glob in inputs {
                if !glob.starts_with('+') && !glob.starts_with('-') {
                    return Err(format_error!(
                        "Invalid glob: {glob:?}. Must begin with '+' (includes) or '-' (excludes) in {}",
                        rule.name
                    ));
                }
            }
        }

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

    /// Adds a rule that will kill the execution of another rule.
    ///
    /// ```python
    /// run.add_kill_exec(
    ///     rule = {"name": "stop_service", "type": "Run"},
    ///     kill = {
    ///         "signal": "Terminate",
    ///         "target": "my_long_running_service",
    ///         "expect": "Success",
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: Rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `kill`: Kill details containing `signal` (`Hup`|`Int`|`Quit`|`Abort`|`Kill`|`Alarm`|`Terminate`|`User1`|`User2`), `target` (`str`), and `expect` (`Failure`|`Success`|`Any`).
    fn add_kill_exec(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] kill: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for kill rule"))?;

        if let Some(inputs) = rule.inputs.as_ref() {
            for glob in inputs {
                if !glob.starts_with('+') && !glob.starts_with('-') {
                    return Err(format_error!(
                        "Invalid glob: {glob:?}. Must begin with '+' (includes) or '-' (excludes) in {}",
                        rule.name
                    ));
                }
            }
        }

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

    /// Adds a rule that will archive a directory.
    ///
    /// ```python
    /// run.add_archive(
    ///     rule = {"name": name, "type": "Optional", "deps": ["sysroot-python:venv"]},
    ///     archive = {
    ///         "input": "build/install",
    ///         "name": "my_archive",
    ///         "version": "1.0",
    ///         "driver": "tar.gz",
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: Rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `archive`: Archive details containing `input` (`str`), `name` (`str`), `version` (`str`), `driver` (`tar.gz`|`tar.bz2`|`zip`|`tar.7z`|`tar.xz`), `platform` (`str`), `includes` (`list`), and `excludes` (`list`).
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

        if rule.targets.is_some() {
            return Err(anyhow::anyhow!(
                "outputs are populated automatically by add_archive"
            ));
        }

        let mut create_archive: archiver::CreateArchive =
            serde_json::from_value(archive.to_json_value()?)
                .context(format_context!("bad options for archive"))?;

        let rule_name = rule.name.clone();

        let input = create_archive
            .input
            .strip_prefix("//")
            .unwrap_or(&create_archive.input)
            .to_owned();
        create_archive.input = input;

        // Add archive input globs to deps without clobbering existing deps
        let includes = vec![format!("//{}/**", create_archive.input).into()];
        rule::Deps::push_any_dep(
            &mut rule.deps,
            rule::AnyDep::Glob(rule::Globs::Includes(includes)),
        );

        let target_path = format!(
            "build/{}/{}",
            rules::get_sanitized_rule_name(rule_name.clone()),
            create_archive.get_output_file()
        )
        .into();
        rule.push_target(targets::Target::Directory(target_path));

        let archive = executor::archive::Archive { create_archive };

        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Run,
            executor::Task::CreateArchive(archive),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    /// Adds a rule that will execute based on the cloned from rule.
    ///
    /// The new rule is merged with the cloned rule: the new rule's fields take precedence,
    /// and the exec/target is taken from the cloned rule.
    ///
    /// ```python
    /// run.add_from_clone(
    ///     rule = {
    ///         "name": "my_new_rule",
    ///         "deps": ["other_dep"],
    ///     },
    ///     clone_from = "existing_rule",
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: Rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `clone_from`: The name of an existing rule whose exec will be cloned.
    fn add_from_clone(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] clone_from: &str,
    ) -> anyhow::Result<NoneType> {
        let mut rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add_exec_from_clone rule"))?;

        if singleton::is_lsp_mode() {
            return Ok(NoneType);
        }

        let cloned_task = rules::get_cloned_task(clone_from)
            .context(format_context!("Failed to clone task {}", clone_from))?;

        match &cloned_task.executor {
            executor::Task::Exec(_) => (),
            executor::Task::Target => (),
            _ => {
                return Err(format_error!(
                    "clone_from rule {} does not have an Exec/Target task",
                    clone_from
                ));
            }
        };

        // Merge: cloned rule fields are used as defaults, new rule fields take precedence
        let cloned_rule = cloned_task.rule;
        if let Some(cloned_deps) = cloned_rule.deps {
            let cloned_any_deps: Vec<rule::AnyDep> = match cloned_deps {
                rule::Deps::Rules(rules) => rules.into_iter().map(rule::AnyDep::Rule).collect(),
                rule::Deps::Any(any) => any,
            };
            rule::Deps::push_any_deps(&mut rule.deps, cloned_any_deps);
        }
        if rule.help.is_none() {
            rule.help = cloned_rule.help;
        }
        if rule.inputs.is_none() {
            rule.inputs = cloned_rule.inputs;
        } else {
            return Err(format_error!("Cloned rules cannot specify inputs"));
        }
        if rule.outputs.is_none() {
            rule.outputs = cloned_rule.outputs;
        } else {
            return Err(format_error!("Cloned rules cannot specify outputs"));
        }
        if rule.targets.is_none() {
            rule.targets = cloned_rule.targets;
        } else {
            return Err(format_error!(
                "Cloned rules cannot specify targets (always cloned from the original)"
            ));
        }
        if rule.platforms.is_none() {
            rule.platforms = cloned_rule.platforms;
        } else {
            return Err(format_error!(
                "Cloned rules cannot specify platforms (always cloned from the original)"
            ));
        }
        if rule.type_.is_none() {
            rule.type_ = cloned_rule.type_;
        }

        add_rule_to_all(&rule)
            .context(format_context!("Internal Error: Failed to add rule to all"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Run,
            cloned_task.executor,
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }
}
