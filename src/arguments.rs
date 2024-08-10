use crate::{
    context, git, platform,
    manifest::{self, WorkspaceConfig},
};

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
    #[arg(long)]
    level: Option<Level>,
}

fn update_execution_context(
    execution_context: &mut context::ExecutionContext,
    space_name: Option<&String>,
    level: Option<Level>,
) {
    if let Some(level) = level {
        execution_context.printer.level = level.into();
    }

    execution_context.context.template_model.spaces.sysroot = if let Some(name) = space_name {
        // for create
        format!(
            "{}/{name}/sysroot",
            execution_context.context.current_directory
        )
    } else {
        // for sync
        format!("{}/sysroot", execution_context.context.current_directory)
    };
}

pub fn execute() -> anyhow::Result<()> {
    use crate::{archive, ledger, workspace};
    let args = Arguments::parse();
    let mut execution_context = context::ExecutionContext::new()?;

    match args {
        Arguments {
            commands: Commands::Create { name, config },
            level,
        } => {
            update_execution_context(&mut execution_context, Some(&name), level);
            workspace::create(execution_context, &name, &config)?;
        }

        Arguments {
            commands:
                Commands::CreateWorktree {
                    name,
                    git,
                    branch,
                    dev_branch,
                },
            level,
        } => {
            update_execution_context(&mut execution_context, Some(&name), level);
            let hash_key = git::BareRepository::get_workspace_name_from_url(&git)?;

            let config = WorkspaceConfig {
                repositories: maplit::hashmap! {
                    hash_key.clone() => manifest::Dependency {
                        git,
                        branch: Some(branch),
                        dev: dev_branch,
                        ..Default::default()
                    }
                },
                ..Default::default()
            };
            workspace::create_from_config(execution_context, &name, config)?;
        }

        Arguments {
            commands: Commands::Sync {},
            level,
        } => {
            update_execution_context(&mut execution_context, None, level);
            workspace::sync(execution_context)?;
        }

        Arguments {
            commands: Commands::List {},
            level,
        } => {
            update_execution_context(&mut execution_context, None, level);
            let arc_context = std::sync::Arc::new(execution_context.context);
            let ledger = ledger::Ledger::new(arc_context.clone())
                .with_context(|| format_context!("while creating ledger"))?;
            ledger.show_status(arc_context)?;
        }

        Arguments {
            commands: Commands::CreateArchive { manifest },
            level,
        } => {
            update_execution_context(&mut execution_context, None, level);
            let manifest_path = manifest.unwrap_or("spaces_create_archive.toml".to_string());
            archive::create(execution_context, manifest_path)?;
        }
        Arguments {
            commands: Commands::TemplateHelp {},
            level,
        } => {
            update_execution_context(&mut execution_context, None, level);
            let mut printer = execution_context.printer;
            printer.info("substitutions", &execution_context.context.template_model)?;
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

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum Platform {
    MacosX86_64,
    MacosAarch64,
    WindowsX86_64,
    WindowsAarch64,
    LinuxX86_64,
    LinuxAarch64,
}

impl From<Platform> for platform::Platform {
    fn from(platform: Platform) -> platform::Platform {
        match platform {
            Platform::MacosX86_64 => platform::Platform::MacosX86_64,
            Platform::MacosAarch64 => platform::Platform::MacosAarch64,
            Platform::WindowsX86_64 => platform::Platform::WindowsX86_64,
            Platform::WindowsAarch64 => platform::Platform::WindowsAarch64,
            Platform::LinuxX86_64 => platform::Platform::LinuxX86_64,
            Platform::LinuxAarch64 => platform::Platform::LinuxAarch64,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Creates a new workspace using a workspace configuration file.
    Create {
        /// The name of the workspace
        #[arg(long)]
        name: String,
        /// The path to the configuration file
        #[arg(long)]
        config: String,
    },
    /// Creates a workspace using a single git repository plus its deps.
    CreateWorktree {
        /// The name of the workspace
        #[arg(long)]
        name: String,
        /// The URL to the git repository
        #[arg(long)]
        git: String,
        /// The base branch
        #[arg(long)]
        branch: String,
        /// The development branch `user/{USER}/{SPACE}-{UNIQUE}` is the default value
        #[arg(long)]
        dev_branch: Option<String>,
    },
    /// Synchronizes the current workspace.
    Sync {},
    /// Lists the workspaces in the spaces store on the local machine.
    List {},
    /// Creates an archive using a spaces create archive manifest.
    CreateArchive {
        /// spaces_create_archive.toml is the default
        #[arg(long)]
        manifest: Option<String>,
    },
    /// Show the list of substitions made when copying `Template` assets to a space
    TemplateHelp {},
}
