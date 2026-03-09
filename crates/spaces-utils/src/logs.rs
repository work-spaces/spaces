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

impl RulesStatus {
    pub fn load_from_json(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .context(format_context!("Failed to read {}", path.display()))?;
        let rules: Vec<Status> = serde_json::from_str(&content)
            .context(format_context!("Failed to parse {}", path.display()))?;
        Ok(Self { rules })
    }

    pub fn query_member(&self, name: &str, member: &str) -> anyhow::Result<String> {
        let status = self
            .rules
            .iter()
            .find(|s| s.name.as_ref() == name)
            .ok_or_else(|| format_error!("Rule '{}' not found in log status", name))?;

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
    }
}

#[derive(Debug, clap::Subcommand, Clone)]
pub enum LogsCommand {
    /// List all log folders that contain a log_status.json file.
    List {},
    /// Query the status of a rule from the latest log.
    Query {
        /// The rule name to query (e.g. //:setup)
        rule_name: String,
        /// A specific member to return: name, status, duration, file, cache_status
        #[clap(long)]
        member: Option<String>,
        /// Print JSON output on a single line (no pretty printing)
        #[clap(long)]
        json: bool,
        /// Use a specific log timestamp instead of latest (e.g. 20250308-20-05-28)
        #[clap(long)]
        timestamp: Option<String>,
    },
}

pub fn execute(
    printer: &mut printer::Printer,
    workspace_path: &std::path::Path,
    command: LogsCommand,
) -> anyhow::Result<()> {
    match command {
        LogsCommand::List {} => {
            let logs_path = workspace_path.join(ws::SPACES_LOGS_NAME);
            if !logs_path.exists() {
                return Err(format_error!(
                    "No logs directory found at {}. Run a spaces command first.",
                    logs_path.display()
                ));
            }

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

            for entry in dirs {
                logs_logger(printer).raw(entry.file_name().to_string_lossy().as_ref());
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

            if let Some(member) = member {
                let value = rules_status
                    .query_member(&rule_name, &member)
                    .context(format_context!("Failed to query member"))?;
                logs_logger(printer).info(&value);
            } else {
                let status = rules_status
                    .rules
                    .iter()
                    .find(|s| s.name.as_ref() == rule_name.as_str())
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
