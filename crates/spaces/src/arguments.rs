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
    #[command(subcommand)]
    commands: Commands,
}

enum RunWorkspace {
    Target(Option<String>),
    Script(String),
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
        RunWorkspace::Script(script) => {
            let modules = vec![("checkout.star".to_string(), script)];
            evaluator::run_starlark_modules(printer, modules, phase, None)
                .context(format_context!("while executing checkout rules"))?
        }
    }
    Ok(())
}

pub fn execute() -> anyhow::Result<()> {
    use crate::ledger;

    if std::env::args().len() == 1 {
        let mut stdin_contents = String::new();
        use std::io::Read;
        std::io::stdin().read_to_string(&mut stdin_contents)?;
        run_starlark_script("stdin", stdin_contents.as_str())
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

    // install pre-requisites
    tools::install_tools(&mut printer).context(format_context!("while installing tools"))?;

    match args {
        Arguments {
            verbosity,
            commands: Commands::Checkout { name, script },
        } => {
            printer.level = verbosity.into();
            std::fs::create_dir_all(name.as_str())
                .context(format_context!("while creating workspace directory {name}"))?;

            let script_contents = std::fs::read_to_string(script.as_str())
                .context(format_context!("while reading script file {script}"))?;

            std::fs::write(format!("{}/{}", name, workspace::WORKSPACE_FILE_NAME), "")
                .context(format_context!("while creating spaces_deps.toml file"))?;

            let current_working_directory = std::env::current_dir()
                .context(format_context!("Failed to get current working directory"))?;

            let target_workspace_directory = current_working_directory.join(name.as_str());

            std::env::set_current_dir(target_workspace_directory.clone()).context(
                format_context!(
                    "Failed to set current directory to {:?}",
                    target_workspace_directory
                ),
            )?;

            run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Checkout,
                RunWorkspace::Script(script_contents),
            )
            .context(format_context!("while executing checkout rules"))?;
        }

        Arguments {
            verbosity,
            commands: Commands::Sync {},
        } => {
            printer.level = verbosity.into();

            run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Checkout,
                RunWorkspace::Target(None),
            )
            .context(format_context!("while executing checkout rules"))?;
        }

        Arguments {
            verbosity,
            commands: Commands::Run { target },
        } => {
            printer.level = verbosity.into();

            run_starlark_modules_in_workspace(
                &mut printer,
                rules::Phase::Run,
                RunWorkspace::Target(target),
            )
            .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            verbosity,
            commands: Commands::Evaluate { target },
        } => {
            printer.level = verbosity.into();
            if printer.level > printer::Level::Info {
                printer.level = printer::Level::Info;
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
            commands: Commands::Completions { shell },
        } => {
            let _verbosity = verbosity;
            clap_complete::generate(
                shell,
                &mut Arguments::command(),
                "spaces",
                &mut std::io::stdout(),
            );
        }

        Arguments {
            verbosity,
            commands: Commands::Docs { item },
        } => {
            printer.level = verbosity.into();

            docs::show(&mut printer, item)?;
        }

        Arguments {
            verbosity,
            commands: Commands::List {},
        } => {
            printer.level = verbosity.into();

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
        /// The path to the star file containing checkout rules.
        #[arg(long, value_hint = ValueHint::FilePath)]
        script: String,
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
