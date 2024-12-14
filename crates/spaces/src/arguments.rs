use crate::{docs, evaluator, rules, tools, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum, ValueHint};

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Level {
    Trace,
    Debug,
    Message,
    Info,
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
            Level::Warning => printer::Level::Warning,
            Level::Error => printer::Level::Error,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub struct Arguments {
    /// The verbosity level of the output.
    #[arg(short, long, default_value = "warning")]
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

enum RunWorkspace {
    Target(Option<String>),
    Script(Vec<(String, String)>),
}

fn run_starlark_script(name: &str, contents: &str) -> anyhow::Result<()> {
    evaluator::run_starlark_script(name, contents)
        .context(format_context!("Failed to run evaluate starlark script"))?;
    Ok(())
}

fn run_starlark_modules_in_workspace(
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
            workspace::set_digest(workspace::calculate_digest(&scripts));
            evaluator::run_starlark_modules(printer, scripts, phase, None)
                .context(format_context!("while executing checkout rules"))?
        }
    }
    Ok(())
}

fn handle_verbosity(
    printer: &mut printer::Printer,
    verbosity: printer::Level,
    is_ci: bool,
    is_hide_progress_bars: bool,
) {
    if is_ci {
        workspace::set_ci_true();
        printer.verbosity.level = printer::Level::Trace;
        printer.verbosity.is_show_progress_bars = false;
    } else {
        printer.verbosity.level = verbosity;
        printer.verbosity.is_show_progress_bars = !is_hide_progress_bars;
    }
}

pub fn execute() -> anyhow::Result<()> {
    use crate::ledger;

    if std::env::args().len() == 1 {
        let mut stdin_contents = String::new();
        use std::io::Read;
        std::io::stdin().read_to_string(&mut stdin_contents)?;
        run_starlark_script(workspace::SPACES_STDIN_NAME, stdin_contents.as_str())
            .context(format_context!("Failed to run starlark script"))?;
        return Ok(());
    }

    if std::env::args().len() >= 2 {
        let filename = std::env::args().nth(1).unwrap();
        let input = std::path::Path::new(filename.as_str());
        if input.exists() && input.extension().unwrap_or_default() == "star" {
            starstd::script::set_args(std::env::args().skip(1).collect());

            let input_contents = std::fs::read_to_string(input)
                .context(format_context!("Failed to read input file {input:?}"))?;
            run_starlark_script(filename.as_str(), input_contents.as_str())
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
                    create_lock_file,
                },
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);

            tools::install_tools(&mut printer)
                .context(format_context!("while installing tools"))?;

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

            std::env::set_current_dir(target_workspace_directory.clone()).context(
                format_context!(
                    "Failed to set current directory to {:?}",
                    target_workspace_directory
                ),
            )?;

            workspace::set_create_lock_file(create_lock_file);

            run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Checkout,
                RunWorkspace::Script(scripts),
            )
            .context(format_context!("while executing checkout rules"))?;

            settings.save(&workspace::absolute_path()).context(format_context!("while saving settings"))?;
            workspace::save_lock_file().context(format_context!("Failed to save workspace lock file"))?;
            
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::Sync {},
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);
            run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Checkout,
                RunWorkspace::Target(None),
            )
            .context(format_context!("while executing checkout rules"))?;
        }

        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::Run { target },
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);

            run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Run,
                RunWorkspace::Target(target),
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

            run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Evaluate,
                RunWorkspace::Target(target),
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

        Arguments {
            verbosity,
            hide_progress_bars,
            ci,
            commands: Commands::List {},
        } => {
            handle_verbosity(&mut printer, verbosity.into(), ci, hide_progress_bars);

            let ledger =
                ledger::Ledger::new().with_context(|| format_context!("while creating ledger"))?;
            ledger.show_status()?;
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
        name: String,
        /// The path(s) to the star file containing checkout rules. Paths are processed in order.
        #[arg(long, value_hint = ValueHint::FilePath)]
        script: Vec<String>,
        /// Create a lock file for the workspace. This file can be passed on the next checkout as a script to re-create the exact workspace.
        #[arg(long)]
        create_lock_file: bool,
    },
    /// Synchronizes the workspace with the checkout rules.
    Sync {},
    /// Executes the Run phase rules.
    Run {
        /// The name of the target to run (default is all targets).
        #[arg(long)]
        target: Option<String>,
    },
    /// List the targets with all details in the workspace.
    Evaluate {
        /// The name of the target to evaluate (default is all targets).
        #[arg(long)]
        target: Option<String>,
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
    /// Lists the workspaces in the spaces store on the local machine. (experimental)
    List {},
}
