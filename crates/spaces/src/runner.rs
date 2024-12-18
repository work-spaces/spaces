use crate::{evaluator, rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;

pub enum RunWorkspace {
    Target(Option<String>),
    Script(Vec<(String, String)>),
}

pub fn run_starlark_modules_in_workspace(
    printer: &mut printer::Printer,
    phase: rules::Phase,
    run_workspace: RunWorkspace,
) -> anyhow::Result<()> {
    let workspace = {
        let mut multi_progress = printer::MultiProgress::new(printer);
        let progress =
            multi_progress.add_progress("loading workspace", Some(100), Some("Complete"));
        workspace::Workspace::new(progress).context(format_context!("while running workspace"))?
    };

    match run_workspace {
        RunWorkspace::Target(target) => {
            evaluator::run_starlark_modules(printer, workspace.modules, phase, target)
                .context(format_context!("while executing workspace rules"))?
        }
        RunWorkspace::Script(scripts) => {
            for (name, _) in scripts.iter() {
                printer.log(
                    printer::Level::Message,
                    format!("Digesting {}", name).as_str(),
                )?;
            }
            workspace::set_digest(workspace::calculate_digest(&scripts));
            evaluator::run_starlark_modules(printer, scripts, phase, None)
                .context(format_context!("while executing checkout rules"))?
        }
    }
    Ok(())
}

pub fn checkout(printer: &mut printer::Printer, name: String, script: Vec<String>, create_lock_file: bool) -> anyhow::Result<()> {
    std::fs::create_dir_all(name.as_str())
        .context(format_context!("while creating workspace directory {name}"))?;

    let mut settings = workspace::Settings::default();
    let mut scripts = Vec::new();

    for one_script in script {
        let script_path = if workspace::is_rules_module(&one_script) {
            one_script.clone()
        } else {
            format!("{one_script}.{}", workspace::SPACES_MODULE_NAME)
        };

        let script_as_path = std::path::Path::new(script_path.as_str());
        let file_name = script_as_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        settings.push(file_name.as_str());

        let one_script_contents = std::fs::read_to_string(script_path.as_str())
            .context(format_context!("while reading script file {script_path}"))?;

        std::fs::write(
            format!("{}/{}", name, file_name),
            one_script_contents.as_str(),
        )
        .context(format_context!(
            "while writing script file {script_path} to workspace"
        ))?;

        scripts.push((file_name, one_script_contents));
    }

    settings.store_path = workspace::get_checkout_store_path();

    std::fs::write(format!("{}/{}", name, workspace::ENV_FILE_NAME), "").context(
        format_context!("while creating {} file", workspace::ENV_FILE_NAME),
    )?;

    let current_working_directory = std::env::current_dir()
        .context(format_context!("Failed to get current working directory"))?;

    let target_workspace_directory = current_working_directory.join(name.as_str());

    std::env::set_current_dir(target_workspace_directory.clone()).context(format_context!(
        "Failed to set current directory to {:?}",
        target_workspace_directory
    ))?;

    workspace::set_create_lock_file(create_lock_file);

    run_starlark_modules_in_workspace(
        printer,
        rules::Phase::Checkout,
        RunWorkspace::Script(scripts),
    )
    .context(format_context!("while executing checkout rules"))?;

    settings
        .save(&workspace::absolute_path())
        .context(format_context!("while saving settings"))?;
    workspace::save_lock_file().context(format_context!("Failed to save workspace lock file"))?;

    Ok(())
}
