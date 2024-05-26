use clap::{Parser, Subcommand, ValueEnum};
use crate::config::Printer;


#[derive(ValueEnum, Clone, Debug)]
pub enum Level {
    Trace,
    Debug,
    Message,
    Info,
    Warning,
    Error
}

impl Into<printer::Level> for Level {
    fn into(self) -> printer::Level {
        match self {
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

fn update_printer(printer: &mut Printer, is_dry_run: bool, level: Option<Level>) {
    printer.is_dry_run = is_dry_run;
    if let Some(level) = level {
        printer.level = level.clone().into();
    }   
}

pub fn execute() -> anyhow::Result<()> {
    use crate::config::Config;
    let args = Arguments::parse();
    let config = Config::new()?;

    let mut printer = printer::Printer::new_stdout(config);
    use crate::workspace;
    match args {
        Arguments { commands: Commands::Create { name }, is_dry_run, level } => {
            update_printer(&mut printer, is_dry_run, level);
            workspace::create(&mut printer, &name)?;
        }
        Arguments { commands: Commands::Sync {}, is_dry_run, level } => {
            update_printer(&mut printer, is_dry_run, level);
            workspace::sync(&mut printer)?;
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









