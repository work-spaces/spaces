//! Reading and viewing the metrics written by `spaces` (see `telemetry.rs` in the
//! `spaces` crate).
//!
//! Each sink is a JSONL file with one [`Record`] per line:
//! - `spaces.perf.*` -> `<workspace>/.spaces/metrics/spaces.perf.jsonl`
//! - `spaces.usage.*` -> `<store>/metrics/spaces.usage.jsonl`
//!
//! The schema lives here so that the writer and the viewer (`spaces metrics`) share it.
//! This module is named `metrics_log` (not `metrics`) so it doesn't shadow the `metrics`
//! crate used for recording.

mod command;
mod prune;
mod query;
mod stats;

pub use command::{
    MetricsCommand, MetricsContext, MetricsQueryArgs, PruneSource, StatusFilter, execute,
};
pub use prune::{PruneReport, prune_file};
pub use query::{Filter, LabelFilter, PSEUDO_KEYS, Sample, Selection, When, group_key, select};
pub use stats::{BinCount, Bucket, Scale, Stat, Stats, make_bins, percentile, trend};

use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use crate::{store, ws};

pub const PERF_PREFIX: &str = "spaces.perf.";
pub const USAGE_PREFIX: &str = "spaces.usage.";

pub const PERF_COMMAND_DURATION: &str = "spaces.perf.command.duration_seconds";
pub const PERF_RULE_DURATION: &str = "spaces.perf.rule.duration_seconds";
pub const PERF_RULE_HASH_INPUTS_DURATION: &str = "spaces.perf.rule.hash_inputs.duration_seconds";
pub const PERF_EVALUATE_DURATION: &str = "spaces.perf.evaluate.duration_seconds";
pub const PERF_STEP_DURATION: &str = "spaces.perf.step.duration_seconds";
pub const PERF_REPO_STEP_DURATION: &str = "spaces.perf.repo_step.duration_seconds";
pub const USAGE_COMMAND: &str = "spaces.usage.command";
pub const USAGE_TARGET: &str = "spaces.usage.target";
pub const USAGE_RULE: &str = "spaces.usage.rule";
pub const USAGE_SEARCH: &str = "spaces.usage.search";

const SPACES_PREFIX: &str = "spaces.";
const DURATION_SUFFIX: &str = ".duration_seconds";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CounterRecord {
    pub name: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistogramRecord {
    pub name: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub samples: Vec<f64>,
}

/// One line of a metrics file: everything one command recorded for one sink.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub timestamp_ms: u64,
    pub spaces_version: String,
    pub command: String,
    pub status: String,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub counters: Vec<CounterRecord>,
    #[serde(default)]
    pub histograms: Vec<HistogramRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum MetricKind {
    Counter,
    Histogram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Source {
    Perf,
    Usage,
}

/// Static description of a metric that spaces records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricInfo {
    pub name: &'static str,
    pub kind: MetricKind,
    pub labels: &'static [&'static str],
    pub default_group_by: &'static str,
}

pub const KNOWN_METRICS: &[MetricInfo] = &[
    MetricInfo {
        name: PERF_COMMAND_DURATION,
        kind: MetricKind::Histogram,
        labels: &["command"],
        default_group_by: "command",
    },
    MetricInfo {
        name: PERF_RULE_DURATION,
        kind: MetricKind::Histogram,
        labels: &["rule", "cache"],
        default_group_by: "rule",
    },
    MetricInfo {
        name: PERF_RULE_HASH_INPUTS_DURATION,
        kind: MetricKind::Histogram,
        labels: &["rule"],
        default_group_by: "rule",
    },
    MetricInfo {
        name: PERF_EVALUATE_DURATION,
        kind: MetricKind::Histogram,
        labels: &["phase"],
        default_group_by: "phase",
    },
    MetricInfo {
        name: PERF_STEP_DURATION,
        kind: MetricKind::Histogram,
        labels: &["step"],
        default_group_by: "step",
    },
    MetricInfo {
        name: PERF_REPO_STEP_DURATION,
        kind: MetricKind::Histogram,
        labels: &["step", "repo"],
        default_group_by: "repo",
    },
    MetricInfo {
        name: USAGE_COMMAND,
        kind: MetricKind::Counter,
        labels: &["command", "status", "ci"],
        default_group_by: "command",
    },
    MetricInfo {
        name: USAGE_TARGET,
        kind: MetricKind::Counter,
        labels: &["command", "target"],
        default_group_by: "target",
    },
    MetricInfo {
        name: USAGE_RULE,
        kind: MetricKind::Counter,
        labels: &["rule", "cache"],
        default_group_by: "rule",
    },
    MetricInfo {
        name: USAGE_SEARCH,
        kind: MetricKind::Counter,
        labels: &["scope", "keyword"],
        default_group_by: "keyword",
    },
];

