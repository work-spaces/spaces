use crate::{completions, evaluator, executor, label, rules, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::Arc;
use utils::{ci, git, lock, logger, shell, store, version, ws};

use crate::{lsp_context, singleton};
use itertools::Itertools;

pub use evaluator::IsExecuteTasks;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForEachRepo {
    Repo,
    Branch,
    DirtyBranch,
    DevBranch,
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
    is_clear_inputs: workspace::IsClearInputs,
    is_checkout_phase: workspace::IsCheckoutPhase,
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
        is_clear_inputs,
        checkout_scripts,
        is_checkout_phase,
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
    let workspace = get_workspace(
        printer,
        run_workspace,
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
    )
    .context(format_context!("while getting workspace"))?;

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));
    evaluate_environment(printer, workspace_arc.clone())
        .context(format_context!("while evaluating starlark env module"))?;

    let mut multi_progress = printer::MultiProgress::new(printer);
    let workspace_members = workspace_arc.read().settings.json.members.clone();
    let dev_branch_rules = workspace_arc.read().settings.json.dev_branches.clone();

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
            } else if for_each_repo == ForEachRepo::DevBranch {
                // get the dev-branches from the workspace settings
                for dev_branch_rule in dev_branch_rules.iter() {
                    let location_in_workspace =
                        label::get_rule_name_from_label(dev_branch_rule.as_ref());
                    if location_in_workspace == member.path.as_ref() {
                        repos.push(member.clone());
                        // skip the rest of the loop
                        break;
                    }
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
        let mut args = command_arguments.to_owned().split_off(1);
        let command = command_arguments[0].clone();
        if command.as_ref() == "git" {
            args.insert(0, "-c".into());
            args.insert(1, "color.ui=always".into());
        }
        let working_directory: Arc<str> = format!("//{}", member.path).into();

        let mut exec_progress =
            multi_progress.add_progress(command.as_ref(), Some(100), Some("Complete"));

        exec_progress.log(
            printer::Level::Passthrough,
            format!(">>> {working_directory} $ {command_string}").as_str(),
        );

        let name = format!("__foreach_{working_directory}_{command_label}");

        let exec = executor::exec::Exec {
            command: command.clone(),
            args: Some(args),
            env: None,
            working_directory: Some(working_directory.clone()),
            redirect_stdout: None,
            expect: None,
            log_level: Some(printer::Level::Passthrough),
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
    completions_command: Option<(clap::Command, rules::HasHelp)>,
) -> anyhow::Result<()> {
    let workspace = get_workspace(
        printer,
        RunWorkspace::Target(None, vec![]),
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
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

    let run_environment = workspace_arc
        .read()
        .get_env()
        .get_run_environment()
        .context(format_context!("while getting run environment"))?;

    const SHELL_DIR: &str = ".spaces/shell";
    std::fs::create_dir_all(SHELL_DIR).context(format_context!(
        "while creating shell directory `{}`",
        SHELL_DIR
    ))?;

    let completion_content = if let Some((command, has_help)) = completions_command {
        // rules are now available
        let clap_shell = shell_config
            .get_shell()
            .context(format_context!("Shell does not support completions"))?;

        let run_targets = run_starlark_get_targets(printer, has_help)
            .context(format_context!("Failed to get targets"))?;

        completions::generate_workspace_completions(&command, clap_shell, run_targets)
            .context(format_context!("Failed to generate workspace completions"))?
    } else {
        Vec::new()
    };

    let relative_directory = workspace_arc.read().relative_invoked_path.clone();
    let working_directory =
        std::path::Path::new(workspace_arc.read().absolute_path.clone().as_ref())
            .join(relative_directory.as_ref());

    shell::run(
        &shell_config,
        &run_environment.vars,
        std::path::Path::new(SHELL_DIR),
        completion_content,
        &working_directory,
    )
    .context(format_context!("while running shell"))?;

    Ok(())
}

pub fn run_store_command_in_workspace(
    printer: &mut printer::Printer,
    store_command: store::StoreCommand,
) -> anyhow::Result<()> {
    let workspace_result = get_workspace(
        printer,
        RunWorkspace::Target(None, vec![]),
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
    );
    let store_path_str = match workspace_result {
        Ok(workspace) => workspace.get_store_path(),
        Err(_) => ws::get_checkout_store_path(),
    };
    let store_path = std::path::Path::new(store_path_str.as_ref());
    let mut store = store::Store::new_from_store_path(store_path).context(format_context!(
        "Failed to load store at {}",
        store_path_str
    ))?;
    let is_ci: ci::IsCi = singleton::get_is_ci().into();

    match store_command {
        store::StoreCommand::Info { sort_by } => {
            store
                .show_info(printer, sort_by, is_ci)
                .context(format_context!("While getting store info"))?;
        }
        store::StoreCommand::Fix { dry_run } => {
            store
                .fix(printer, dry_run, is_ci)
                .context(format_context!("While fixing store"))?;

            store
                .save(store_path)
                .context(format_context!("Failed to save store at {store_path_str}",))?;
        }
        store::StoreCommand::Prune { age, dry_run } => {
            store
                .prune(printer, age, dry_run, is_ci)
                .context(format_context!("While pruning store"))?;

            store
                .save(store_path)
                .context(format_context!("Failed to save store at {store_path_str}",))?;
        }
    }

    Ok(())
}

pub fn run_version_command_in_workspace(
    printer: &mut printer::Printer,
    command: version::Command,
) -> anyhow::Result<()> {
    let workspace_result = get_workspace(
        printer,
        RunWorkspace::Target(None, vec![]),
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
    );
    let store_path_str = match workspace_result {
        Ok(workspace) => workspace.get_store_path(),
        Err(_) => ws::get_checkout_store_path(),
    };
    let store_path = std::path::Path::new(store_path_str.as_ref());
    let version_manager = version::Manager::new(store_path);

    match command {
        version::Command::List {} => {
            version_manager
                .list(printer)
                .context(format_context!("Failed to list versions"))?;
        }
        version::Command::Fetch { tag } => {
            version_manager
                .fetch(printer, tag.clone())
                .context(format_context!(
                    "Failed to fetch {}",
                    tag.as_ref().map(|e| e.as_ref()).unwrap_or("latest")
                ))?;
        }
    }

    Ok(())
}

pub fn run_starlark_get_targets(
    printer: &mut printer::Printer,
    has_help: rules::HasHelp,
) -> anyhow::Result<Vec<Arc<str>>> {
    run_starlark_modules_in_workspace(
        printer,
        task::Phase::Inspect,
        None,
        workspace::IsClearInputs::No,
        RunWorkspace::Target(None, vec![]),
        IsCreateLockFile::No,
        IsExecuteTasks::No,
    )
    .context(format_context!("while executing run rules"))?;

    rules::get_run_targets(has_help).context(format_context!("Failed to get run targets"))
}

pub fn run_starlark_modules_in_workspace(
    printer: &mut printer::Printer,
    phase: task::Phase,
    absolute_path_to_workspace: Option<Arc<str>>,
    is_clear_inputs: workspace::IsClearInputs,
    run_workspace: RunWorkspace,
    is_create_lock_file: IsCreateLockFile,
    is_execute_tasks: IsExecuteTasks,
) -> anyhow::Result<()> {
    let is_checkout_phase = if phase == task::Phase::Checkout {
        workspace::IsCheckoutPhase::Yes
    } else {
        workspace::IsCheckoutPhase::No
    };
    let workspace = get_workspace(
        printer,
        run_workspace.clone(),
        absolute_path_to_workspace,
        is_clear_inputs,
        is_checkout_phase,
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
                logger::Logger::new_printer(printer, name.clone()).debug("Digesting");
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

pub fn run_lsp(printer: &mut printer::Printer) -> anyhow::Result<()> {
    let workspace = {
        let mut multi_progress = printer::MultiProgress::new(printer);
        let progress = multi_progress.add_progress("workspace", Some(100), Some("Complete"));
        workspace::Workspace::new(
            progress,
            None,
            workspace::IsClearInputs::No,
            None,
            workspace::IsCheckoutPhase::No,
        )
        .context(format_context!("while running workspace"))?
    };

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));

    use starlark_lsp::server;
    eprintln!("Starting Spaces Starlark server-");

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

    let lsp_context = lsp_context::SpacesContext::new(workspace_arc, lsp_context::ContextMode::Run)
        .context(format_context!(
            "Internal Error: Failed to create spaces lsp context"
        ))?;

    // Note that  we must have our logging only write out to stderr.

    server::stdio_server(lsp_context).context(format_context!("spaces LSP server exited"))?;
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

    // ENV file is empty at the beginning of checkout
    let env_path = std::path::Path::new(name.as_ref()).join(workspace::ENV_FILE_NAME);
    std::fs::write(env_path, "").context(format_context!(
        "while creating {} file",
        workspace::ENV_FILE_NAME
    ))?;

    let current_working_directory = std::env::current_dir()
        .context(format_context!("Failed to get current working directory"))?;

    let target_workspace_directory = current_working_directory.join(name.as_ref());
    let absolute_path_to_workspace: Arc<str> = target_workspace_directory.to_string_lossy().into();

    let checkout_result = run_starlark_modules_in_workspace(
        printer,
        task::Phase::Checkout,
        Some(absolute_path_to_workspace.clone()),
        workspace::IsClearInputs::No,
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
