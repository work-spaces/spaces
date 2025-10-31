use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
}

impl Config {
    pub fn load(
        path: Option<Arc<str>>,
        default_shell_path: Option<Arc<str>>,
    ) -> anyhow::Result<Self> {
        if let Some(path) = path {
            let contents = std::fs::read_to_string(path.as_ref()).context(format_context!(
                "Failed to read shell configuration file from {}",
                path
            ))?;

            let shell = toml::from_str(&contents).context(format_context!(
                "Failed to parse toml shell configuration from {}",
                path
            ))?;

            Ok(shell)
        } else {
            Ok(Self {
                path: default_shell_path.unwrap_or("/bin/bash".into()),
                startup: None,
                args: Vec::new(),
            })
        }
    }
}

pub fn run(
    config: &Config,
    environment_map: &std::collections::HashMap<Arc<str>, Arc<str>>,
    startup_directory: &std::path::Path,
) -> anyhow::Result<()> {
    // Create the command
    let mut process = std::process::Command::new(config.path.as_ref());
    process.env_clear();

    if let Some(startup) = config.startup.as_ref() {
        let config_file = startup_directory.join(startup.name.as_ref());

        std::fs::write(&config_file, startup.contents.as_ref()).context(format_context!(
            "Failed to write shell startupconfiguration file to {}",
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