/// Removes the `spaces.` prefix and the `.duration_seconds` suffix.
pub fn get_short_name(name: &str) -> &str {
    let name = name.strip_prefix(SPACES_PREFIX).unwrap_or(name);
    name.strip_suffix(DURATION_SUFFIX).unwrap_or(name)
}

pub fn get_metric_info(name: &str) -> Option<&'static MetricInfo> {
    KNOWN_METRICS.iter().find(|info| info.name == name)
}

/// A metric selected on the command line (full or short name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metric {
    pub name: String,
}

impl Metric {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }

    /// Accepts a known full or short name, or any full name with a `spaces.perf.` or
    /// `spaces.usage.` prefix (so new metrics work without changing the CLI).
    pub fn parse(value: &str) -> Result<Self, String> {
        if let Some(info) = KNOWN_METRICS
            .iter()
            .find(|info| info.name == value || get_short_name(info.name) == value)
        {
            return Ok(Self::new(info.name));
        }
        let is_full_name = [PERF_PREFIX, USAGE_PREFIX]
            .iter()
            .any(|prefix| value.len() > prefix.len() && value.starts_with(prefix));
        if is_full_name {
            return Ok(Self::new(value));
        }
        let known = KNOWN_METRICS
            .iter()
            .map(|info| get_short_name(info.name))
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "unknown metric `{value}`. Known metrics: {known} (or a full name starting with `{PERF_PREFIX}` or `{USAGE_PREFIX}`)"
        ))
    }

    pub fn short_name(&self) -> &str {
        get_short_name(&self.name)
    }

    pub fn info(&self) -> Option<&'static MetricInfo> {
        get_metric_info(&self.name)
    }

    pub fn source(&self) -> Source {
        if self.name.starts_with(PERF_PREFIX) {
            Source::Perf
        } else {
            Source::Usage
        }
    }

    pub fn is_seconds(&self) -> bool {
        self.name.ends_with("_seconds")
    }

    pub fn default_group_by(&self) -> &'static str {
        self.info()
            .map(|info| info.default_group_by)
            .unwrap_or("@command")
    }

    /// The kind from `KNOWN_METRICS`, or inferred from the data for unknown metrics.
    pub fn kind(&self, records: &[Record]) -> MetricKind {
        if let Some(info) = self.info() {
            return info.kind;
        }
        let is_histogram = records
            .iter()
            .any(|record| record.histograms.iter().any(|h| h.name == self.name));
        if is_histogram {
            MetricKind::Histogram
        } else {
            MetricKind::Counter
        }
    }
}

pub fn get_perf_path(workspace_path: &Path) -> PathBuf {
    workspace_path
        .join(ws::METRICS_DIR_NAME)
        .join(ws::PERF_METRICS_FILE_NAME)
}

pub fn get_usage_path(store_path: &Path) -> PathBuf {
    store_path
        .join(store::SPACES_STORE_METRICS)
        .join(store::USAGE_METRICS_FILE_NAME)
}

/// The records read from a metrics file.
#[derive(Debug, Default)]
pub struct ReadOutcome {
    pub records: Vec<Record>,
    /// Lines that could not be parsed (for example, a torn last line).
    pub skipped: usize,
}

/// Reads all records from a JSONL metrics file. Malformed lines are counted and skipped.
pub fn read_records(path: &Path) -> anyhow::Result<ReadOutcome> {
    read_records_filtered(path, |_| true)
}

