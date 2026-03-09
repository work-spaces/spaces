use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub use crate::rcache::CacheStatus;
pub use crate::rule::Expect;
use crate::{logger, ws};

fn logs_logger(printer: &mut printer::Printer) -> logger::Logger<'_> {
    logger::Logger::new_printer(printer, "logs".into())
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
        #[arg(long)]
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
        #[arg(long)]
        timestamp: Option<Arc<str>>,
    },
}

pub fn execute(
    printer: &mut printer::Printer,
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
                    logs_logger(printer).raw(&output);
                    logs_logger(printer).raw("\n");
                } else {
                    let as_yaml = serde_yaml::to_string(&names).context(format_context!(
                        "Failed to serialize log folder names as YAML"
                    ))?;
                    logs_logger(printer).raw(&as_yaml);
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
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect();
                    let output = serde_json::to_string(&names)
                        .context(format_context!("Failed to serialize log folder names"))?;
                    logs_logger(printer).raw(&output);
                    logs_logger(printer).raw("\n");
                } else {
                    let entries: Vec<_> = dirs
                        .iter()
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .collect();
                    let as_yaml = serde_yaml::to_string(&entries).context(format_context!(
                        "Failed to serialize log folder names as YAML"
                    ))?;
                    logs_logger(printer).raw(&as_yaml);
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
                workspace_path
                    .join(ws::SPACES_LOGS_NAME)
                    .join(format!("logs_{ts}"))
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
                logs_logger(printer).raw(&value);
                logs_logger(printer).raw("\n");
            } else {
                let status = rules_status
                    .rules
                    .iter()
                    .find(|s| s.name.as_ref() == rule_name.as_ref())
                    .ok_or_else(|| format_error!("Rule '{}' not found in log status", rule_name))?;

                let output = if json {
                    serde_json::to_string(status).context(format_context!(
                        "Failed to serialize status for rule '{}'",
                        rule_name
                    ))?
                } else {
                    serde_yaml::to_string(status).context(format_context!(
                        "Failed to serialize status for rule '{}'",
                        rule_name
                    ))?
                };
                logs_logger(printer).raw(&output);
                logs_logger(printer).raw("\n");
            }

            Ok(())
        }
    }
}
