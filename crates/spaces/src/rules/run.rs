use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::{environment::GlobalsBuilder, values::none::NoneType};

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
        state.insert_task(
            rules::Task::new(rule, rules::Phase::Run, executor::Task::Target),
        ).context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    fn add_exec(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] exec: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let exec: executor::exec::Exec = serde_json::from_value(exec.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        let mut state = rules::get_state().write().unwrap();
        let rule_name = rule.name.clone();
        state.insert_task(
            rules::Task::new(rule, rules::Phase::Run, executor::Task::Exec(exec)),
        ).context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] archive: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let create_archive: easy_archiver::CreateArchive = serde_json::from_value(archive.to_json_value()?)
        .context(format_context!("bad options for repo"))?;

        let archive = executor::archive::Archive {
            create_archive,
        };

        let mut state = rules::get_state().write().unwrap();
        let rule_name = rule.name.clone();
        state.insert_task(
            rules::Task::new(rule, rules::Phase::Run, executor::Task::CreateArchive(archive))
        ).context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }
}
