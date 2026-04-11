use crate::sandbox;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub enum IsSandbox {
    No,
    Yes,
}

impl From<IsSandbox> for bool {
    fn from(value: IsSandbox) -> Self {
        matches!(value, IsSandbox::Yes)
    }
}

impl From<bool> for IsSandbox {
    fn from(value: bool) -> Self {
        if value { IsSandbox::Yes } else { IsSandbox::No }
    }
}

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
pub struct ShortcutEntry {
    pub command: Arc<str>,
    pub help: Arc<str>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShortcutValue {
    Simple(Arc<str>),
    Detailed(ShortcutEntry),
}

impl ShortcutValue {
    pub fn command(&self) -> &Arc<str> {
        match self {
            ShortcutValue::Simple(cmd) => cmd,
            ShortcutValue::Detailed(entry) => &entry.command,
        }
    }

    pub fn help(&self) -> Option<&Arc<str>> {
        match self {
            ShortcutValue::Simple(_) => None,
            ShortcutValue::Detailed(entry) => Some(&entry.help),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub path: Arc<str>,
    pub startup: Option<Startup>,
    pub args: Vec<Arc<str>>,
    pub shortcuts: Option<HashMap<Arc<str>, ShortcutValue>>,
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

    pub fn to_markdown(&self) -> String {
        use crate::markdown;

        let mut md = String::new();

        md.push_str(&markdown::heading(1, "Spaces Shell"));
        md.push_str(&markdown::paragraph(
            &format!("Run {} from anywhere in the workspace to launch an interactive shell with the workspace environment. Get more info with {}",
            markdown::code("spaces shell"),
            markdown::code("spaces shell --help")
        )));

        md.push_str(&markdown::heading(2, "Configuration"));

        md.push_str(&markdown::heading(3, "Shell Path"));
        md.push_str(&markdown::code_block("", &self.path));
        md.push('\n');

        if !self.args.is_empty() {
            md.push_str(&markdown::heading(3, "Shell Arguments"));
            let items: Vec<Arc<str>> = self.args.iter().map(|a| markdown::code(a).into()).collect();
            md.push_str(&markdown::list(items));
        }

        if let Some(startup) = &self.startup {
            md.push_str(&markdown::heading(3, "Startup Script"));
            md.push_str(&markdown::paragraph(&format!(
                "File: {}",
                markdown::code(&startup.name)
            )));
            if let Some(env_name) = &startup.env_name {
                md.push_str(&markdown::paragraph(&format!(
                    "Environment variable: {}",
                    markdown::code(env_name)
                )));
            }
            md.push_str(&markdown::code_block("sh", &startup.contents));
            md.push('\n');
        }

        if let Some(shortcuts) = &self.shortcuts {
            md.push_str(&markdown::heading(2, "Shortcuts"));
            md.push_str(&markdown::paragraph(
                "The following shortcuts are available as shell functions in the spaces shell.",
            ));

            let mut keys: Vec<&Arc<str>> = shortcuts.keys().collect();
            keys.sort();

            for key in keys {
                let value = &shortcuts[key];
                let command = value.command();

                md.push_str(markdown::hline());
                md.push_str(&markdown::heading(3, key));
                md.push_str(&markdown::code_block("sh", key));

                if let Some(help) = value.help() {
                    md.push_str(&markdown::paragraph(help));
                }

                md.push_str(&markdown::code_block("sh", command));
                md.push('\n');
            }
        }

        md
    }

    pub fn get_shell(&self) -> anyhow::Result<clap_complete::Shell> {
        let program_name = std::path::Path::new(self.path.as_ref())
            .file_name()
            .ok_or(format_error!("Failed to get Shell from {}", self.path))?;
        match program_name.to_str() {
            Some("bash") => Ok(clap_complete::Shell::Bash),
            Some("zsh") => Ok(clap_complete::Shell::Zsh),
            Some("fish") => Ok(clap_complete::Shell::Fish),
            Some("pwsh") => Ok(clap_complete::Shell::PowerShell),
            _ => Err(format_error!(
                "Unsupported shell: {}",
                program_name.display()
            )),
        }
    }
}

fn create_shortcuts(
    path: Arc<str>,
    shortcuts: &HashMap<Arc<str>, ShortcutValue>,
) -> anyhow::Result<Vec<Arc<str>>> {
    let shell_type = ShellType::try_from(path.clone())
        .context(format_context!("while decoding shell type from {}", path))?;

    let mut functions = Vec::new();

    for (key, value) in shortcuts {
        let command = value.command();
        let function = match shell_type {
            ShellType::Bash | ShellType::Zsh => {
                format!("{key}() {{\n\t{command}\n}}")
            }
            ShellType::Fish => format!("function {key}\n\t{command}\nend"),
        };
        functions.push(function.into());
    }

    Ok(functions)
}

pub fn create_sandbox(
    env_path: Arc<str>,
    store_path: Arc<str>,
) -> anyhow::Result<sandbox::Sandbox> {
    let cwd = std::env::current_dir()
        .context(format_context!("Failed to get current working directory"))?;
    let cwd_str: Arc<str> = cwd
        .to_str()
        .ok_or_else(|| format_error!("Current working directory path is not valid UTF-8"))?
        .into();

    let mut manifest = sandbox::Sandbox::new()
        .with_name("spaces-shell")
        .allow_write(cwd_str.clone())
        .allow_exec(cwd_str)
        .allow_read(store_path.clone())
        .allow_write(store_path.clone())
        .allow_exec(store_path)
        .with_network(sandbox::NetworkPolicy::Unrestricted);

    for path in env_path.split(':') {
        let path = path.trim();
        if !path.is_empty() {
            manifest = manifest.allow_exec(path);
        }
    }

    Ok(manifest)
}

pub fn run(
    config: &Config,
    environment_map: &std::collections::HashMap<Arc<str>, Arc<str>>,
    startup_directory: &std::path::Path,
    completion_content: Vec<u8>,
    working_directory: &std::path::Path,
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

        let completion_content_str = String::from_utf8(completion_content).context(
            format_context!("Failed to convert completion content to string"),
        )?;

        contents.push_str(completion_content_str.as_str());
        contents.push_str("\n\n");

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
    process.current_dir(working_directory);

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
