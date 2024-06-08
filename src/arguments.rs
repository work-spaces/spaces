use clap::{Parser, Subcommand, ValueEnum};


#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Level {
    Trace,
    Debug,
    Message,
    Info,
    Warning,
    Error
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
    #[arg(long)]
    level: Option<Level>,
}

pub fn execute() -> anyhow::Result<()> {
    use crate::{context::Context, workspace, ledger};
    let args = Arguments::parse();
    let mut context = Context::new()?;

    match args {
        Arguments { commands: Commands::Create { name, config }, level } => {
            context.update_printer(level.map(|e| e.into()));
            workspace::create(context, &name, &config)?;
        }
        Arguments { commands: Commands::Sync {}, level } => {
            context.update_printer(level.map(|e| e.into()));
            workspace::sync(context)?;
        }

        Arguments { commands: Commands::List {}, level } => {
            context.update_printer(level.map(|e| e.into()));
            let arc_context = std::sync::Arc::new(context);
            let ledger = ledger::Ledger::new(arc_context.clone())?;
            ledger.show_status(arc_context)?;
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
    /// Creates a new workspace
    Create {
        /// The name of the workspace
        #[arg(long)]
        name: String,
        #[arg(long)]
        config: String,
    },
    /// Synchronize the current workspace. This is useful if you modify the workspace after creating it.
    Sync {},
    /// Lists the workspaces in the spaces store on the local machine.
    List {}
}









