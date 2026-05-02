use crate::{co, completions, docs, evaluator, rules, runner, singleton, task, tools, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum, ValueHint};
use std::{io::IsTerminal, sync::Arc};
use utils::{ci, features, git, logs, shell, store, version};

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Level {
    Trace,
    Debug,
    Message,
    Info,
    App,
    Passthrough,
    Warning,
    Error,
}

impl From<Level> for console::Level {
    fn from(level: Level) -> Self {
        match level {
            Level::Trace => console::Level::Trace,
            Level::Debug => console::Level::Debug,
            Level::Message => console::Level::Message,
            Level::Info => console::Level::Info,
            Level::App => console::Level::App,
            Level::Passthrough => console::Level::Passthrough,
            Level::Warning => console::Level::Warning,
            Level::Error => console::Level::Error,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    #[arg(short, long, default_value = "app")]
    pub verbosity: Level,
    /// Dont show progress bars
    #[arg(long)]
    pub hide_progress_bars: bool,
    /// Show elapsed time - use with --verbosity=debug to instrument spaces performance
    #[arg(long)]
    pub show_elapsed_time: bool,
    /// If this is passed, info.is_ci() returns true in scripts.
    #[arg(long)]
    ci: bool,
    /// Disables creating log files
    #[arg(long)]
    disable_logs: bool,
    /// Rescan the workspace for *spaces.star files
    #[arg(long)]
    rescan: bool,
    #[command(subcommand)]
    commands: Commands,
}

fn handle_verbosity(
    console: console::Console,
    level: console::Level,
    is_ci: bool,
    disable_logs: bool,
    rescan: bool,
    is_hide_progress_bars: bool,
    show_elapsed_time: bool,
) {
    singleton::set_rescan(rescan);
    singleton::set_logging_disabled(disable_logs);
    console.set_level(level);
    if is_ci {
        singleton::set_ci(true);
        if level == console::Level::App {
            console.set_level(console::Level::Message);
        }
        console.set_is_show_progress_bars(false);
        console.set_is_show_elapsed_time(true);
        console.set_is_tty(false);
    } else {
        console.set_is_show_progress_bars(!is_hide_progress_bars);
        console.set_is_show_elapsed_time(show_elapsed_time);
    }
}

pub fn execute() -> anyhow::Result<()> {
    if std::env::args().len() == 1 {
        if !std::io::stdin().is_terminal() {
            let mut stdin_contents = String::new();
            use std::io::Read;
            std::io::stdin().read_to_string(&mut stdin_contents)?;
            evaluator::run_starlark_script(
                workspace::SPACES_STDIN_NAME.into(),
                stdin_contents.into(),
            )
            .context(format_context!("Failed to run starlark script"))?;
            return Ok(());
        }

        return Err(format_error!(
            "Use `spaces help` for details or pipe a script to standard input"
        ));
    }

    if std::env::args().len() >= 2 {
        let filename: Arc<str> = std::env::args().nth(1).unwrap().into();
        let input = std::path::Path::new(filename.as_ref());
        if input.exists() && input.extension().unwrap_or_default() == "star" {
            starstd::script::set_args(std::env::args().skip(1).collect());

            let input_contents = std::fs::read_to_string(input)
                .context(format_context!("Failed to read input file {input:?}"))?;
            evaluator::run_starlark_script(filename.clone(), input_contents.into())
                .context(format_context!("Failed to run starlark script {filename}"))?;

            let exit_code = starstd::script::get_exit_code();
            if exit_code != 0 {
                std::process::exit(exit_code);
            }

            return Ok(());
        }
    }

    let args = Arguments::parse();
    let Arguments {
        verbosity,
        hide_progress_bars,
        show_elapsed_time,
        ci,
        disable_logs,
        rescan,
        commands,
    } = args;

    let hide_progress_bars = if matches!(commands, Commands::Foreach { .. }) {
        true
    } else {
        hide_progress_bars
    };

    let stdout_console = console::Console::new_stdout(console::Verbosity {
        level: verbosity.into(),
        ..Default::default()
    })?;
    let null_console = console::Console::new_null();

    let effective_console = if matches!(commands, Commands::RunLsp { .. }) {
        &null_console
    } else {
        // terminate immediately if ctrl+c is received twice
        use signal_hook::consts::SIGINT;
        let term_now = Arc::new(std::sync::atomic::AtomicBool::new(false));
        signal_hook::flag::register_conditional_shutdown(SIGINT, 1, Arc::clone(&term_now))?;
        signal_hook::flag::register(SIGINT, Arc::clone(&term_now))?;

        stdout_console.set_is_tty(std::io::stdout().is_terminal());

        &stdout_console
    };

    handle_verbosity(
        effective_console.clone(),
        verbosity.into(),
        ci,
        disable_logs,
        rescan,
        hide_progress_bars,
        show_elapsed_time,
    );

    let refresh_handle = effective_console.start_refresh_thread();
    let result = execute_command(commands, effective_console.clone());
    {
        let deferred_warnings = utils::logger::get_deferred_warnings();
        if !deferred_warnings.is_empty() {
            for warning in deferred_warnings {
                let _ = effective_console.log(console::Level::Warning, warning.as_ref());
            }
        }
    }

    let result = if let Err(error) = result {
        if let Some(logs) = singleton::get_logs_for_failed_rules() {
            let _ = effective_console.error("see also", format!("\n  {}", logs.join("\n  ")));
        } else {
            singleton::process_anyhow_error(error);
            singleton::show_error_chain(effective_console.clone());
        }
        Err(anyhow::anyhow!("execution failed"))
    } else {
        Ok(())
    };

    effective_console.shutdown_refresh_thread();
    let _ = refresh_handle.join();
    result
}

fn execute_command(command: Commands, effective_console: console::Console) -> anyhow::Result<()> {
    match command {
        Commands::Checkout {
            name,
            env,
            store,
            new_branch,
            script,
            workflow,
            wf,
            create_lock_file,
            force_install_tools,
            keep_workspace_on_failure,
            lock,
        } => {
            singleton::set_is_checkout();
            co::checkout_workflow(
                effective_console,
                name,
                co::CheckoutWorkflowArgs {
                    script,
                    workflow,
                    wf,
                },
                co::CheckoutArgs {
                    env,
                    store,
                    new_branch,
                    create_lock_file,
                    force_install_tools,
                    keep_workspace_on_failure,
                    lock,
                },
            )
            .context(format_context!("While checking out workflow"))?;
        }
        Commands::CheckoutRepo {
            name,
            rule_name,
            url,
            rev,
            clone,
            env,
            store,
            new_branch,
            create_lock_file,
            force_install_tools,
            keep_workspace_on_failure,
            lock,
            locked,
        } => {
            singleton::set_execution_phase(task::Phase::Checkout);
            singleton::set_is_checkout();
            if locked {
                singleton::set_use_locks();
            }

            let is_ci = singleton::get_is_ci().into();
            let group = ci::GithubLogGroup::new_group(
                effective_console.clone(),
                is_ci,
                format!("Spaces Checkout Repo {url}").as_str(),
            )?;

            let result = co::checkout_repo(
                effective_console.clone(),
                name,
                co::CheckoutRepoArgs {
                    rule_name,
                    url,
                    rev,
                    clone,
                },
                co::CheckoutArgs {
                    env,
                    store,
                    new_branch,
                    create_lock_file,
                    force_install_tools,
                    keep_workspace_on_failure,
                    lock,
                },
            )
            .context(format_context!("while checking out repo"));

            group.end_group(effective_console, is_ci)?;
            result?;
        }
        Commands::Co { args } => {
            singleton::set_is_checkout();
            if args.locked {
                singleton::set_use_locks();
            }
            let checkout_map =
                co::Checkout::load().context(format_context!("Failed to load co file"))?;

            let mut checkout = checkout_map
                .get(&args.checkout)
                .context(format_context!(
                    "Failed to find `{}` in `{}`",
                    args.checkout,
                    co::CO_FILE_NAME
                ))?
                .clone();

            checkout
                .apply_overrides(&args)
                .context(format_context!("while applying co overrides"))?;

            checkout
                .clone()
                .checkout(
                    effective_console,
                    args.name,
                    args.keep_workspace_on_failure,
                    args.lock,
                )
                .context(format_context!("while checking out repo"))?;
        }
        Commands::Sync {
            env,
            store,
            no_store,
            dev_branch,
            no_dev_branch,
        } => {
            singleton::set_execution_phase(task::Phase::Checkout);

            if shell::is_spaces_shell() {
                return Err(format_error!("Exit the spaces shell to run `spaces sync`"));
            }

            singleton::set_args_env(env).context(format_context!(
                "while setting environment variables for sync"
            ))?;

            singleton::set_args_store(store)
                .context(format_context!("while setting store values for sync"))?;

            singleton::set_args_store_removals(no_store);

            // Add any dev branches specified by the command line
            if !dev_branch.is_empty() {
                let mut new_branches = singleton::get_new_branches();
                new_branches.extend(dev_branch);
                singleton::set_new_branches(new_branches);
            }

            // Remove any dev branches specified by --no-dev-branch
            if !no_dev_branch.is_empty() {
                let mut removed_branches = singleton::get_removed_branches();
                removed_branches.extend(no_dev_branch);
                singleton::set_removed_branches(removed_branches);
            }

            // Always need to evaluate when doing a sync
            singleton::set_rescan(true);
            singleton::set_is_sync();

            runner::run_starlark_modules_in_workspace(
                effective_console,
                task::Phase::Checkout,
                None,
                workspace::IsClearInputs::Yes,
                runner::RunWorkspace::Target(None, vec![]),
                runner::IsCreateLockFile::No,
                runner::IsExecuteTasks::Yes,
            )
            .context(format_context!("during runner sync"))?;
        }

        Commands::Foreach { mode } => {
            // Extract command_args from the mode

            let (for_each_repo, command_args) = match &mode {
                ForEachMode::Repo { command_args } => (runner::ForEachRepo::Repo, command_args),
                ForEachMode::Branch { command_args } => (runner::ForEachRepo::Branch, command_args),
                ForEachMode::DevBranch { command_args } => {
                    (runner::ForEachRepo::DevBranch, command_args)
                }
                ForEachMode::DirtyBranch { command_args } => {
                    (runner::ForEachRepo::DirtyBranch, command_args)
                }
            };

            if command_args.is_empty() {
                return Err(format_error!(
                    "No command provided to run on each repo. Pass after ` -- `."
                ));
            }

            runner::foreach_repo(
                effective_console,
                runner::RunWorkspace::Target(None, vec![]),
                for_each_repo,
                command_args,
            )
            .context(format_context!("while running command in each repo"))?;
        }

        Commands::Shell {
            path,
            completions,
            all_targets,
        } => {
            if shell::is_spaces_shell() {
                return Err(format_error!("Already running in a `spaces shell`"));
            }
            let completions_command = if completions {
                let has_help = if all_targets {
                    rules::HasHelp::No
                } else {
                    rules::HasHelp::Yes
                };

                Some((Arguments::command(), has_help))
            } else {
                None
            };

            runner::run_shell_in_workspace(effective_console, path, completions_command)
                .context(format_context!("while running user shell"))?;
        }

        Commands::RunLsp {} => {
            #[cfg(feature = "lsp-debug")]
            {
                // Open (or create) a log file for append
                let log_file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(".spaces/lsp.log")?;

                // Redirect the process's stderr to this file
                use std::os::fd::IntoRawFd;
                let fd = log_file.into_raw_fd();
                unsafe {
                    libc::dup2(fd, libc::STDERR_FILENO);
                }
            }

            singleton::enable_lsp_mode();
            starstd::enable_lsp_mode();

            runner::run_lsp(effective_console).context(format_context!("during runner sync"))?;
        }

        Commands::Run {
            target,
            env,
            forget_inputs,
            skip_deps,
            extra_rule_args,
        } => {
            singleton::set_execution_phase(task::Phase::Run);

            if target.is_none() && skip_deps {
                return Err(format_error!(
                    "Skipping dependencies is only allowed when a target is specified."
                ));
            }

            if target.is_none() && !extra_rule_args.is_empty() {
                return Err(format_error!(
                    "Extra rule arguments are only allowed when a target is specified."
                ));
            }

            singleton::set_args_env(env).context(format_context!(
                "while setting environment variables for run rules"
            ))?;

            if skip_deps {
                singleton::enable_skip_deps_mode();
            }

            let is_ci: ci::IsCi = singleton::get_is_ci().into();
            let target_message = target
                .as_ref()
                .map(|target| format!(" {target}"))
                .unwrap_or_default();
            let group = ci::GithubLogGroup::new_group(
                effective_console.clone(),
                is_ci,
                format!("Spaces Run{target_message}").as_str(),
            )?;
            let result = runner::run_starlark_modules_in_workspace(
                effective_console.clone(),
                task::Phase::Run,
                None,
                forget_inputs.into(),
                runner::RunWorkspace::Target(target, extra_rule_args),
                runner::IsCreateLockFile::No,
                runner::IsExecuteTasks::Yes,
            );
            group.end_group(effective_console, is_ci)?;
            result.context(format_context!("while executing run rules"))?;
        }

        Commands::Inspect {
            target,
            filter,
            has_help,
            markdown,
            stardoc,
            fuzzy,
            details,
            json,
            checkout,
            force,
        } => {
            singleton::set_execution_phase(task::Phase::Inspect);
            if checkout {
                if target.is_some() {
                    return Err(format_error!(
                        "checkout mode does not support specifying a target"
                    ));
                }

                if details {
                    return Err(format_error!(
                        "checkout mode does not support specifying `--details`"
                    ));
                }

                if has_help {
                    return Err(format_error!(
                        "checkout mode does not support specifying `--has-help`"
                    ));
                }

                if json {
                    return Err(format_error!(
                        "checkout mode does not support specifying `--json`"
                    ));
                }

                if filter.is_some() {
                    return Err(format_error!(
                        "checkout mode does not support specifying `--filter`"
                    ));
                }

                if markdown.is_some() {
                    return Err(format_error!(
                        "checkout mode does not support specifying `--markdown`"
                    ));
                }
            } else if force {
                return Err(format_error!(
                    "`--force` is only supported with `--checkout`"
                ));
            }

            if effective_console.get_level() > console::Level::Info {
                effective_console.set_level(console::Level::Info);
            }

            if details && target.is_none() {
                return Err(format_error!("details requires a rule to be specified"));
            }

            if json && !details {
                return Err(format_error!("json requires `--details`"));
            }

            if details {
                effective_console.set_is_show_progress_bars(false);
                effective_console.set_level(console::Level::Warning);
            }

            let mut filter_globs = std::collections::HashSet::new();
            if let Some(filter) = filter {
                let filter_parts = filter.split(',');
                for glob_expression in filter_parts {
                    let effective_expressions =
                        if glob_expression.starts_with('-') || glob_expression.starts_with('+') {
                            vec![glob_expression.to_string()]
                        } else if glob_expression.contains('*') {
                            vec![format!("+{}", glob_expression)]
                        } else {
                            vec![
                                format!("+**/*:*{}*", glob_expression),
                                format!("+**/*{}*:*", glob_expression),
                                format!("+**/{}*:*", glob_expression),
                                format!("+**/*{}*/*:*", glob_expression),
                            ]
                        };
                    for exp in effective_expressions {
                        filter_globs.insert(exp.into());
                    }
                }
            }

            if stardoc.is_some() {
                singleton::set_rescan(true);
            }
            singleton::set_inspect_options(utils::inspect::Options {
                target: target.clone(),
                filter_globs,
                has_help,
                markdown,
                stardoc,
                fuzzy,
                details,
                json,
                checkout,
                force,
            });

            runner::run_starlark_modules_in_workspace(
                effective_console,
                task::Phase::Inspect,
                None,
                workspace::IsClearInputs::No,
                runner::RunWorkspace::Target(target, vec![]),
                runner::IsCreateLockFile::No,
                runner::IsExecuteTasks::Yes,
            )
            .context(format_context!("while executing run rules"))?;
        }

        Commands::Query { command } => {
            singleton::set_execution_phase(task::Phase::Inspect);
            singleton::set_query_command(command.clone());

            // When exporting stardoc, reuse the inspect --stardoc pipeline:
            // set_rescan forces re-evaluation (clears cached hashes → is_dirty=true)
            // so evaluate_starlark_modules runs and collects per-module docs, then
            // calls workspace.stardoc.generate(path) before execute_tasks returns.
            if let Some(stardoc_path) = command.export_stardoc_path() {
                singleton::set_rescan(true);
                singleton::set_inspect_options(utils::inspect::Options {
                    stardoc: Some(stardoc_path),
                    ..Default::default()
                });
            }

            if effective_console.get_level() > console::Level::Info {
                effective_console.set_level(console::Level::Info);
            }

            runner::run_starlark_modules_in_workspace(
                effective_console.clone(),
                task::Phase::Inspect,
                None,
                workspace::IsClearInputs::No,
                runner::RunWorkspace::Target(None, vec![]),
                runner::IsCreateLockFile::No,
                runner::IsExecuteTasks::Yes,
            )
            .context(format_context!("while loading workspace for query"))?;

            let ctx = singleton::take_query_context()
                .ok_or_else(|| format_error!("Internal error: query context was not built"))?;

            command
                .execute(effective_console, &ctx)
                .context(format_context!("while executing query command"))?;
        }

        Commands::Completions {
            shell,
            output,
            all_targets,
        } => {
            let has_help = if all_targets {
                rules::HasHelp::No
            } else {
                rules::HasHelp::Yes
            };

            // rules are now available
            let run_targets = runner::run_starlark_get_targets(effective_console, has_help)
                .context(format_context!("Failed to get targets"))?;

            let completion_content = completions::generate_workspace_completions(
                &Arguments::command(),
                shell,
                run_targets,
            )
            .context(format_context!("Failed to generate workspace completions"))?;

            //write content to output
            std::fs::write(std::path::Path::new(output.as_ref()), completion_content).context(
                format_context!("Failed to write workspace completions to file {output}"),
            )?;
        }
        Commands::Docs {} => {
            docs::show(effective_console)?;
        }
        Commands::Tools { command } => {
            effective_console.set_level(console::Level::Info);
            tools::handle_command(effective_console, command)
                .context(format_context!("Failed to handle tool command"))?;
        }
        Commands::Logs { command } => {
            effective_console.set_is_show_progress_bars(false);
            runner::run_logs_command_in_workspace(effective_console, command)
                .context(format_context!("Failed to run logs command"))?
        }
        Commands::Store { command } => {
            if effective_console.get_level() > console::Level::Info {
                effective_console.set_level(console::Level::Info);
            }
            runner::run_store_command_in_workspace(effective_console, command)
                .context(format_context!("Failed to run store command"))?
        }
        Commands::Features { command } => {
            if effective_console.get_level() > console::Level::Info {
                effective_console.set_level(console::Level::Info);
            }
            runner::run_features_command_in_workspace(effective_console, command)
                .context(format_context!("Failed to run features command"))?
        }
        Commands::Version { command } => {
            if effective_console.get_level() > console::Level::Info {
                effective_console.set_level(console::Level::Info);
            }
            runner::run_version_command_in_workspace(effective_console, command)
                .context(format_context!("Failed to run version command"))?
        }
    }
    Ok(())
}

#[derive(Debug, Subcommand, Clone)]
enum ForEachMode {
    /// Run the command in each repository in the workspace.
    Repo {
        /// The arguments to pass to the command.
        #[arg(
            trailing_var_arg = true,
            help = r"Command plus arguments to run in each repo (passed after `--`)"
        )]
        command_args: Vec<Arc<str>>,
    },
    /// Run the command in each repository in the workspace that is checked out on a branch .
    Branch {
        /// The arguments to pass to the command.
        #[arg(
            trailing_var_arg = true,
            help = r"Command plus arguments to run in each repo on a branch (passed after `--`)"
        )]
        command_args: Vec<Arc<str>>,
    },
    /// Run the command in each repository where the branch is dirty.
    DirtyBranch {
        /// The arguments to pass to the command.
        #[arg(
            trailing_var_arg = true,
            help = r"Command plus arguments to run in each repo on a dirty branch (passed after `--`)"
        )]
        command_args: Vec<Arc<str>>,
    },
    /// Run the command in each repository that was checked out as a development branch.
    DevBranch {
        /// The arguments to pass to the command.
        #[arg(
            trailing_var_arg = true,
            help = r"Command plus arguments to run in each repo on a development branch (passed after `--`)"
        )]
        command_args: Vec<Arc<str>>,
    },
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(about = r#"
Executes the checkout rules in the specified scripts or workflow files."#)]
    Checkout {
        /// The name of the workspace to create.
        #[arg(long)]
        name: Arc<str>,
        #[arg(
            long,
            help = r#"Environment variables to add to the checked out workspace.
  Use `--env=VAR=VALUE`. Makes workspace not reproducible."#
        )]
        env: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Store values in the checkout store accessible via workspace.load_value().
  Use `--store=KEY=VALUE`. Values are stored with path `//` and url `<command line>`.
  Command line store values take priority over all other path or url values."#
        )]
        store: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Use --new-branch=<rule> to have spaces create a new branch for the rule.
  Branch name will match the workspace name."#
        )]
        new_branch: Vec<Arc<str>>,
        /// The path(s) to the `spaces.star` file containing checkout rules. Paths are processed in order.
        #[arg(long, value_hint = ValueHint::FilePath)]
        script: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Scripts to process in the format of `--workflow=<directory>:<script>,<script>,...`.
  `--script` is processed before `--workflow`.

  If <directory> has `workflows.spaces.toml`, it will be parsed for shortcuts if only one <script> is passed.
  - `spaces checkout --workflow=workflows:my-shortcut --name=workspace-name`
    - run scripts listed in `my-shortcut` in `workflows/workflows.spaces.toml`
  - `spaces checkout --workflow=workflows:preload,my-shortcut --name=workspace-name`
    - run `workflows/preload.spaces.star` then `workflows/my-shortcut.spaces.star`

  ```toml
  my-shortcut = ["preload", "my-shortcut"]
  ```"#
        )]
        workflow: Option<Arc<str>>,
        /// Shortcut for --workflow
        #[arg(long)]
        wf: Option<Arc<str>>,
        #[arg(
            long,
            help = r#"Create a lock file for the workspace.
  This file can be passed on the next checkout as a script to re-create the exact workspace."#
        )]
        create_lock_file: bool,
        /// Force install the tools spaces needs to run.
        #[arg(long)]
        force_install_tools: bool,
        /// Do not delete the workspace directory if checkout fails.
        #[arg(long)]
        keep_workspace_on_failure: bool,
        #[arg(
            long,
            help = r#"Override locks set in the rules.
  Use `--lock=REPO=REV`. Can be used multiple times."#
        )]
        lock: Vec<Arc<str>>,
    },
    #[command(about = r#"
