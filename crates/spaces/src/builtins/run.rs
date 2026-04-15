use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use starlark::eval::Evaluator;
use starlark::{environment::GlobalsBuilder, values::none::NoneType};

use std::sync::Arc;
use utils::{logger, marker, rule, targets};

use crate::builtins::eval_context::get_eval_context;
use crate::{executor, rules, task};

fn add_file_tokens_to_deps(
    exec: &executor::exec::Exec,
    rule: &mut rule::Rule,
) -> anyhow::Result<()> {
    let mut file_paths: Vec<String> = Vec::new();
    for arg in exec.args.iter().flatten() {
        file_paths.extend(marker::extract_marker_values(
            arg,
            rule::FILE_CONTENT_MARKER,
        ));
    }
    for value in exec.env.iter().flat_map(|e| e.values()) {
        file_paths.extend(marker::extract_marker_values(
            value,
            rule::FILE_CONTENT_MARKER,
        ));
    }

    if file_paths.is_empty() {
        return Ok(());
    }

    let normalize = |path: &str| -> String {
        if path.starts_with("//") {
            path.to_string()
        } else {
            match exec.working_directory.as_ref() {
                Some(wd) => format!("{wd}/{path}"),
                None => format!("//{path}"),
            }
        }
    };

    for raw_path in &file_paths {
        let normalized = normalize(raw_path);
        let already_covered = rule
            .deps
            .iter()
            .flat_map(|deps| deps.collect_globs())
            .filter_map(|g| match g {
                rule::Globs::Includes(v) => Some(v),
                _ => None,
            })
            .flatten()
            .any(|p| p.as_ref() == normalized.as_str());

        if !already_covered {
            rule::Deps::push_any_dep(
                &mut rule.deps,
                rule::AnyDep::Glob(rule::Globs::Includes(vec![normalized.into()])),
            );
        }
    }
    Ok(())
}

fn sanitize_exit_value_tokens(
    exec: &mut executor::exec::Exec,
    module_name: &Arc<str>,
) -> anyhow::Result<()> {
    let marker_open = format!("{}{{", rule::EXIT_VALUE_MARKER);
    let sanitize_str = |s: &Arc<str>| -> anyhow::Result<Arc<str>> {
        let mut result = s.to_string();
        for raw_name in marker::extract_marker_values(s.as_ref(), rule::EXIT_VALUE_MARKER) {
            if !raw_name.starts_with("//") && !raw_name.starts_with(':') {
                return Err(format_error!(
                    "$RUN_LOAD_EXIT_VALUE{{{raw_name}}} must start with '//' or ':'"
                ));
            }
            let expanded =
                rules::get_sanitized_rule_name_for_module(raw_name.as_str().into(), module_name);
            let old_token = format!("{marker_open}{raw_name}}}");
            let new_token = format!("{marker_open}{expanded}}}");
            result = result.replace(&old_token, &new_token);
        }
        Ok(result.into())
    };

    for arg in exec.args.iter_mut().flatten() {
        *arg = sanitize_str(arg).context(format_context!("invalid $RUN_LOAD_EXIT_VALUE in arg"))?;
    }
    for value in exec.env.iter_mut().flat_map(|e| e.values_mut()) {
        *value =
            sanitize_str(value).context(format_context!("invalid $RUN_LOAD_EXIT_VALUE in env"))?;
    }
    Ok(())
}

