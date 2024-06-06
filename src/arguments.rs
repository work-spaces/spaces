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
    Sync {},
    List {}
}