/// Streams a JSONL metrics file and keeps the records accepted by `keep`.
pub fn read_records_filtered(
    path: &Path,
    mut keep: impl FnMut(&Record) -> bool,
) -> anyhow::Result<ReadOutcome> {
    let file = std::fs::File::open(path).context(format_context!(
        "Failed to open metrics file {}",
        path.display()
    ))?;
    let reader = std::io::BufReader::new(file);
    let mut outcome = ReadOutcome::default();
    for line in reader.split(b'\n') {
        let line = line.context(format_context!(
            "Failed to read metrics file {}",
            path.display()
        ))?;
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<Record>(&line) {
            Ok(record) => {
                if keep(&record) {
                    outcome.records.push(record);
                }
            }
            Err(_) => outcome.skipped += 1,
        }
    }
    Ok(outcome)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    pub(crate) fn histogram(
        name: &str,
        labels: &[(&str, &str)],
        samples: &[f64],
    ) -> HistogramRecord {
        HistogramRecord {
            name: name.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            samples: samples.to_vec(),
        }
    }

    pub(crate) fn counter(name: &str, labels: &[(&str, &str)], value: u64) -> CounterRecord {
        CounterRecord {
            name: name.to_string(),
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            value,
        }
    }

    pub(crate) fn sample_record(timestamp_ms: u64, command: &str, samples: &[f64]) -> Record {
        Record {
            timestamp_ms,
            spaces_version: "0.23.0".to_string(),
            command: command.to_string(),
            status: "ok".to_string(),
            workspace: Some("/ws".to_string()),
            counters: vec![],
            histograms: vec![histogram(
                PERF_COMMAND_DURATION,
                &[("command", command)],
                samples,
            )],
        }
    }

    #[test]
    fn test_metric_parse() {
        assert_eq!(Metric::parse("perf.rule").unwrap().name, PERF_RULE_DURATION);
        assert_eq!(
            Metric::parse("perf.rule.hash_inputs").unwrap().name,
            PERF_RULE_HASH_INPUTS_DURATION
        );
        assert_eq!(Metric::parse("usage.search").unwrap().name, USAGE_SEARCH);
        assert_eq!(
            Metric::parse(PERF_STEP_DURATION).unwrap().name,
            PERF_STEP_DURATION
        );
        assert_eq!(
            Metric::parse("spaces.usage.new_thing").unwrap().name,
            "spaces.usage.new_thing"
        );
        assert!(Metric::parse("perf.nope").is_err());
        assert!(Metric::parse("spaces.perf.").is_err());
        assert!(Metric::parse("other").is_err());

        let metric = Metric::parse("perf.repo_step").unwrap();
        assert_eq!(metric.short_name(), "perf.repo_step");
        assert_eq!(metric.source(), Source::Perf);
        assert!(metric.is_seconds());
        assert_eq!(metric.default_group_by(), "repo");
        assert_eq!(Metric::parse("usage.rule").unwrap().source(), Source::Usage);
    }

    #[test]
    fn test_every_known_metric_has_a_short_name() {
        for info in KNOWN_METRICS {
            let short = get_short_name(info.name);
            assert!(short.starts_with("perf.") || short.starts_with("usage."));
            assert_eq!(Metric::parse(short).unwrap().name, info.name);
            assert!(info.labels.contains(&info.default_group_by));
        }
    }

    #[test]
    fn test_unknown_metric_kind_is_inferred() {
        let metric = Metric::new("spaces.perf.new.duration_seconds");
        let mut record = sample_record(1, "run", &[1.0]);
        assert_eq!(
            metric.kind(std::slice::from_ref(&record)),
            MetricKind::Counter
        );
        record.histograms[0].name = metric.name.clone();
        assert_eq!(metric.kind(&[record]), MetricKind::Histogram);
        assert_eq!(metric.default_group_by(), "@command");
    }

    #[test]
    fn test_reader_tolerates_bad_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metrics.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        let good = serde_json::to_string(&sample_record(1, "run", &[1.0])).unwrap();
        writeln!(file, "{good}").unwrap();
        writeln!(file, "not json").unwrap();
        writeln!(file).unwrap();
        // unknown extra fields and missing optional fields are fine
        writeln!(
            file,
            r#"{{"timestamp_ms":2,"spaces_version":"9.9.9","command":"sync","status":"err","extra":[1,2]}}"#
        )
        .unwrap();
        file.write_all(&[0xff, 0xfe, b'\n']).unwrap();
        // torn last line
        write!(file, "{}", &good[..good.len() / 2]).unwrap();
        drop(file);

        let outcome = read_records(&path).unwrap();
        assert_eq!(outcome.records.len(), 2);
        assert_eq!(outcome.skipped, 3);
        assert_eq!(outcome.records[1].command, "sync");
        assert!(outcome.records[1].histograms.is_empty());
        assert_eq!(outcome.records[1].workspace, None);

        let outcome = read_records_filtered(&path, |record| record.command == "sync").unwrap();
        assert_eq!(outcome.records.len(), 1);
    }

    #[test]
    fn test_paths() {
        assert_eq!(
            get_perf_path(Path::new("/ws")),
            PathBuf::from("/ws/.spaces/metrics/spaces.perf.jsonl")
        );
        assert_eq!(
            get_usage_path(Path::new("/store")),
            PathBuf::from("/store/metrics/spaces.usage.jsonl")
        );
    }
}