fn add_exit_value_rule_deps(
    exec: &executor::exec::Exec,
    rule: &mut rule::Rule,
    module_name: &Arc<str>,
) -> anyhow::Result<()> {
    let mut rule_names: Vec<String> = Vec::new();
    for arg in exec.args.iter().flatten() {
        rule_names.extend(marker::extract_marker_values(arg, rule::EXIT_VALUE_MARKER));
    }
    for value in exec.env.iter().flat_map(|e| e.values()) {
        rule_names.extend(marker::extract_marker_values(
            value,
            rule::EXIT_VALUE_MARKER,
        ));
    }

    for raw_name in &rule_names {
        let sanitized: Arc<str> =
            rules::get_sanitized_rule_name_for_module(raw_name.as_str().into(), module_name);
        let already_dep = rule
            .deps
            .iter()
            .flat_map(|deps| deps.collect_rules())
            .any(|r| r.as_ref() == sanitized.as_ref());
        if !already_dep {
            rule::Deps::push_any_dep(&mut rule.deps, rule::AnyDep::Rule(sanitized));
        }
    }
    Ok(())
}

fn add_rule_to_all(
    rule: &rule::Rule,
    workspace_arc: &crate::workspace::WorkspaceArc,
    module_name: &Arc<str>,
) -> anyhow::Result<()> {
    if let Some(rule::RuleType::Run) = rule.type_.as_ref() {
        let mut workspace = workspace_arc.write();
        workspace
            .settings
            .bin
            .run_all
            .insert(rules::get_sanitized_rule_name_for_module(
                rule.name.clone(),
                module_name,
            ));
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
    fn abort(message: &str, eval: &mut Evaluator) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        if ctx.is_lsp {
            Ok(NoneType)
        } else {
            Err(format_error!("Run Aborting: {}", message))
        }
    }

    /// Adds a rule that depends on other rules but doesn't execute any command.
    ///
    /// There is no specific action for the rule, but this rule can be useful for organizing dependencies.
    ///
    /// ```python
    /// run.add(
    ///     rule = {
    ///         "name": "my_rule",
    ///         "deps": ["my_other_rule"],
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: Rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    fn add(
        #[starlark(require = named)] rule: starlark::values::Value,
        eval: &mut Evaluator,
    ) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add target rule"))?;

        if let Some(workspace_arc) = ctx.workspace.clone() {
            add_rule_to_all(&rule, &workspace_arc, &ctx.module_name)
                .context(format_context!("Internal Error: Failed to add rule to all"))?;
        }

        let rule_name = rule.name.clone();
        rules::insert_task_for_module(
            task::Task::new(rule, task::Phase::Run, executor::Task::Target),
            &ctx.module_name,
            ctx.default_module_visibility.clone(),
        )
        .context(format_context!("Failed to register rule {rule_name}"))?;
        Ok(NoneType)
    }

    /// Adds a rule that depends on other rules.
    ///
    /// This rule will be deprecated in favor of `run.add`.
    ///
    /// # Arguments
    /// * `rule`: Rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    fn add_target(
        #[starlark(require = named)] rule: starlark::values::Value,
        eval: &mut Evaluator,
    ) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        logger::push_deprecation_warning(
            Some(ctx.module_name.clone()),
            "Support for checkout.add_which_asset() will be removed in v0.16. Use checkout.add_any_asset().",
        );

        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add target rule"))?;

        if let Some(workspace_arc) = ctx.workspace.clone() {
            add_rule_to_all(&rule, &workspace_arc, &ctx.module_name)
                .context(format_context!("Internal Error: Failed to add rule to all"))?;
        }

        let rule_name = rule.name.clone();
        rules::insert_task_for_module(
            task::Task::new(rule, task::Phase::Run, executor::Task::Target),
            &ctx.module_name,
            ctx.default_module_visibility.clone(),
        )
        .context(format_context!("Failed to register rule {rule_name}"))?;
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
        eval: &mut Evaluator,
    ) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        let mut rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
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

        if let Some(workspace_arc) = ctx.workspace.clone() {
            add_rule_to_all(&rule, &workspace_arc, &ctx.module_name)
                .context(format_context!("Internal Error: Failed to add rule to all"))?;
        }

        let mut exec: executor::exec::Exec = serde_json::from_value(exec.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        if let Some(working_directory) = exec.working_directory.as_mut() {
            *working_directory = rules::get_sanitized_working_directory_for_module(
                working_directory.clone(),
                &ctx.module_name,
            );
        }

        if let Some(redirect_stdout) = exec.redirect_stdout.as_mut() {
            *redirect_stdout = format!("build/{redirect_stdout}").into();
        }

        sanitize_exit_value_tokens(&mut exec, &ctx.module_name).context(format_context!(
            "Failed to sanitize $RUN_LOAD_EXIT_VALUE tokens for rule {}",
            rule.name
        ))?;
        add_file_tokens_to_deps(&exec, &mut rule).context(format_context!(
            "Failed to add $FILE tokens to deps for rule {}",
            rule.name
        ))?;
        add_exit_value_rule_deps(&exec, &mut rule, &ctx.module_name).context(format_context!(
            "Failed to add $RUN_LOAD_EXIT_VALUE rule deps for rule {}",
            rule.name
        ))?;

        let rule_name = rule.name.clone();
        rules::insert_task_for_module(
            task::Task::new(rule, task::Phase::Run, executor::Task::Exec(exec)),
            &ctx.module_name,
            ctx.default_module_visibility.clone(),
        )
        .context(format_context!("Failed to register rule {rule_name}"))?;
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
        eval: &mut Evaluator,
    ) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
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

        if let Some(workspace_arc) = ctx.workspace.clone() {
            add_rule_to_all(&rule, &workspace_arc, &ctx.module_name)
                .context(format_context!("Internal Error: Failed to add rule to all"))?;
        }

        let mut kill_exec: executor::exec::Kill = serde_json::from_value(kill.to_json_value()?)
            .context(format_context!("bad options for kill"))?;
        kill_exec.target =
            rules::get_sanitized_rule_name_for_module(kill_exec.target.clone(), &ctx.module_name);

        let rule_name = rule.name.clone();
        rules::insert_task_for_module(
            task::Task::new(rule, task::Phase::Run, executor::Task::Kill(kill_exec)),
            &ctx.module_name,
            ctx.default_module_visibility.clone(),
        )
        .context(format_context!("Failed to register rule {rule_name}"))?;
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
        eval: &mut Evaluator,
    ) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
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
            "//build/{}/{}",
            rules::get_sanitized_rule_name_for_module(rule_name.clone(), &ctx.module_name),
            create_archive.get_output_file()
        )
        .into();
        rule.push_target(targets::Target::File(target_path));

        let archive = executor::archive::Archive { create_archive };

        rules::insert_task_for_module(
            task::Task::new(
                rule,
                task::Phase::Run,
                executor::Task::CreateArchive(archive),
            ),
            &ctx.module_name,
            ctx.default_module_visibility.clone(),
        )
        .context(format_context!("Failed to register rule {rule_name}"))?;
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
    /// * `rule`: Rule definition containing `name` (`str`), `deps` (`list`), `type` (`str`), and `help` (`str`).
    /// * `clone_from`: The name of an existing rule whose exec will be cloned.
    fn add_from_clone(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] clone_from: &str,
        eval: &mut Evaluator,
    ) -> anyhow::Result<NoneType> {
        let ctx = get_eval_context(eval)?;
        let mut rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add_from_clone rule"))?;

        if ctx.is_lsp {
            return Ok(NoneType);
        }

        let cloned_task = rules::get_cloned_task_for_module(clone_from, &ctx.module_name)
            .context(format_context!("Failed to clone rule {}", clone_from))?;

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

        if let Some(workspace_arc) = ctx.workspace.clone() {
            add_rule_to_all(&rule, &workspace_arc, &ctx.module_name)
                .context(format_context!("Internal Error: Failed to add rule to all"))?;
        }

        let rule_name = rule.name.clone();
        rules::insert_task_for_module(
            task::Task::new(rule, task::Phase::Run, cloned_task.executor),
            &ctx.module_name,
            ctx.default_module_visibility.clone(),
        )
        .context(format_context!("Failed to register rule {rule_name}"))?;
        Ok(NoneType)
    }
}
