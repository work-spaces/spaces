use crate::{evaluator, rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use std::sync::Arc;

#[cfg(feature = "lsp")]
use crate::{lsp_context, singleton};
#[cfg(feature = "lsp")]
use itertools::Itertools;

pub enum RunWorkspace {
    Target(Option<Arc<str>>, Vec<Arc<str>>),
    Script(Vec<(Arc<str>, Arc<str>)>),
}

pub fn run_starlark_modules_in_workspace(
    printer: &mut printer::Printer,
    phase: rules::Phase,
    absolute_path_to_workspace: Option<Arc<str>>,
    is_clear_inputs: bool,
    run_workspace: RunWorkspace,
    is_create_lock_file: bool,
) -> anyhow::Result<()> {
    let workspace = {
        let mut multi_progress = printer::MultiProgress::new(printer);
        let progress = multi_progress.add_progress("workspace", Some(100), Some("Complete"));
        workspace::Workspace::new(progress, absolute_path_to_workspace, is_clear_inputs)
            .context(format_context!("while running workspace"))?
    };

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));
    match run_workspace {
        RunWorkspace::Target(target, trailing_args) => {
            workspace_arc.write().trailing_args = trailing_args;
            workspace_arc.write().target = target.clone();
            let modules = workspace_arc.read().modules.clone();
            evaluator::run_starlark_modules(printer, workspace_arc.clone(), modules, phase, target)
                .context(format_context!("while executing workspace rules"))?
        }
        RunWorkspace::Script(scripts) => {
            for (name, _) in scripts.iter() {
                logger::Logger::new_printer(printer, name.clone()).message("Digesting");
            }

            workspace_arc.write().is_create_lock_file = is_create_lock_file;
            workspace_arc.write().digest = workspace::calculate_digest(&scripts);

            evaluator::run_starlark_modules(printer, workspace_arc.clone(), scripts, phase, None)
                .context(format_context!("while evaulating starlark modules"))?;

            workspace_arc
                .read()
                .save_lock_file()
                .context(format_context!("Failed to save workspace lock file"))?;
        }
    }

    workspace::RuleMetricsFile::update(workspace_arc.clone())
        .context(format_context!("Failed to update rule metrics file"))?;

    Ok(())
}

#[cfg(feature = "lsp")]
pub fn run_lsp(printer: &mut printer::Printer) -> anyhow::Result<()> {
    let workspace = {
        let mut multi_progress = printer::MultiProgress::new(printer);
        let progress = multi_progress.add_progress("workspace", Some(100), Some("Complete"));
        workspace::Workspace::new(progress, None)
            .context(format_context!("while running workspace"))?
    };

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));

    use starlark_lsp::server;
    let dialect = evaluator::get_dialect();
    let globals = evaluator::get_globals(evaluator::WithRules::Yes).build();
    eprintln!("Starting Spaces Starlark server");

    singleton::set_active_workspace(workspace_arc.clone());

    // collect .star files in workspace
    let workspace_path = workspace_arc.read().absolute_path.to_owned();
    let mut modules = Vec::new();
    let walkdir = walkdir::WalkDir::new(workspace_path.as_ref());
    for entry in walkdir {
        let entry = entry.context(format_context!("Failed to walk directory"))?;
        if entry.file_type().is_file() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "star"
                    && !path
                        .components()
                        .contains(&std::path::Component::Normal("script".as_ref()))
                {
                    modules.push(path.to_path_buf());
                }
            }
        }
    }

    let lsp_context = lsp_context::SpacesContext::new(
        workspace_arc.read().get_absolute_path(),
        lsp_context::ContextMode::Run,
        true,
        &[],
        true,
        dialect,
        globals,
    )
    .context(format_context!(
        "Internal Error: Failed to create spaces lsp context"
    ))?;

    // Note that  we must have our logging only write out to stderr.

    let (connection, io_threads) = lsp_server::Connection::stdio();
    server::server_with_connection(connection, lsp_context)
        .context(format_context!("spaces LSP server exited"))?;
    // Make sure that the io threads stop properly too.
    io_threads
        .join()
        .context(format_context!("Failed to join io threads"))?;

    eprintln!("Stopping Spaces Starlark server");

    Ok(())
}

pub fn checkout(
    printer: &mut printer::Printer,
    name: Arc<str>,
    script: Vec<Arc<str>>,
    create_lock_file: bool,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(name.as_ref())
        .context(format_context!("while creating workspace directory {name}"))?;

    let mut settings = workspace::Settings::default();
    let mut scripts = Vec::new();

    for one_script in script {
        let script_path = if workspace::is_rules_module(&one_script) {
            one_script.clone()
        } else {
            format!("{one_script}.{}", workspace::SPACES_MODULE_NAME).into()
        };

        let script_as_path = std::path::Path::new(script_path.as_ref());
        let file_name: Arc<str> = script_as_path.file_name().unwrap().to_string_lossy().into();
        settings.push(file_name.clone());

        let one_script_contents = std::fs::read_to_string(script_path.as_ref())
            .context(format_context!("while reading script file {script_path}"))?;

        std::fs::write(format!("{name}/{file_name}"), one_script_contents.as_str()).context(
            format_context!("while writing script file {script_path} to workspace"),
        )?;

        scripts.push((file_name, one_script_contents.into()));
    }

    settings.store_path = workspace::get_checkout_store_path();

    std::fs::write(format!("{}/{}", name, workspace::ENV_FILE_NAME), "").context(
        format_context!("while creating {} file", workspace::ENV_FILE_NAME),
    )?;

    let current_working_directory = std::env::current_dir()
        .context(format_context!("Failed to get current working directory"))?;

    let target_workspace_directory = current_working_directory.join(name.as_ref());
    let absolute_path_to_workspace: Arc<str> = target_workspace_directory.to_string_lossy().into();

    run_starlark_modules_in_workspace(
        printer,
        rules::Phase::Checkout,
        Some(absolute_path_to_workspace.clone()),
        false,
        RunWorkspace::Script(scripts),
        create_lock_file,
    )
    .context(format_context!(
        "while evaulating starklark modules for checkout"
    ))?;

    settings
        .save(absolute_path_to_workspace.as_ref())
        .context(format_context!("while saving settings"))?;

    Ok(())
}
