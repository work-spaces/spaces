use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub const IS_SPACES_SHELL_ENV_NAME: &str = "SPACES_IS_SPACES_SHELL";
pub const IS_SPACES_SHELL_ENV_VALUE: &str = "SPACES_IS_RUNNING_IN_A_SPACES_SHELL";
const SHORTCUTS_SCRIPTS_NAME: &str = "shortcuts.sh";

enum ShellType {
    Bash,
    Zsh,
    Fish,
}

impl TryFrom<Arc<str>> for ShellType {
    type Error = anyhow::Error;

    fn try_from(shell: Arc<str>) -> Result<Self, Self::Error> {
        match shell.as_ref() {
            "bash" => Ok(ShellType::Bash),
            "zsh" => Ok(ShellType::Zsh),
            "fish" => Ok(ShellType::Fish),
            _ => {
                if shell.ends_with("bash") {
                    Ok(ShellType::Bash)
                } else if shell.ends_with("zsh") {
                    Ok(ShellType::Zsh)
                } else if shell.ends_with("fish") {
                    Ok(ShellType::Fish)
                } else {
                    Err(format_error!(
                        "Unsupported shell type: {} - use path to: bash, zsh, fish",
                        shell
                    ))
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Startup {
    pub env_name: Option<Arc<str>>,
    pub name: Arc<str>,
    pub contents: Arc<str>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub path: Arc<str>,
    pub startup: Option<Startup>,
    pub args: Vec<Arc<str>>,
    pub shortcuts: Option<HashMap<Arc<str>, Arc<str>>>,
}

impl Config {
    pub fn load(
        config_path_path: Option<Arc<str>>,
        default_shell_path: Option<Arc<str>>,
    ) -> anyhow::Result<Self> {
        if let Some(config_path_path) = config_path_path {
            let contents =
                std::fs::read_to_string(config_path_path.as_ref()).context(format_context!(
                    "Failed to read shell configuration file from {}",
                    config_path_path
                ))?;

            let shell = toml::from_str(&contents).context(format_context!(
                "Failed to parse toml shell configuration from {}",
                config_path_path
            ))?;

            Ok(shell)
        } else {
            Ok(Self {
                path: default_shell_path.unwrap_or("/bin/bash".into()),
                startup: None,
                args: Vec::new(),
                shortcuts: None,
            })
        }
    }
}

fn create_shortcuts(
    path: Arc<str>,
    shortcuts: &HashMap<Arc<str>, Arc<str>>,
) -> anyhow::Result<Vec<Arc<str>>> {
    let shell_type = ShellType::try_from(path.clone())
        .context(format_context!("while decoding shell type from {}", path))?;

    let mut functions = Vec::new();

    for (key, value) in shortcuts {
        let function = match shell_type {
            ShellType::Bash | ShellType::Zsh => format!("{key}() {{\n\t{value}\n}}"),
            ShellType::Fish => format!("function {key}\n\t{value}\nend"),
        };
        functions.push(function.into());
    }

    Ok(functions)
}

pub fn run(
    config: &Config,
    environment_map: &std::collections::HashMap<Arc<str>, Arc<str>>,
    startup_directory: &std::path::Path,
) -> anyhow::Result<()> {
    // Create the command
    let mut process = std::process::Command::new(config.path.as_ref());
    process.env_clear();

    if let Some(shortcuts) = config.shortcuts.as_ref() {
        let shortcuts = create_shortcuts(config.path.clone(), shortcuts)
            .context(format_context!("While creating shortcuts"))?;
        let mut contents = shortcuts.join("\n\n");
        contents.push_str("\n\n");
        let shortcuts_file = startup_directory.join(SHORTCUTS_SCRIPTS_NAME);
        std::fs::write(&shortcuts_file, contents).context(format_context!(
            "Failed to write shell shortcuts file to {}",
            shortcuts_file.display()
        ))?;
    }

    if let Some(startup) = config.startup.as_ref() {
        let config_file = startup_directory.join(startup.name.as_ref());

        std::fs::write(&config_file, startup.contents.as_ref()).context(format_context!(
            "Failed to write shell startup configuration file to {}",
            config_file.display()
        ))?;

        // Set startup env directory
        if let Some(env_name) = startup.env_name.as_ref() {
            process.env(env_name.as_ref(), startup_directory);
        }
    }

    // Set custom environment variables
    for (key, value) in environment_map {
        process.env(key.as_ref(), value.as_ref());
    }

    process.env(IS_SPACES_SHELL_ENV_NAME, IS_SPACES_SHELL_ENV_VALUE);

    for arg in config.args.iter() {
        process.arg(arg.as_ref());
    }

    // Make it interactive
    process
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    process
        .status()
        .context(format_context!("failed to launch shell: {}", config.path))?;

    Ok(())
}

pub fn is_spaces_shell() -> bool {
    std::env::var(IS_SPACES_SHELL_ENV_NAME).is_ok_and(|value| value == IS_SPACES_SHELL_ENV_VALUE)
}
