use anyhow_source_location::format_error;
use starlark::{environment::GlobalsBuilder, values::none::NoneType};
use std::collections::HashSet;

use crate::{executor, rules};
use starlark::values::{dict::DictRef, list::ListRef};

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn add_target(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] deps: Option<&ListRef>,
    ) -> anyhow::Result<NoneType> {
        let deps = rules::list_to_vec(deps);
        let mut state = rules::get_state().write().unwrap();

        state.tasks.insert(
            name.to_string(),
            rules::Task::new(
                name,
                rules::Phase::Run,
                deps,
                HashSet::new(),
                HashSet::new(),
                executor::Task::Target,
            ),
        );

        Ok(NoneType)
    }

    fn add_exec(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] command: &str,
        #[starlark(require = named)] redirect_stdout: Option<&str>,
        #[starlark(require = named)] directory: Option<String>,
        #[starlark(require = named)] args: Option<&ListRef>,
        #[starlark(require = named)] deps: Option<&ListRef>,
        #[starlark(require = named)] inputs: Option<&ListRef>,
        #[starlark(require = named)] outputs: Option<&ListRef>,
        #[starlark(require = named)] env: Option<DictRef>,
    ) -> anyhow::Result<NoneType> {
        let deps = rules::list_to_vec(deps);
        let args = rules::list_to_vec(args);
        let inputs = rules::list_to_hashset(inputs);
        let outputs = rules::list_to_hashset(outputs);

        let env = env
            .map(|v| {
                v.iter()
                    .map(|(k, v)| (k.to_str().to_string(), v.to_str().to_string()))
                    .collect()
            })
            .unwrap_or(Vec::new());

        // path to command should be added to inputs

        let mut state = rules::get_state().write().unwrap();
        state.tasks.insert(
            name.to_string(),
            rules::Task::new(
                name,
                rules::Phase::Run,
                deps,
                inputs,
                outputs,
                executor::Task::Exec(executor::exec::Exec::new(
                    command,
                    args,
                    directory,
                    env,
                    redirect_stdout,
                )),
            ),
        );
        Ok(NoneType)
    }

    fn add_archive(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] version: &str,
        #[starlark(require = named)] source_directory: &str,
        #[starlark(require = named)] includes: Option<&ListRef>,
        #[starlark(require = named)] excludes: Option<&ListRef>,
        #[starlark(require = named)] extension: &str,
        #[starlark(require = named)] platform: Option<&str>,
        #[starlark(require = named)] output: &str,
    ) -> anyhow::Result<NoneType> {
        let includes = if includes.is_some() {
            Some(rules::list_to_vec(includes))
        } else {
            None
        };
        let excludes = if excludes.is_some() {
            Some(rules::list_to_vec(excludes))
        } else {
            None
        };

        use crate::executor::archive;
        let archive = archive::Archive {
            input: source_directory.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            driver: match extension {
                "tar.gz" => archive::ArchiveDriver::TarGz,
                "tar.bz2" => archive::ArchiveDriver::TarBz2,
                "tar.7z" => archive::ArchiveDriver::Tar7z,
                "zip" => archive::ArchiveDriver::Zip,
                _ => return Err(format_error!("Invalid archive driver")),
            },
            platform: None,
            includes,
            excludes,
        };

        Ok(NoneType)
    }
}
