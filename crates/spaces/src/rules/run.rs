use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::{environment::GlobalsBuilder, values::none::NoneType};
use std::collections::HashSet;

use crate::{executor, rules};

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn add_target(
        #[starlark(require = named)] rule: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let mut state = rules::get_state().write().unwrap();
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

        let mut state = rules::get_state().write().unwrap();
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

        if let Some(redirect_stdout) = exec_if.then_.redirect_stdout.as_mut() {
            *redirect_stdout = format!(
                "{}/{}",
                rules::get_path_to_build_checkout(rule.name.as_str())?,
                redirect_stdout
            );
        }

        if let Some(exec_else) = exec_if.else_.as_mut() {
            if let Some(redirect_stdout) = exec_else.redirect_stdout.as_mut() {
                *redirect_stdout = format!(
                    "{}/{}",
                    rules::get_path_to_build_checkout(rule.name.as_str())?,
                    redirect_stdout
                );
            }
        }

        let mut state = rules::get_state().write().unwrap();
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
            .context(format_context!("bad options for repo"))?;

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
                .context(format_context!("bad options for repo"))?;

        let mut state = rules::get_state().write().unwrap();
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
