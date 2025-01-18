use crate::{docs, evaluator, rules, runner, singleton, tools, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum, ValueHint};
use std::sync::Arc;

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Level {
    Trace,
    Debug,
    Message,
    Info,
    App,
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
            Level::Warning => printer::Level::Warning,
            Level::Error => printer::Level::Error,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    /// The verbosity level of the output.
    #[arg(short, long, default_value = "app")]
    pub verbosity: Level,
    #[arg(long)]
    /// Dont show progress bars
    pub hide_progress_bars: bool,
    /// If this is passed, info.is_ci() returns true in scripts.
    #[arg(long)]
    ci: bool,
    #[command(subcommand)]
    commands: Commands,
}

fn handle_verbosity(
    printer: &mut printer::Printer,
    verbosity: printer::Level,
    is_ci: bool,
    is_hide_progress_bars: bool,
) {
    if is_ci {
        singleton::set_ci(true);
        printer.verbosity.level = printer::Level::Trace;
        printer.verbosity.is_show_progress_bars = false;
    } else {
        printer.verbosity.level = verbosity;
        printer.verbosity.is_show_progress_bars = !is_hide_progress_bars;
    }
}

pub fn execute() -> anyhow::Result<()> {
    if std::env::args().len() == 1 {
        let mut stdin_contents = String::new();
        use std::io::Read;
        std::io::stdin().read_to_string(&mut stdin_contents)?;
        evaluator::run_starlark_script(workspace::SPACES_STDIN_NAME.into(), stdin_contents.into())
            .context(format_context!("Failed to run starlark script"))?;
        return Ok(());
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
    let mut printer = printer::Printer::new_stdout();

    match args {
        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands:
                Commands::Checkout {
                    name,
                    script,
                    workflow,
                    create_lock_file,
                    force_install_tools,
                },
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);

            let mut inputs: Vec<Arc<str>> = vec![];
            inputs.extend(script.clone());
            if let Some(workflow) = workflow {
                let parts: Vec<&str> = workflow.split(':').collect();
                if parts.len() != 2 {
                    return Err(format_error!("Invalid workflow format: {}.\n Use --workflow=<directory>:<script>,<script>,...", workflow));
                }
                let directory = parts[0];
                let scripts = parts[1].split(',');
                for script in scripts {
                    inputs.push(format!("{}/{}", directory, script).into());
                }
            }

            tools::install_tools(&mut printer, force_install_tools)
                .context(format_context!("while installing tools"))?;

            runner::checkout(&mut printer, name, inputs, create_lock_file)
                .context(format_context!("during runner checkout"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::Sync {},
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);
            runner::run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Checkout,
                None,
                true,
                runner::RunWorkspace::Target(None),
                false,
            )
            .context(format_context!("during runner sync"))?;
        }

        #[cfg(feature = "lsp")]
        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::RunLsp {},
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);
            runner::run_lsp(&mut printer).context(format_context!("during runner sync"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::Run { target, forget_inputs },
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);

            runner::run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Run,
                None,
                forget_inputs,
                runner::RunWorkspace::Target(target),
                false,
            )
            .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::Evaluate { target },
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);

            if printer.verbosity.level > printer::Level::Info {
                printer.verbosity.level = printer::Level::Info;
            }

            runner::run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Evaluate,
                None,
                false,
                runner::RunWorkspace::Target(target),
                false,
            )
            .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::Completions { shell },
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);

            clap_complete::generate(
                shell,
                &mut Arguments::command(),
                "spaces",
                &mut std::io::stdout(),
            );
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::Docs { item },
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);

            docs::show(&mut printer, item)?;
        }
    }

    Ok(())
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Executes the Checkout phase rules for the script and its dependencies.
    Checkout {
        /// The name of the workspace
        #[arg(long)]
        name: Arc<str>,
        /// The path(s) to the star file containing checkout rules. Paths are processed in order.
        #[arg(long, value_hint = ValueHint::FilePath)]
        script: Vec<Arc<str>>,
        /// Workflow scripts to process in the format of "--workflow=<directory>:<script>,<script>,...". --script is processed first.
        #[arg(long)]
        workflow: Option<Arc<str>>,
        /// Create a lock file for the workspace. This file can be passed on the next checkout as a script to re-create the exact workspace.
        #[arg(long)]
        create_lock_file: bool,
        /// Force install the tools spaces needs to run.
        #[arg(long)]
        force_install_tools: bool,
    },
    /// Synchronizes the workspace with the checkout rules.
    Sync {},
    /// Executes the Run phase rules.
    Run {
        /// The name of the target to run (default is all targets).
        #[arg(long)]
        target: Option<Arc<str>>,
        /// Forces rules to run even if input globs are the same as last time.
        #[arg(long)]
        forget_inputs: bool,
    },
    /// List the targets with all details in the workspace.
    Evaluate {
        /// The name of the target to evaluate (default is all targets).
        #[arg(long)]
        target: Option<Arc<str>>,
    },
    /// Generates shell completions for the spaces command.
    Completions {
        /// The shell to generate the completions for
        #[arg(long, value_enum)]
        shell: clap_complete::Shell,
    },
    /// Shows the documentation for spaces starlark modules.
    Docs {
        /// What documentation do you want to see?
        #[arg(value_enum)]
        item: Option<docs::DocItem>,
    },
    /// Run the Spaces language server protocol. Not currently functional.
    #[cfg(feature = "lsp")]
    RunLsp {},
}