Uses git to clone a repository in a new workspace and evaluates the top level [*]spaces.star files.
This can be used if the repository defines all of its own dependencies."#)]
    CheckoutRepo {
        #[arg(long)]
        /// The new workspace name.
        name: Arc<str>,
        #[arg(
            long,
            help = r#"The name to give to the repo checkout rule.
  This will also be the name of the directory where the repository is cloned.
  The default behavior is to infer the name from the URL."#
        )]
        rule_name: Option<Arc<str>>,
        /// The URL of the repository to clone.
        #[arg(long)]
        url: Arc<str>,
        /// The revision (branch/commit/tag) to checkout
        #[arg(long)]
        rev: Arc<str>,
        #[arg(
            long,
            help = r#"Use --new-branch=<rule> to have spaces create a new branch for the rule.
  Branch name will match the workspace name. This can be used multiple times."#
        )]
        new_branch: Vec<Arc<str>>,
        /// The method to use for cloning the repository (default is a standard clone).
        #[arg(long)]
        clone: Option<git::Clone>,
        #[arg(
            long,
            help = r#"Environment variables to add to the checked out workspace.
  Use `--env=VAR=VALUE`. Makes workspace not reproducible."#
        )]
        env: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Store values in the checkout store accessible via workspace.load_value().
  Use `--store=KEY=VALUE`. Values are stored with path `//` and url `<command line>`.
  Command line store values take priority over all other path or url values."#
        )]
        store: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Create a lock file for the workspace.
  This file can be passed on the next checkout as a script to re-create the exact workspace.
  This argument is deprecated and will be removed in a future release."#
        )]
        create_lock_file: bool,
        /// Force install the tools spaces needs to run.
        #[arg(long)]
        force_install_tools: bool,
        /// Do not delete the workspace directory if checkout fails.
        #[arg(long)]
        keep_workspace_on_failure: bool,
        #[arg(
            long,
            help = r#"Override locks set in the rules.
  Use `--lock=REPO=REV`. Can be used multiple times."#
        )]
        lock: Vec<Arc<str>>,
        /// The workspaces lock rev's will override the rule rev for repos
        #[arg(long)]
        locked: bool,
    },
    #[command(about = r#"
