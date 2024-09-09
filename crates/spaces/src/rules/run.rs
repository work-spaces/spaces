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

        state.tasks.insert(
            rule.name.to_string(),
            rules::Task::new(rule, rules::Phase::Run, executor::Task::Target),
        );

        Ok(NoneType)
    }

    fn add_exec(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] exec: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let exec: executor::exec::Exec = serde_json::from_value(exec.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let mut state = rules::get_state().write().unwrap();
        state.tasks.insert(
            rule.name.clone(),
            rules::Task::new(rule, rules::Phase::Run, executor::Task::Exec(exec)),
        );
        Ok(NoneType)
    }

    fn add_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] archive: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let _rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let _archive: executor::archive::Archive = serde_json::from_value(archive.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        Ok(NoneType)
    }
}
