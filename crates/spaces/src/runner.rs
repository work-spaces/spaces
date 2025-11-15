use crate::{evaluator, executor, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::Arc;

#[cfg(feature = "lsp")]
use crate::{lsp_context, singleton};
#[cfg(feature = "lsp")]
use itertools::Itertools;

pub use evaluator::IsExecuteTasks;

pub enum IsClearInputs {
    No,
    Yes,
}

impl From<IsClearInputs> for bool {
    fn from(is_clear_inputs: IsClearInputs) -> bool {
        match is_clear_inputs {
            IsClearInputs::No => false,
            IsClearInputs::Yes => true,
        }
    }
}

impl From<bool> for IsClearInputs {
    fn from(is_clear_inputs: bool) -> Self {
        match is_clear_inputs {
            false => IsClearInputs::No,
            true => IsClearInputs::Yes,
        }
    }
}

pub enum IsCreateLockFile {
    No,
    Yes,
}

impl From<IsCreateLockFile> for bool {
    fn from(is_create_lock_file: IsCreateLockFile) -> bool {
        match is_create_lock_file {
            IsCreateLockFile::No => false,
            IsCreateLockFile::Yes => true,
        }
    }
}

impl From<bool> for IsCreateLockFile {
    fn from(is_create_lock_file: bool) -> Self {
        match is_create_lock_file {
            false => IsCreateLockFile::No,
            true => IsCreateLockFile::Yes,
        }
    }
}

pub enum ForEachRepo {
    Repo,
    Branch,
    DirtyBranch,
}

#[derive(Debug, Clone)]
pub enum RunWorkspace {
    Target(Option<Arc<str>>, Vec<Arc<str>>),
    Script(Vec<(Arc<str>, Arc<str>)>),
}

fn get_workspace(
    printer: &mut printer::Printer,
    run_workspace: RunWorkspace,
    absolute_path_to_workspace: Option<Arc<str>>,
    is_clear_inputs: IsClearInputs,
) -> anyhow::Result<workspace::Workspace> {
    let checkout_scripts: Option<Vec<Arc<str>>> = match &run_workspace {
        RunWorkspace::Target(_, _) => None,
        RunWorkspace::Script(scripts) => Some(scripts.iter().map(|e| e.0.clone()).collect()),
    };

    let mut multi_progress = printer::MultiProgress::new(printer);
    let progress = multi_progress.add_progress("workspace", Some(100), Some("Complete"));

    workspace::Workspace::new(
        progress,
        absolute_path_to_workspace,
        is_clear_inputs.into(),
        checkout_scripts,
    )
    .context(format_context!("while running workspace"))
}

fn evaluate_environment(
    printer: &mut printer::Printer,
    workspace_arc: workspace::WorkspaceArc,
) -> anyhow::Result<()> {
    let workspace_modules = workspace_arc.read().modules.clone();
    let modules = workspace_modules.iter().filter_map(|(name, module)| {
        if name.as_ref() == workspace::ENV_FILE_NAME {
            Some((name.clone(), module.clone()))
        } else {
            None
        }
    });

    // evaluate the modules to bring in env.spaces.star
    evaluator::evaluate_starlark_modules(
        printer,
        workspace_arc.clone(),
        modules.collect(),
        task::Phase::Inspect,
    )
    .context(format_context!("while evaluating starlark env module"))?;

    Ok(())
}

pub fn foreach_repo(
    printer: &mut printer::Printer,
    run_workspace: RunWorkspace,
    for_each_repo: ForEachRepo,
    command_arguments: &[Arc<str>],
) -> anyhow::Result<()> {
    let workspace = get_workspace(printer, run_workspace, None, IsClearInputs::No)
        .context(format_context!("while getting workspace"))?;

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));
    evaluate_environment(printer, workspace_arc.clone())
        .context(format_context!("while evaluating starlark env module"))?;

    let mut multi_progress = printer::MultiProgress::new(printer);
    let workspace_members = workspace_arc.read().settings.json.members.clone();

    let mut repos = Vec::new();

    let is_run_on_branches_only = matches!(
        for_each_repo,
        ForEachRepo::Branch | ForEachRepo::DirtyBranch
    );
    let is_run_on_dirty_branches = matches!(for_each_repo, ForEachRepo::DirtyBranch);

    for (url, member_list) in workspace_members.iter() {
        for member in member_list.iter() {
            if is_run_on_branches_only {
                let mut repo_progress = multi_progress.add_progress(
                    format!("//{}", member.path).as_str(),
                    Some(100),
                    Some("Queueing for execution"),
                );
                // use git to check if member is on a branch
                let repo = git::Repository::new(url.clone(), member.path.clone());
                if repo.is_branch(&mut repo_progress, &member.rev) {
                    if repo.is_currently_on_a_branch(&mut repo_progress) {
                        if is_run_on_dirty_branches {
                            // check if the branch is dirty
                            if repo.is_dirty(&mut repo_progress) {
                                repos.push(member.clone());
                            } else {
                                repo_progress.set_ending_message("Skipping: branch is clean");
                            }
                        } else {
                            repos.push(member.clone());
                        }
                    } else {
                        repo_progress.set_ending_message("Skipping: not currently on a branch");
                    }
                } else {
                    repo_progress.set_ending_message("Skipping: rev is not a branch");
                }
            } else {
                // check if member.path is an existing directory
                let path = std::path::Path::new(member.path.as_ref());
                if path.exists() && path.is_dir() {
                    repos.push(member.clone());
                }
            }
        }
    }

    let command_string = command_arguments.join(" ");
    let command_label = command_arguments.join("_");
    for member in repos.iter() {
        let args = Some(command_arguments.to_owned().split_off(1));
        let command = command_arguments[0].clone();
        let working_directory = format!("//{}", member.path).into();

        let mut exec_progress =
            multi_progress.add_progress(command.as_ref(), Some(100), Some("Complete"));

        exec_progress.log(
            printer::Level::App,
            format!("[{working_directory}] {command_string}").as_str(),
        );

        let name = format!("__foreach_{working_directory}_{command_label}");

        let exec = executor::exec::Exec {
            command: command.clone(),
            args,
            env: None,
            working_directory: Some(working_directory),
            redirect_stdout: None,
            expect: None,
            log_level: Some(printer::Level::App),
            timeout: None,
        };

        exec.execute(&mut exec_progress, workspace_arc.clone(), name.as_str())
            .context(format_context!(
                "while executing command {} in workspace",
                command
            ))?;
    }

    Ok(())
}

