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
    is_dry_run: bool,
    #[arg(long)]
    level: Option<Level>,
}

pub fn execute() -> anyhow::Result<()> {
    use crate::{context::Context, workspace};
    let args = Arguments::parse();
    let mut context = Context::new()?;

    match args {
        Arguments { commands: Commands::Create { name }, is_dry_run, level } => {
            context.update_printer(is_dry_run, level.map(|e| e.into()));
            workspace::create(context, &name)?;
        }
        Arguments { commands: Commands::Sync {}, is_dry_run, level } => {
            context.update_printer(is_dry_run, level.map(|e| e.into()));
            workspace::sync(context)?;
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
    },
    Sync {}
}









