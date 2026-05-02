use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn parse_timestamp(value: &str) -> Result<Arc<str>, String> {
    if value.is_empty() {
        return Err("timestamp cannot be empty".to_string());
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "invalid timestamp '{value}': only [A-Za-z0-9_-] characters are allowed"
        ));
    }
    Ok(value.into())
}

pub use crate::rcache::CacheStatus;
pub use crate::rule::Expect;
use crate::{logger, ws};

fn logs_logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "logs".into())
}

pub const LOG_STATUS_FILE_NAME: &str = "log_status.json";

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Status {
    pub name: Arc<str>,
    pub status: Expect,
    pub duration: std::time::Duration,
    pub file: Arc<str>,
    pub cache_status: CacheStatus,
}

pub struct RulesStatus {
    pub rules: Vec<Status>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum IsJson {
    No,
    Yes,
}

impl RulesStatus {
    pub fn load_from_json(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .context(format_context!("Failed to read {}", path.display()))?;
        let rules: Vec<Status> = serde_json::from_str(&content)
            .context(format_context!("Failed to parse {}", path.display()))?;
        Ok(Self { rules })
    }

    fn query_member(&self, name: &str, member: &str, json: IsJson) -> anyhow::Result<String> {
        let status = self
            .rules
            .iter()
            .find(|s| s.name.as_ref() == name)
            .ok_or_else(|| format_error!("Rule '{}' not found in log status", name))?;

        let serialize = |member: &str| -> anyhow::Result<String> {
            if json == IsJson::Yes {
                let value = match member {
                    "name" => serde_json::to_string(&status.name),
                    "status" => serde_json::to_string(&status.status),
                    "duration" => serde_json::to_string(&status.duration),
                    "file" => serde_json::to_string(&status.file),
                    "cache_status" => serde_json::to_string(&status.cache_status),
                    _ => {
                        return Err(format_error!(
                            "Unknown member '{}'. Expected one of: name, status, duration, file, cache_status",
                            member
                        ));
                    }
                };
                value.context(format_context!(
                    "Failed to serialize member '{}' for rule '{}'",
                    member,
                    name
                ))
            } else {
                let value = match member {
                    "name" => serde_yaml::to_string(&status.name),
                    "status" => serde_yaml::to_string(&status.status),
                    "duration" => serde_yaml::to_string(&status.duration),
                    "file" => serde_yaml::to_string(&status.file),
                    "cache_status" => serde_yaml::to_string(&status.cache_status),
                    _ => {
                        return Err(format_error!(
                            "Unknown member '{}'. Expected one of: name, status, duration, file, cache_status",
                            member
                        ));
                    }
                };
                value.context(format_context!(
                    "Failed to serialize member '{}' for rule '{}'",
                    member,
                    name
                ))
            }
        };

        serialize(member)
    }
}

#[derive(Debug, clap::Subcommand, Clone)]
pub enum LogsCommand {
    /// List all log folders that contain a log_status.json file, or list rule names for a specific timestamp.
    List {
        /// A timestamp to list rule names from (e.g. 20250308-20-05-28 or "latest")
        #[arg(long, value_parser = parse_timestamp)]
        timestamp: Option<Arc<str>>,
        /// Print output as JSON on a single line
        #[arg(long)]
        json: bool,
    },
    /// Query the status of a rule from the latest log.
    Query {
        /// The rule name to query (e.g. //:setup)
        rule_name: Arc<str>,
        /// A specific member to return: name, status, duration, file, cache_status
        #[arg(long)]
        member: Option<Arc<str>>,
        /// Print JSON output on a single line (no pretty printing)
        #[arg(long)]
        json: bool,
        /// Use a specific log timestamp instead of latest (e.g. 20250308-20-05-28)
        #[arg(long, value_parser = parse_timestamp)]
        timestamp: Option<Arc<str>>,
    },
}

/// Atomically update the `latest` symlink inside the logs directory to point
/// to `log_folder_name` (a bare folder name such as `logs_20250101-12-00-00-000`,
/// **not** a full path).
///
/// Uses a create-temp-then-rename approach so that:
/// - `latest` is never absent (no gap between delete and create)
/// - two concurrent processes both succeed (the last `rename` wins, and both
///   targets are valid log folders)
pub fn update_latest_symlink(log_folder_name: &str) -> anyhow::Result<()> {
    let latest_symlink = format!("{}/latest", ws::SPACES_LOGS_NAME);
    let latest_path = std::path::Path::new(latest_symlink.as_str());
    let temp_symlink = format!("{}/latest.tmp.{}", ws::SPACES_LOGS_NAME, std::process::id());
    let temp_path = std::path::Path::new(temp_symlink.as_str());
    // Remove any leftover temp symlink from a previous crash of this PID.
    let _ = symlink::remove_symlink_auto(temp_path);
    symlink::symlink_dir(log_folder_name, temp_path).context(format_context!(
        "Failed to create temp symlink in {}",
        ws::SPACES_LOGS_NAME
    ))?;
    // On Windows `rename` (MoveFileW without REPLACE_EXISTING) fails when the
    // destination already exists, so remove it first.  On Unix the rename(2)
    // syscall atomically replaces the destination, giving a zero-gap swap.
    #[cfg(windows)]
    let _ = symlink::remove_symlink_auto(latest_path);
    std::fs::rename(temp_path, latest_path).context(format_context!(
        "Failed to atomically update latest symlink in {}",
        ws::SPACES_LOGS_NAME
    ))?;
    Ok(())
}

pub fn execute(
    console: console::Console,
    workspace_path: &std::path::Path,
    command: LogsCommand,
) -> anyhow::Result<()> {
    match command {
        LogsCommand::List { timestamp, json } => {
            let logs_path = workspace_path.join(ws::SPACES_LOGS_NAME);
            if !logs_path.exists() {
                return Err(format_error!(
                    "No logs directory found at {}. Run a spaces command first.",
                    logs_path.display()
                ));
            }

            if let Some(ts) = &timestamp {
                let logs_dir = if ts.as_ref() == "latest" {
                    logs_path.join("latest")
                } else if ts.starts_with("logs_") {
                    logs_path.join(ts.as_ref())
                } else {
                    logs_path.join(format!("logs_{ts}"))
                };
                let status_path = logs_dir.join(LOG_STATUS_FILE_NAME);

                if !status_path.exists() {
                    return Err(format_error!(
                        "No log status file found at {}.",
                        status_path.display()
                    ));
                }

                let rules_status = RulesStatus::load_from_json(&status_path)
                    .context(format_context!("Failed to load log status"))?;

                let names: Vec<&str> = rules_status.rules.iter().map(|s| s.name.as_ref()).collect();

                if json {
                    let output = serde_json::to_string(&names)
                        .context(format_context!("Failed to serialize rule names"))?;
                    logs_logger(console.clone()).raw(&output);
                    logs_logger(console.clone()).raw("\n");
                } else {
                    let as_yaml = serde_yaml::to_string(&names).context(format_context!(
                        "Failed to serialize log folder names as YAML"
                    ))?;
                    logs_logger(console.clone()).raw(&as_yaml);
                }
            } else {
                let mut dirs: Vec<_> = std::fs::read_dir(&logs_path)
                    .context(format_context!(
                        "Failed to read logs directory {}",
                        logs_path.display()
                    ))?
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        name_str != "latest" && entry.path().join(LOG_STATUS_FILE_NAME).exists()
                    })
                    .collect();

                dirs.sort_by_key(|entry| entry.file_name());

                if json {
                    let names: Vec<String> = dirs
                        .iter()
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().into_owned();
                            name.strip_prefix("logs_").map(String::from).unwrap_or(name)
                        })
                        .collect();
                    let output = serde_json::to_string(&names)
                        .context(format_context!("Failed to serialize log folder names"))?;
                    logs_logger(console.clone()).raw(&output);
                    logs_logger(console.clone()).raw("\n");
                } else {
                    let entries: Vec<_> = dirs
                        .iter()
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            name.strip_prefix("logs_").map(String::from).unwrap_or(name)
                        })
                        .collect();
                    let as_yaml = serde_yaml::to_string(&entries).context(format_context!(
                        "Failed to serialize log folder names as YAML"
                    ))?;
                    logs_logger(console.clone()).raw(&as_yaml);
                }
            }

            Ok(())
        }
        LogsCommand::Query {
            rule_name,
            member,
            json,
            timestamp,
        } => {
            let logs_dir = if let Some(ts) = &timestamp {
                if ts.starts_with("logs_") {
                    workspace_path.join(ws::SPACES_LOGS_NAME).join(ts.as_ref())
                } else {
                    workspace_path
                        .join(ws::SPACES_LOGS_NAME)
                        .join(format!("logs_{ts}"))
                }
            } else {
                workspace_path.join(ws::SPACES_LOGS_NAME).join("latest")
            };
            let latest_path = logs_dir.join(LOG_STATUS_FILE_NAME);

            if !latest_path.exists() {
                return Err(format_error!(
                    "No log status file found at {}. Run a spaces command first.",
                    latest_path.display()
                ));
            }

            let rules_status = RulesStatus::load_from_json(&latest_path)
                .context(format_context!("Failed to load log status"))?;

            let is_json = if json { IsJson::Yes } else { IsJson::No };

            if let Some(member) = member {
                let value = rules_status
                    .query_member(&rule_name, &member, is_json)
                    .context(format_context!("Failed to query member"))?;
                logs_logger(console.clone()).raw(&value);
                if is_json == IsJson::Yes {
                    logs_logger(console.clone()).raw("\n");
                }
            } else {
                let status = rules_status
                    .rules
                    .iter()
                    .find(|s| s.name.as_ref() == rule_name.as_ref())
                    .ok_or_else(|| format_error!("Rule '{}' not found in log status", rule_name))?;

                if json {
                    let output = serde_json::to_string(status).context(format_context!(
                        "Failed to serialize status for rule '{}'",
                        rule_name
                    ))?;
                    logs_logger(console.clone()).raw(&output);
                    logs_logger(console.clone()).raw("\n");
                } else {
                    let output = serde_yaml::to_string(status).context(format_context!(
                        "Failed to serialize status for rule '{}'",
                        rule_name
                    ))?;
                    logs_logger(console.clone()).raw(&output);
                };
            }

            Ok(())
        }
    }
}