pub fn run_shell_in_workspace(
    printer: &mut printer::Printer,
    path: Option<Arc<str>>,
) -> anyhow::Result<()> {
    let workspace = get_workspace(
        printer,
        RunWorkspace::Target(None, vec![]),
        None,
        IsClearInputs::No,
    )
    .context(format_context!("while getting workspace"))?;

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));
    evaluate_environment(printer, workspace_arc.clone())
        .context(format_context!("while evaluating starlark env module"))?;

    let shell_config_path = std::path::Path::new(workspace::SHELL_TOML_NAME);
    let shell_config_path_option = if shell_config_path.exists() {
        Some(workspace::SHELL_TOML_NAME.into())
    } else {
        None
    };

    let shell_config = shell::Config::load(shell_config_path_option, path)
        .context(format_context!("while loading shell config"))?;

    let environment_map =
        workspace_arc
            .read()
            .get_env()
            .get_run_vars()
            .context(format_context!(
                "while getting runtime environment for shell"
            ))?;

    const SHELL_DIR: &str = ".spaces/shell";
    std::fs::create_dir_all(SHELL_DIR).context(format_context!(
        "while creating shell directory `{}`",
        SHELL_DIR
    ))?;

    shell::run(
        &shell_config,
        &environment_map,
        std::path::Path::new(SHELL_DIR),
    )
    .context(format_context!("while running shell"))?;

    Ok(())
}

