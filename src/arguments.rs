use crate::{git, manifest::{self, WorkspaceConfig}};
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


pub fn execute() -> anyhow::Result<()> {
    use crate::{archive, context::Context, ledger, workspace};
    let args = Arguments::parse();
    let mut context = Context::new()?;

    match args {
        Arguments {
            commands: Commands::Create { name, config },
            level,
        } => {
            context.update_printer(level.map(|e| e.into()));
            context.spaces_sysroot = Some(format!("{}/{}/sysroot", context.current_directory, name));
            workspace::create(context, &name, &config)?;
        }

        Arguments{
            commands: Commands::CreateWorktree { name, git, branch, dev_branch },
            level,
        } => {
            context.update_printer(level.map(|e| e.into()));
            context.spaces_sysroot = Some(format!("{}/{}/sysroot", context.current_directory, name));

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

            workspace::create_from_config(context, &name, config)?;

        }

        Arguments {
            commands: Commands::Sync {},
            level,
        } => {
            context.update_printer(level.map(|e| e.into()));
            context.spaces_sysroot = Some(format!("{}/sysroot", context.current_directory));
            workspace::sync(context)?;
        }

        Arguments {
            commands: Commands::List {},
            level,
        } => {
            context.update_printer(level.map(|e| e.into()));
            let arc_context = std::sync::Arc::new(context);
            let ledger = ledger::Ledger::new(arc_context.clone())?;
            ledger.show_status(arc_context)?;
        }
        Arguments {
            commands:
                Commands::CreateArchive {
                    name,
                    path,
                    macos_aarch64,
                    macos_x86_64,
                    windows_aarch64,
                    windows_x86_64,
                    linux_aarch64,
                    linux_x86_64,
                },
            level,
        } => {
            context.update_printer(level.map(|e| e.into()));
            let executable_paths = archive::PlatformPaths {
                macos_x86_64,
                macos_aarch64,
                windows_x86_64,
                windows_aarch64,
                linux_x86_64,
                linux_aarch64,
            };
            archive::create(context, name, path, executable_paths)?;
        }
        Arguments {
            commands: Commands::InspectArchive { path },
            level,
        } => {
            context.update_printer(level.map(|e| e.into()));
            archive::inspect(context, path)?;
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

impl From<Platform> for manifest::Platform {
    fn from(platform: Platform) -> manifest::Platform {
        match platform {
            Platform::MacosX86_64 => manifest::Platform::MacosX86_64,
            Platform::MacosAarch64 => manifest::Platform::MacosAarch64,
            Platform::WindowsX86_64 => manifest::Platform::WindowsX86_64,
            Platform::WindowsAarch64 => manifest::Platform::WindowsAarch64,
            Platform::LinuxX86_64 => manifest::Platform::LinuxX86_64,
            Platform::LinuxAarch64 => manifest::Platform::LinuxAarch64,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Creates a new workspace
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
    /// Synchronize the current workspace. This is useful if you modify the workspace after creating it.
    Sync {},
    /// Lists the workspaces in the spaces store on the local machine.
    List {},
    CreateArchive {
        /// The name of the archive to create (without .zip)
        #[arg(long)]
        name: String,
        /// The path to the files to compress
        #[arg(long)]
        path: String,
        /// Path to macos_x86_64 executables
        #[arg(long)]
        macos_x86_64: Option<String>,
        /// Path to macos_aarch64 executables
        #[arg(long)]
        macos_aarch64: Option<String>,
        /// Path to windows_x86_64 executables
        #[arg(long)]
        windows_x86_64: Option<String>,
        /// Path to windows_aarch64 executables
        #[arg(long)]
        windows_aarch64: Option<String>,
        /// Path to linux_x86_64 executables
        #[arg(long)]
        linux_x86_64: Option<String>,
        /// Path to linux_aarch64 executables
        #[arg(long)]
        linux_aarch64: Option<String>,
    },
    InspectArchive {
        /// The path of the .zip archive to inspect
        #[arg(long)]
        path: String,
    },
}
