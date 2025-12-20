use crate::{co, completions, docs, evaluator, rules, runner, singleton, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum, ValueHint};
use std::{io::IsTerminal, sync::Arc};
use utils::{git, shell};

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

impl From<Level> for printer::Level {
    fn from(level: Level) -> Self {
        match level {
            Level::Trace => printer::Level::Trace,
            Level::Debug => printer::Level::Debug,
            Level::Message => printer::Level::Message,
            Level::Info => printer::Level::Info,
            Level::App => printer::Level::App,
            Level::Passthrough => printer::Level::Passthrough,
            Level::Warning => printer::Level::Warning,
            Level::Error => printer::Level::Error,
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
    /// Rescan the workspace for *spaces.star files
    #[arg(long)]
    rescan: bool,
    #[command(subcommand)]
    commands: Commands,
}

fn handle_verbosity(
    printer: &mut printer::Printer,
    verbosity: printer::Level,
    is_ci: bool,
    rescan: bool,
    is_hide_progress_bars: bool,
    show_elapsed_time: bool,
) {
    singleton::set_rescan(rescan);
    if is_ci {
        singleton::set_ci(true);
        printer.verbosity.level = printer::Level::Trace;
        printer.verbosity.is_show_progress_bars = false;
        printer.verbosity.is_show_elapsed_time = true;
    } else {
        printer.verbosity.level = verbosity;
        printer.verbosity.is_show_progress_bars = !is_hide_progress_bars;
        printer.verbosity.is_show_elapsed_time = show_elapsed_time;
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

    // terminate immediately if ctrl+c is received twice
    use signal_hook::consts::SIGINT;
    let term_now = Arc::new(std::sync::atomic::AtomicBool::new(false));
    signal_hook::flag::register_conditional_shutdown(SIGINT, 1, Arc::clone(&term_now))?;
    signal_hook::flag::register(SIGINT, Arc::clone(&term_now))?;

    let args = Arguments::parse();
    let mut stdout_printer = printer::Printer::new_stdout();

    match args {
        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands:
                Commands::Checkout {
                    name,
                    env,
                    new_branch,
                    script,
                    workflow,
                    wf,
                    create_lock_file,
                    force_install_tools,
                    keep_workspace_on_failure,
                },
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            co::checkout_workflow(
                &mut stdout_printer,
                name,
                env,
                new_branch,
                script,
                workflow,
                wf,
                create_lock_file,
                force_install_tools,
                keep_workspace_on_failure,
            )
            .context(format_context!("While checking out workflow"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands:
                Commands::CheckoutRepo {
                    name,
                    rule_name,
                    url,
                    rev,
                    clone,
                    env,
                    new_branch,
                    create_lock_file,
                    force_install_tools,
                    keep_workspace_on_failure,
                },
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            co::checkout_repo(
                &mut stdout_printer,
                name,
                rule_name,
                url,
                rev,
                clone,
                env,
                new_branch,
                create_lock_file,
                force_install_tools,
                keep_workspace_on_failure,
            )
            .context(format_context!("while checking out repo"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands:
                Commands::Co {
                    checkout,
                    name,
                    keep_workspace_on_failure,
                },
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            let checkout_map =
                co::Checkout::load().context(format_context!("Failed to load co file"))?;

            let checkout = checkout_map.get(&checkout).context(format_context!(
                "Failed to find `{}` in `{}`",
                checkout,
                co::CO_FILE_NAME
            ))?;

            checkout
                .clone()
                .checkout(&mut stdout_printer, name, keep_workspace_on_failure)
                .context(format_context!("while checking out repo"))?;
        }
        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands: Commands::Sync {},
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            if shell::is_spaces_shell() {
                return Err(format_error!("Exit the spaces shell to run `spaces sync`"));
            }

            // Always need to evaluate when doing a sync
            singleton::set_rescan(true);
            singleton::set_is_sync();

            runner::run_starlark_modules_in_workspace(
                &mut stdout_printer,
                task::Phase::Checkout,
                None,
                workspace::IsClearInputs::Yes,
                runner::RunWorkspace::Target(None, vec![]),
                runner::IsCreateLockFile::No,
                runner::IsExecuteTasks::Yes,
            )
            .context(format_context!("during runner sync"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands: Commands::Foreach { mode },
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

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
                &mut stdout_printer,
                runner::RunWorkspace::Target(None, vec![]),
                for_each_repo,
                command_args,
            )
            .context(format_context!("while running command in each repo"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands: Commands::Shell { path },
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            if shell::is_spaces_shell() {
                return Err(format_error!("Already running in a `spaces shell`"));
            }

            runner::run_shell_in_workspace(&mut stdout_printer, path)
                .context(format_context!("while running user shell"))?;
        }

        #[cfg(feature = "lsp")]
        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands: Commands::RunLsp {},
        } => {
            let mut null_printer = printer::Printer::new_null_term();

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

            handle_verbosity(
                &mut null_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            singleton::enable_lsp_mode();

            runner::run_lsp(&mut null_printer).context(format_context!("during runner sync"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands:
                Commands::Run {
                    target,
                    env,
                    forget_inputs,
                    skip_deps,
                    extra_rule_args,
                },
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

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

            runner::run_starlark_modules_in_workspace(
                &mut stdout_printer,
                task::Phase::Run,
                None,
                forget_inputs.into(),
                runner::RunWorkspace::Target(target, extra_rule_args),
                runner::IsCreateLockFile::No,
                runner::IsExecuteTasks::Yes,
            )
            .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands:
                Commands::Inspect {
                    target,
                    filter,
                    has_help,
                    markdown,
                    stardoc,
                },
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            if stdout_printer.verbosity.level > printer::Level::Info {
                stdout_printer.verbosity.level = printer::Level::Info;
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

            singleton::set_inspect_globs(filter_globs);
            singleton::set_has_help(has_help);
            singleton::set_inspect_markdown_path(markdown);
            if stardoc.is_some() {
                singleton::set_rescan(true);
            }
            singleton::set_inspect_stardoc_path(stardoc);

            runner::run_starlark_modules_in_workspace(
                &mut stdout_printer,
                task::Phase::Inspect,
                None,
                workspace::IsClearInputs::No,
                runner::RunWorkspace::Target(target, vec![]),
                runner::IsCreateLockFile::No,
                runner::IsExecuteTasks::Yes,
            )
            .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands: Commands::Completions { shell, output },
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            runner::run_starlark_modules_in_workspace(
                &mut stdout_printer,
                task::Phase::Inspect,
                None,
                workspace::IsClearInputs::No,
                runner::RunWorkspace::Target(None, vec![]),
                runner::IsCreateLockFile::No,
                runner::IsExecuteTasks::No,
            )
            .context(format_context!("while executing run rules"))?;

            // rules are now available
            let run_targets =
                rules::get_run_targets().context(format_context!("Failed to get run targets"))?;

            let completion_content = completions::generate_workspace_completions(
                &Arguments::command(),
                shell,
                run_targets,
            )
            .context(format_context!("Failed to generate workspace completions"))?;

            //write content to stdout
            std::fs::write(std::path::Path::new(output.as_ref()), completion_content).context(
                format_context!("Failed to write workspace completions to file {output}"),
            )?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            show_elapsed_time,
            ci,
            rescan,
            commands: Commands::Docs { item },
        } => {
            handle_verbosity(
                &mut stdout_printer,
                verbosity.into(),
                ci,
                rescan,
                hide_progress_bars,
                show_elapsed_time,
            );

            docs::show(&mut stdout_printer, item)?;
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
            help = r"Command plus arguments to run in each repo on a dirty branch (passed after `--`)"
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
        /// The name of the checkout entry (e.g. `spaces-dev` or `ninja-build` from above).
        checkout: Arc<str>,
        /// The name of the workspace to create.
        name: Arc<str>,
        /// Do not delete the workspace directory if checkout fails.
        #[arg(long)]
        keep_workspace_on_failure: bool,
    },
    /// Runs checkout rules within an existing workspace (experimental)
    Sync {},
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
        /// Write the output of the inspect command to a markdown file
        #[arg(long)]
        markdown: Option<Arc<str>>,
        /// Write the starlark documentation to the specified path
        #[arg(long)]
        stardoc: Option<Arc<str>>,
    },
    /// Generates shell completions for the spaces command (experimental)
    Completions {
        /// Target shell
        #[arg(long, value_enum)]
        shell: clap_complete::Shell,
        /// Output file path
        #[arg(long)]
        output: Arc<str>,
    },
    /// Shows the documentation for spaces starlark modules.
    Docs {
        /// What documentation do you want to see?
        #[arg(value_enum)]
        item: Option<docs::DocItem>,
    },
    /// Runs a command in each repo or branch in the workspace.
    Foreach {
        /// The mode to run the command in.
        #[command(subcommand)]
        mode: ForEachMode,
    },
    /// Runs an interactive shell using the workspace environment (experimental).
    Shell {
        /// Path to the shell to run. Default is /bin/bash
        #[arg(long)]
        path: Option<Arc<str>>,
    },
    /// Run the Spaces language server protocol. Not currently functional.
    #[cfg(feature = "lsp")]
    RunLsp {},
}