pub fn run_starlark_modules_in_workspace(
    printer: &mut printer::Printer,
    phase: task::Phase,
    absolute_path_to_workspace: Option<Arc<str>>,
    is_clear_inputs: IsClearInputs,
    run_workspace: RunWorkspace,
    is_create_lock_file: IsCreateLockFile,
    is_execute_tasks: IsExecuteTasks,
) -> anyhow::Result<()> {
    let workspace = get_workspace(
        printer,
        run_workspace.clone(),
        absolute_path_to_workspace,
        is_clear_inputs,
    )
    .context(format_context!("while getting workspace"))?;

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));
    match run_workspace {
        RunWorkspace::Target(target, trailing_args) => {
            workspace_arc.write().trailing_args = trailing_args;
            let target = target.map(|e| workspace_arc.read().transform_target_path(e));
            workspace_arc.write().target = target.clone();
            let modules = workspace_arc.read().modules.clone();
            evaluator::run_starlark_modules(
                printer,
                workspace_arc.clone(),
                modules,
                phase,
                target,
                is_execute_tasks,
            )
            .context(format_context!("while executing workspace rules"))?
        }
        RunWorkspace::Script(scripts) => {
            for (name, _) in scripts.iter() {
                logger::Logger::new_printer(printer, name.clone()).message("Digesting");
            }

            workspace_arc.write().is_create_lock_file = is_create_lock_file.into();
            workspace_arc.write().digest = workspace::calculate_digest(&scripts);

            evaluator::run_starlark_modules(
                printer,
                workspace_arc.clone(),
                scripts,
                phase,
                None,
                is_execute_tasks,
            )
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
        workspace::Workspace::new(progress, None, false, None)
            .context(format_context!("while running workspace"))?
    };

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));

    use starlark_lsp::server;
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
    checkout_repo_script: Option<Arc<str>>,
    create_lock_file: IsCreateLockFile,
    keep_workspace_on_failure: bool,
) -> anyhow::Result<()> {
    #[derive(Debug, Clone, PartialEq)]
    enum CheckoutCleanup {
        Workspace,
        WorkspaceContents,
    }

    // Checkout will fail if the target dir exists and is not empty
    let checkout_cleanup = if std::path::Path::new(name.as_ref()).exists() {
        let dir = std::fs::read_dir(name.as_ref())
            .context(format_context!("while reading directory {name}"))?;
        if dir.count() > 0 {
            return Err(format_error!(
                "checkout directory must be non-existent or empty"
            ));
        }

        // on cleanup, delete only the contents of the directory
        CheckoutCleanup::WorkspaceContents
    } else {
        // on cleanup, delete the entire directory since it doesn't exist yet
        CheckoutCleanup::Workspace
    };

    std::fs::create_dir_all(name.as_ref())
        .context(format_context!("while creating workspace directory {name}"))?;

    let mut scripts = Vec::new();

    for one_script in script {
        let script_path = if workspace::is_rules_module(&one_script) {
            one_script.clone()
        } else {
            format!("{one_script}.{}", workspace::SPACES_MODULE_NAME).into()
        };

        let script_as_path = std::path::Path::new(script_path.as_ref());
        let file_name: Arc<str> = script_as_path.file_name().unwrap().to_string_lossy().into();

        let one_script_contents = std::fs::read_to_string(script_path.as_ref())
            .context(format_context!("while reading script file {script_path}"))?;

        std::fs::write(format!("{name}/{file_name}"), one_script_contents.as_str()).context(
            format_context!("while writing script file {script_path} to workspace"),
        )?;

        scripts.push((file_name, one_script_contents.into()));
    }

    if let Some(checkout_repo_script) = checkout_repo_script {
        scripts.push((
            workspace::CHECKOUT_FILE_NAME.into(),
            checkout_repo_script.clone(),
        ));
        std::fs::write(
            format!("{name}/{}", workspace::CHECKOUT_FILE_NAME),
            checkout_repo_script.as_ref(),
        )
        .context(format_context!(
            "while writing script file {} to workspace",
            workspace::CHECKOUT_FILE_NAME
        ))?;
    }

    std::fs::write(format!("{}/{}", name, workspace::ENV_FILE_NAME), "").context(
        format_context!("while creating {} file", workspace::ENV_FILE_NAME),
    )?;

    let current_working_directory = std::env::current_dir()
        .context(format_context!("Failed to get current working directory"))?;

    let target_workspace_directory = current_working_directory.join(name.as_ref());
    let absolute_path_to_workspace: Arc<str> = target_workspace_directory.to_string_lossy().into();

    let checkout_result = run_starlark_modules_in_workspace(
        printer,
        task::Phase::Checkout,
        Some(absolute_path_to_workspace.clone()),
        IsClearInputs::No,
        RunWorkspace::Script(scripts),
        create_lock_file,
        IsExecuteTasks::Yes,
    )
    .context(format_context!(
        "while evaluating starlark modules for checkout"
    ));

    if !keep_workspace_on_failure && checkout_result.is_err() {
        {
            if checkout_cleanup == CheckoutCleanup::WorkspaceContents {
                printer.log(
                    printer::Level::Debug,
                    format!(
                        "Removing contents from existing workspace {absolute_path_to_workspace}",
                    )
                    .as_str(),
                )?;
                let contents = std::fs::read_dir(absolute_path_to_workspace.as_ref()).context(
                    format_context!(
                        "while reading workspace contents for failed workspace {absolute_path_to_workspace}"
                    ),
                )?;

                for entry in contents {
                    let entry = entry.context(format_context!(
                        "while reading workspace entry in failed workspace {absolute_path_to_workspace}"
                    ))?;
                    let path = entry.path();
                    if path.is_file() {
                        std::fs::remove_file(path).context(format_context!(
                            "while removing file in failed workspace {absolute_path_to_workspace}"
                        ))?;
                    } else if path.is_dir() {
                        std::fs::remove_dir_all(path)
                            .context(format_context!("while removing directory in failed workspace {absolute_path_to_workspace}"))?;
                    }
                }
            } else {
                printer.log(
                    printer::Level::Debug,
                    format!("Cleaning up workspace {absolute_path_to_workspace}").as_str(),
                )?;

                std::fs::remove_dir_all(absolute_path_to_workspace.as_ref()).context(
                    format_context!(
                        "while cleaning up failed workspace {absolute_path_to_workspace}"
                    ),
                )?;
            }
        }
    }

    checkout_result
}