The shortform version of `checkout` and `checkout-repo`. The details of the command are
loaded from `co.spaces.toml` in the current directory.

```toml
[spaces-dev.Repo]
url = "https://github.com/work-spaces/spaces"
rule-name = "spaces" # optionally checkout in a different directory - default is from URL
rev = "main" # branch/tag/commit to checkout
new-branch = ["spaces"] # optionally create a new branch for a git repository
clone = "Default" # optionally clone type Default/Blobless
env = ["SET_VALUE=VALUE", "ANOTHER_VALUE=ANOTHER_VALUE"] # optionally add environment variables
create-lock-file = false # optionally create a lock file


[ninja-build.Workflow]
# Loads the ninja-build-dev flow from workflows/workflows.spaces.toml
workflow = "workflows:ninja-build-dev" # Workflow to checkout or use script
script = ["workflows/preload", "workflows/ninja-build"] # Use in place of or addition to workflow
env = ["SET_VALUE=VALUE", "ANOTHER_VALUE=ANOTHER_VALUE"] # optionally add environment variables
new-branch = ["spaces"] # optionally create a new branch for a git repository
create-lock-file = false # optionally create a lock file
```
"#)]
    Co {
        #[command(flatten)]
        args: co::CoArgs,
    },
    /// Runs checkout rules within an existing workspace
    Sync {
        #[arg(
            long,
            help = r#"Environment variables to add to the workspace.
  Use `--env=VAR=VALUE`. Makes workspace not reproducible."#
        )]
        env: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Store values accessible via workspace.load_value().
  Use `--store=KEY=VALUE`. Values are stored with path `//` and url `<command line>`.
  Command line store values take priority over all other path or url values."#
        )]
        store: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Remove a store value previously set via --store=KEY=VALUE.
  Use `--no-store=KEY`. Removes the named key from the command-line store entry."#
        )]
        no_store: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Use --dev-branch=<rule> to add a repo to the dev-branch list.
  Unlike --new-branch on checkout, this does not create a new git branch."#
        )]
        dev_branch: Vec<Arc<str>>,
        #[arg(
            long,
            help = r#"Use --no-dev-branch=<rule> to remove a repo from the dev-branch list.
  This has the opposite effect of --dev-branch."#
        )]
        no_dev_branch: Vec<Arc<str>>,
    },
    #[command(about = r"Runs a spaces run rule.
  - `spaces run`: Run all non-optional rules with dependencies
  - `spaces run my-target`: Run a single target plus dependencies
  - `spaces run my-target -- --some-arg --some-other-arg`: pass additional arguments to a rule")]
    Run {
        /// The name of the target to run (default is all targets).
        target: Option<Arc<str>>,
        /// Forces rules to run even if input globs are the same as last time.
        #[arg(long)]
        forget_inputs: bool,
        /// Runs only the target specified, without executing dependencies.
        #[arg(long)]
        skip_deps: bool,
        /// Environment variables to override during the run. Use `--env=VAR=VALUE`.
        #[arg(long)]
        env: Vec<Arc<str>>,
        #[arg(
            trailing_var_arg = true,
            help = r"Extra arguments to pass to the rule (passed after `--`)"
        )]
        extra_rule_args: Vec<Arc<str>>,
    },
    #[command(
        about = r"Inspect all the scripts in the workspace without running any rules.
  - `spaces inspect`: show the rules that have `help` entries:
  - `spaces inspect <target-name>`: show target plus dependencies
  - `spaces --verbosity=message inspect`: show all rules
  - `spaces --verbosity=debug inspect`: show all rules in detail"
    )]
    Inspect {
        /// The name of the target to evaluate (default is all targets).
        target: Option<Arc<str>>,
        /// Filter targets with a glob (e.g. `--filter=**/my-target`)
        #[arg(long)]
        filter: Option<Arc<str>>,
        /// Only show rules with the help entry populated
        #[arg(long)]
        has_help: bool,
        /// Inspect the command that will checkout the workspace in the current state
        #[arg(long)]
        checkout: bool,
        /// When used with --checkout, warn if a repo is dirty but still print the checkout command
        #[arg(long)]
        force: bool,
        /// Write the output of the inspect command to a markdown file
        #[arg(long)]
        markdown: Option<Arc<str>>,
        /// Write the starlark documentation to the specified path
        #[arg(long)]
        stardoc: Option<Arc<str>>,
        /// Use fuzzy matching when searching for targets (e.g. `--fuzzy="my query"`)
        #[arg(long)]
        fuzzy: Option<Arc<str>>,
        /// Show the rule details of the specified rule
        #[arg(long)]
        details: bool,
        /// Show the rule details in JSON format (works with details)
        #[arg(long)]
        json: bool,
    },
    /// Query workspace rules without running any of them.
    Query {
        #[command(subcommand)]
        command: utils::query::QueryCommand,
    },
    /// Generates shell completions for the spaces command.
    Completions {
        /// Target shell
        #[arg(long, value_enum)]
        shell: clap_complete::Shell,
        /// Output file path
        #[arg(long)]
        output: Arc<str>,
        /// Show rules for all run targets not just those with help populated
        #[arg(long)]
        all_targets: bool,
    },
    /// Shows the documentation for spaces starlark modules.
    Docs {},
    /// Commands for managing internal spaces tools.
    Tools {
        /// The mode to run the command in.
        #[command(subcommand)]
        command: tools::Command,
    },
    /// Runs a command in each repo or branch in the workspace.
    Foreach {
        /// The mode to run the command in.
        #[command(subcommand)]
        mode: ForEachMode,
    },
    /// Runs an interactive shell using the workspace environment.
    Shell {
        /// Path to the shell to run. Default is /bin/bash
        #[arg(long)]
        path: Option<Arc<str>>,
        /// Generate and source completions for the shell
        #[arg(long)]
        completions: bool,
        /// Include all run targets in completions not just those with help populated
        #[arg(long)]
        all_targets: bool,
    },
    /// Query the status of rules from the logs.
    Logs {
        /// The logs command to run.
        #[command(subcommand)]
        command: logs::LogsCommand,
    },
    /// Commands for managing the spaces store (cache).
    Store {
        /// The mode to run the command in.
        #[command(subcommand)]
        command: store::StoreCommand,
    },
    /// Commands for managing feature flags.
    Features {
        /// The mode to run the command in.
        #[command(subcommand)]
        command: features::FeaturesCommand,
    },
    /// Commands for managing the spaces version including checking for updates.
    Version {
        /// The mode to run the command in.
        #[command(subcommand)]
        command: version::Command,
    },
    /// Run the Spaces language server protocol (experimental).
    RunLsp {},
}
