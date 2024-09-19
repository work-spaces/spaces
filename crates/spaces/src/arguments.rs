use crate::{evaluator, info, rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use clap::{Parser, Subcommand, ValueEnum};

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
    #[command(subcommand)]
    commands: Commands,
}

pub fn execute() -> anyhow::Result<()> {
    use crate::ledger;
    let args = Arguments::parse();

    let mut printer = printer::Printer::new_stdout();

    match args {
        Arguments {
            commands: Commands::Checkout { name, script },
        } => {
            std::fs::create_dir_all(name.as_str())
                .context(format_context!("while creating workspace directory {name}"))?;

            let current_working_directory = std::env::current_dir()
                .context(format_context!("while getting current working directory"))?
                .to_string_lossy()
                .to_string();

            info::set_workspace_path(format!("{current_working_directory}/{name}"))
                .context(format_context!("while setting workspace path"))?;

            evaluator::run_starlark_file(
                &mut printer,
                script.as_str(),
                rules::Phase::Checkout,
                None,
            )
            .context(format_context!("while executing checkout rules"))?;
        }

        Arguments {
            commands: Commands::Sync {},
        } => {
            if !std::path::Path::new(workspace::WORKSPACE_FILE_NAME).exists() {
                return Err(anyhow::anyhow!(
                    "No workspace file found. Run checkout first."
                ));
            }

            let current_working_directory = std::env::current_dir()
                .context(format_context!("while getting current working directory"))?
                .to_string_lossy()
                .to_string();

            info::set_workspace_path(current_working_directory)
                .context(format_context!("while setting workspace path"))?;

            evaluator::run_starlark_workspace(&mut printer, rules::Phase::Checkout, None)
                .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            commands: Commands::Run { target },
        } => {
            evaluator::run_starlark_workspace(&mut printer, rules::Phase::Run, target)
                .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            commands: Commands::Evaluate { target },
        } => {
            evaluator::run_starlark_workspace(&mut printer, rules::Phase::Evaluate, target)
                .context(format_context!("while executing run rules"))?;
        }

        Arguments {
            commands: Commands::List {},
        } => {
            let ledger =
                ledger::Ledger::new().with_context(|| format_context!("while creating ledger"))?;
            ledger.show_status()?;
        }
    }

    Ok(())
}

/*

TODO

Add a sync option to checkout all deps on the branch rather than the rev. This can help testing tip of branch before
updating the dep rev. Should only apply to deps that are part of development repositories. They are the only
ones that can be updated.

Add a command to get tip of tree commit hashes for the deps of the development repositories. This can be used to
update the spaces_deps.toml file.

Add a way to format spaces_deps.toml. This opens the door for auto updating spaces_deps.toml.

*/

#[derive(Debug, Subcommand)]
enum Commands {
    /// Executes the Checkout phase rules for the script and its dependencies.
    Checkout {
        /// The name of the workspace
        #[arg(long)]
        name: String,
        /// The path to the star file containing checkout rules.
        #[arg(long)]
        script: String,
    },
    /// Synchronizes the workspace with the checkout rules.
    Sync {},
    /// Executes the Run phase rules.
    Run {
        /// The path to the star file containing checkout rules.
        #[arg(long)]
        target: Option<String>,
    },
    Evaluate {
        /// The path to the star file containing checkout rules.
        #[arg(long)]
        target: Option<String>,
    },
    /// Lists the workspaces in the spaces store on the local machine.
    List {},
}
