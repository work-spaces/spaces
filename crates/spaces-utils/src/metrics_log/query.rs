//! Record filters, group keys, and sample selection.

use super::{Metric, MetricKind, Record};
use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

/// Group-by keys that select record fields instead of labels.
pub const PSEUDO_KEYS: &[&str] = &["@command", "@status", "@version", "@workspace", "@day"];

/// Group key used when the label or field is missing.
pub const NONE_KEY: &str = "(none)";

const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 60 * MINUTE_MS;
pub const DAY_MS: u64 = 24 * HOUR_MS;
const WEEK_MS: u64 = 7 * DAY_MS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhenTime {
    Ago(u64),
    At(u64),
}

/// A point in time from `--since`/`--until`: relative (`30m`, `12h`, `7d`, `2w`) or
/// absolute (`2026-10-01`, `2026-10-01 12:00`, RFC 3339). Dates without a zone are local.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct When {
    pub text: String,
    time: WhenTime,
}

impl When {
    pub fn parse(value: &str) -> Result<Self, String> {
        let text = value.trim().to_string();
        let time = parse_relative(&text)
            .map(WhenTime::Ago)
            .or_else(|| parse_absolute(&text).map(WhenTime::At))
            .ok_or_else(|| {
                format!(
                    "invalid time `{text}`: use a relative age (30m, 12h, 7d, 2w) or a date (2026-10-01, 2026-10-01 12:00, RFC 3339)"
                )
            })?;
        Ok(Self { text, time })
    }

    /// Returns the time in milliseconds since the epoch.
    pub fn resolve(&self, now_ms: u64) -> u64 {
        match self.time {
            WhenTime::Ago(ms) => now_ms.saturating_sub(ms),
            WhenTime::At(ms) => ms,
        }
    }
}

fn parse_relative(value: &str) -> Option<u64> {
    let unit_ms = match value.chars().last()? {
        's' => 1000,
        'm' => MINUTE_MS,
        'h' => HOUR_MS,
        'd' => DAY_MS,
        'w' => WEEK_MS,
        _ => return None,
    };
    let count: u64 = value[..value.len() - 1].parse().ok()?;
    count.checked_mul(unit_ms)
}

fn to_ms(timestamp_ms: i64) -> u64 {
    timestamp_ms.max(0) as u64
}

fn parse_absolute(value: &str) -> Option<u64> {
    if let Ok(date_time) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(to_ms(date_time.timestamp_millis()));
    }
    let naive = [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ]
    .iter()
    .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())
    .or_else(|| {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
    })?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|date_time| to_ms(date_time.timestamp_millis()))
}

/// Matches `text` against a pattern where `*` matches any sequence (including `/`)
/// and `?` matches one character.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] != '*' && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            backtrack = Some((p, t));
            p += 1;
        } else if let Some((star_p, star_t)) = backtrack {
            p = star_p + 1;
            t = star_t + 1;
            backtrack = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// `--label KEY=GLOB`
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LabelFilter {
    pub key: String,
    pub glob: String,
}

impl LabelFilter {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.split_once('=') {
            Some((key, glob)) if !key.is_empty() => Ok(Self {
                key: key.to_string(),
                glob: glob.to_string(),
            }),
            _ => Err(format!("invalid label filter `{value}`: use KEY=GLOB")),
        }
    }
}

/// Record- and label-level filters.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Filter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until_ms: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<LabelFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

impl Filter {
    pub fn matches_record(&self, record: &Record) -> bool {
        self.since_ms
            .is_none_or(|since| record.timestamp_ms >= since)
            && self
                .until_ms
                .is_none_or(|until| record.timestamp_ms <= until)
            && self
                .command
                .as_ref()
                .is_none_or(|command| record.command == *command)
            && self
                .status
                .as_ref()
                .is_none_or(|status| record.status.eq_ignore_ascii_case(status))
            && self
                .version
                .as_ref()
                .is_none_or(|glob| wildcard_match(glob, &record.spaces_version))
            && self.workspace.as_ref().is_none_or(|glob| {
                record
                    .workspace
                    .as_deref()
                    .is_some_and(|workspace| wildcard_match(glob, workspace))
            })
    }

    pub fn matches_labels(&self, labels: &BTreeMap<String, String>) -> bool {
        self.labels.iter().all(|filter| {
            labels
                .get(&filter.key)
                .is_some_and(|value| wildcard_match(&filter.glob, value))
        })
    }
}

pub fn validate_group_by(group_by: &str) -> Result<(), String> {
    if group_by.is_empty() {
        return Err("--group-by must not be empty".to_string());
    }
    if group_by.starts_with('@') && !PSEUDO_KEYS.contains(&group_by) {
        return Err(format!(
            "unknown group-by key `{group_by}`. Pseudo-keys: {}",
            PSEUDO_KEYS.join(", ")
        ));
    }
    Ok(())
}

pub(crate) fn format_local(timestamp_ms: u64, format: &str) -> String {
    Local
        .timestamp_millis_opt(timestamp_ms as i64)
        .single()
        .map(|date_time| date_time.format(format).to_string())
        .unwrap_or_else(|| timestamp_ms.to_string())
}

/// The group key of an entry: a label value or a record field (pseudo-key).
pub fn group_key(group_by: &str, record: &Record, labels: &BTreeMap<String, String>) -> String {
    match group_by {
        "@command" => record.command.clone(),
        "@status" => record.status.clone(),
        "@version" => record.spaces_version.clone(),
        "@workspace" => record
            .workspace
            .clone()
            .unwrap_or_else(|| NONE_KEY.to_string()),
        "@day" => format_local(record.timestamp_ms, "%Y-%m-%d"),
        key => labels
            .get(key)
            .cloned()
            .unwrap_or_else(|| NONE_KEY.to_string()),
    }
}

/// One value of a metric. For counters (and histograms with `--per-record`) this is the
/// sum of one record's entries in the group.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub timestamp_ms: u64,
    pub value: f64,
    /// Index of the record in the slice passed to `select`.
    pub record: usize,
}

/// Samples grouped by key.
#[derive(Debug, Default)]
pub struct Selection {
    pub groups: BTreeMap<String, Vec<Sample>>,
    /// Number of records that contributed at least one sample.
    pub records: usize,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn values(&self) -> Vec<f64> {
        self.groups
            .values()
            .flat_map(|samples| samples.iter().map(|sample| sample.value))
            .collect()
    }
}

/// Incremental selector that extracts samples for a single metric while streaming records.
#[derive(Debug)]
pub struct Selector {
    metric: Metric,
    kind: Option<MetricKind>,
    group_by: String,
    filter: Filter,
    last: Option<usize>,
    matched_records: usize,
    contributions: VecDeque<(usize, u64, BTreeMap<String, Vec<f64>>)>,
}

impl Selector {
    pub fn new(
        metric: Metric,
        kind: Option<MetricKind>,
        group_by: impl Into<String>,
        filter: Filter,
        last: Option<usize>,
    ) -> Self {
        Self {
            kind: kind.or_else(|| metric.known_kind()),
            metric,
            group_by: group_by.into(),
            filter,
            last,
            matched_records: 0,
            contributions: VecDeque::new(),
        }
    }

    pub fn push(&mut self, record: &Record) {
        if !self.filter.matches_record(record) {
            return;
        }
        let index = self.matched_records;
        self.matched_records += 1;

        if self.kind.is_none()
            && record
                .histograms
                .iter()
                .any(|entry| entry.name == self.metric.name)
        {
            self.kind = Some(MetricKind::Histogram);
            self.contributions.clear();
        }
        let kind = self.kind.unwrap_or(MetricKind::Counter);

        let mut groups: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        match kind {
            MetricKind::Histogram => {
                for entry in record.histograms.iter().filter(|entry| {
                    entry.name == self.metric.name && self.filter.matches_labels(&entry.labels)
                }) {
                    groups
                        .entry(group_key(&self.group_by, record, &entry.labels))
                        .or_default()
                        .extend(entry.samples.iter().copied().filter(|v| v.is_finite()));
                }
            }
            MetricKind::Counter => {
                for entry in record.counters.iter().filter(|entry| {
                    entry.name == self.metric.name && self.filter.matches_labels(&entry.labels)
                }) {
                    groups
                        .entry(group_key(&self.group_by, record, &entry.labels))
                        .or_default()
                        .push(entry.value as f64);
                }
            }
        }
        groups.retain(|_, values| !values.is_empty());
        if !groups.is_empty() {
            if let Some(last) = self.last {
                if last == 0 {
                    return;
                }
                while self.contributions.len() >= last {
                    self.contributions.pop_front();
                }
            }
            self.contributions
                .push_back((index, record.timestamp_ms, groups));
        }
    }

    pub fn kind(&self) -> MetricKind {
        self.kind.unwrap_or(MetricKind::Counter)
    }

    pub fn matched_records(&self) -> usize {
        self.matched_records
    }

    pub fn finish(&self, per_record: bool) -> Selection {
        let kind = self.kind();
        let mut selection = Selection {
            records: self.contributions.len(),
            ..Default::default()
        };
        let sum_per_record = per_record || kind == MetricKind::Counter;
        for (index, timestamp_ms, groups) in &self.contributions {
            let index = *index;
            let timestamp_ms = *timestamp_ms;
            for (key, values) in groups {
                let samples = selection.groups.entry(key.clone()).or_default();
                let make = |value| Sample {
                    timestamp_ms,
                    value,
                    record: index,
                };
                if sum_per_record {
                    samples.push(make(values.iter().sum()));
                } else {
                    samples.extend(values.iter().copied().map(make));
                }
            }
        }
        selection
    }
}

/// Selects the values of `metric` from the records that pass `filter`, grouped by
/// `group_by`. `last` keeps only the last N contributing records.
pub fn select(
    records: &[Record],
    metric: &Metric,
    kind: MetricKind,
    group_by: &str,
    filter: &Filter,
    per_record: bool,
    last: Option<usize>,
) -> Selection {
    let mut selector = Selector::new(metric.clone(), Some(kind), group_by, filter.clone(), last);
    for record in records {
        selector.push(record);
    }
    selector.finish(per_record)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{counter, histogram, sample_record};
    use super::super::{PERF_COMMAND_DURATION, PERF_STEP_DURATION, USAGE_RULE};
    use super::*;

    #[test]
    fn test_when_relative() {
        let now = 10 * WEEK_MS;
        assert_eq!(
            When::parse("30m").unwrap().resolve(now),
            now - 30 * MINUTE_MS
        );
        assert_eq!(When::parse("12h").unwrap().resolve(now), now - 12 * HOUR_MS);
        assert_eq!(When::parse("7d").unwrap().resolve(now), now - 7 * DAY_MS);
        assert_eq!(When::parse("2w").unwrap().resolve(now), now - 2 * WEEK_MS);
        assert_eq!(When::parse("5s").unwrap().resolve(now), now - 5000);
        assert_eq!(When::parse("100w").unwrap().resolve(now), 0);
        assert_eq!(When::parse(" 7d ").unwrap().text, "7d");
        assert!(When::parse("d").is_err());
        assert!(When::parse("7x").is_err());
        assert!(When::parse("-7d").is_err());
        assert!(When::parse("").is_err());
    }

    #[test]
    fn test_when_absolute() {
        let rfc = When::parse("2026-10-01T00:00:00Z").unwrap();
        assert_eq!(rfc.resolve(0), 1_790_812_800_000);
        let rfc = When::parse("2026-10-01T02:00:00+02:00").unwrap();
        assert_eq!(rfc.resolve(0), 1_790_812_800_000);

        let local_midnight = Local
            .from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 10, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )
            .earliest()
            .unwrap()
            .timestamp_millis() as u64;
        assert_eq!(
            When::parse("2026-10-01").unwrap().resolve(0),
            local_midnight
        );
        assert_eq!(
            When::parse("2026-10-01 01:30").unwrap().resolve(0),
            local_midnight + 90 * MINUTE_MS
        );
        assert!(When::parse("2026-13-01").is_err());
    }

    #[test]
    fn test_wildcard_match() {
        assert!(wildcard_match("//spaces:*", "//spaces:check"));
        assert!(wildcard_match("*check", "//spaces:check"));
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("*", "*build"));
        assert!(wildcard_match("0.2?.*", "0.23.0"));
        assert!(wildcard_match("a*b*c", "aXXbYYc"));
        assert!(!wildcard_match("a*b*c", "aXXbYY"));
        assert!(!wildcard_match("//spaces:*", "//other:check"));
        assert!(wildcard_match("exact", "exact"));
        assert!(!wildcard_match("exact", "exactly"));
    }

    #[test]
    fn test_label_filter() {
        assert_eq!(
            LabelFilter::parse("rule=//spaces:*").unwrap(),
            LabelFilter {
                key: "rule".to_string(),
                glob: "//spaces:*".to_string()
            }
        );
        assert_eq!(LabelFilter::parse("cache=").unwrap().glob, "");
        assert!(LabelFilter::parse("cache").is_err());
        assert!(LabelFilter::parse("=x").is_err());

        let filter = Filter {
            labels: vec![
                LabelFilter::parse("cache=executed").unwrap(),
                LabelFilter::parse("rule=//spaces:*").unwrap(),
            ],
            ..Default::default()
        };
        let labels = |cache: &str, rule: &str| {
            BTreeMap::from([
                ("cache".to_string(), cache.to_string()),
                ("rule".to_string(), rule.to_string()),
            ])
        };
        assert!(filter.matches_labels(&labels("executed", "//spaces:check")));
        assert!(!filter.matches_labels(&labels("restored", "//spaces:check")));
        assert!(!filter.matches_labels(&labels("executed", "//:setup")));
        assert!(!filter.matches_labels(&BTreeMap::new()));
    }

    #[test]
    fn test_record_filters() {
        let mut record = sample_record(1000, "run", &[1.0]);
        record.status = "err".to_string();
        let matches = |filter: Filter| filter.matches_record(&record);
        assert!(matches(Filter::default()));
        assert!(matches(Filter {
            since_ms: Some(1000),
            until_ms: Some(1000),
            ..Default::default()
        }));
        assert!(!matches(Filter {
            since_ms: Some(1001),
            ..Default::default()
        }));
        assert!(!matches(Filter {
            until_ms: Some(999),
            ..Default::default()
        }));
        assert!(matches(Filter {
            command: Some("run".to_string()),
            status: Some("ERR".to_string()),
            version: Some("0.23.*".to_string()),
            workspace: Some("/w*".to_string()),
            ..Default::default()
        }));
        assert!(!matches(Filter {
            command: Some("sync".to_string()),
            ..Default::default()
        }));
        assert!(!matches(Filter {
            version: Some("0.22.*".to_string()),
            ..Default::default()
        }));
        record.workspace = None;
        assert!(
            !Filter {
                workspace: Some("*".to_string()),
                ..Default::default()
            }
            .matches_record(&record)
        );
    }

    #[test]
    fn test_group_keys() {
        let record = sample_record(1000, "run", &[1.0]);
        let labels = BTreeMap::from([("rule".to_string(), "//:a".to_string())]);
        assert_eq!(group_key("@command", &record, &labels), "run");
        assert_eq!(group_key("@status", &record, &labels), "ok");
        assert_eq!(group_key("@version", &record, &labels), "0.23.0");
        assert_eq!(group_key("@workspace", &record, &labels), "/ws");
        assert_eq!(
            group_key("@day", &record, &labels),
            format_local(1000, "%Y-%m-%d")
        );
        assert_eq!(group_key("rule", &record, &labels), "//:a");
        assert_eq!(group_key("cache", &record, &labels), NONE_KEY);
        let mut record = record;
        record.workspace = None;
        assert_eq!(group_key("@workspace", &record, &labels), NONE_KEY);

        assert!(validate_group_by("@day").is_ok());
        assert!(validate_group_by("rule").is_ok());
        assert!(validate_group_by("@nope").is_err());
        assert!(validate_group_by("").is_err());
    }

    #[test]
    fn test_select_histograms_and_counters() {
        let mut first = sample_record(1, "run", &[1.0, 2.0]);
        first.histograms.push(histogram(
            PERF_STEP_DURATION,
            &[("step", "graph.sort")],
            &[0.5, 0.25],
        ));
        first.counters.push(counter(
            USAGE_RULE,
            &[("rule", "//:a"), ("cache", "executed")],
            1,
        ));
        first.counters.push(counter(
            USAGE_RULE,
            &[("rule", "//:b"), ("cache", "executed")],
            2,
        ));
        let second = sample_record(2, "sync", &[4.0]);
        let mut third = sample_record(3, "run", &[f64::NAN, 8.0]);
        third.counters.push(counter(
            USAGE_RULE,
            &[("rule", "//:a"), ("cache", "restored")],
            5,
        ));
        let records = vec![first, second, third];

        let metric = Metric::new(PERF_COMMAND_DURATION);
        let filter = Filter::default();
        let selection = select(
            &records,
            &metric,
            MetricKind::Histogram,
            "command",
            &filter,
            false,
            None,
        );
        assert_eq!(selection.records, 3);
        assert_eq!(selection.groups.len(), 2);
        let run: Vec<f64> = selection.groups["run"].iter().map(|s| s.value).collect();
        assert_eq!(run, vec![1.0, 2.0, 8.0]);
        assert_eq!(selection.groups["run"][2].timestamp_ms, 3);
        assert_eq!(selection.groups["run"][2].record, 2);

        // per-record sums samples
        let selection = select(
            &records,
            &metric,
            MetricKind::Histogram,
            "command",
            &filter,
            true,
            None,
        );
        let run: Vec<f64> = selection.groups["run"].iter().map(|s| s.value).collect();
        assert_eq!(run, vec![3.0, 8.0]);

        // --last applies after filters
        let filter = Filter {
            command: Some("run".to_string()),
            ..Default::default()
        };
        let selection = select(
            &records,
            &metric,
            MetricKind::Histogram,
            "@command",
            &filter,
            false,
            Some(1),
        );
        assert_eq!(selection.records, 1);
        assert_eq!(selection.values(), vec![8.0]);

        // counters are summed per record and group
        let metric = Metric::new(USAGE_RULE);
        let selection = select(
            &records,
            &metric,
            MetricKind::Counter,
            "@command",
            &Filter::default(),
            false,
            None,
        );
        assert_eq!(selection.records, 2);
        let values: Vec<f64> = selection.groups["run"].iter().map(|s| s.value).collect();
        assert_eq!(values, vec![3.0, 5.0]);

        let filter = Filter {
            labels: vec![LabelFilter::parse("cache=executed").unwrap()],
            ..Default::default()
        };
        let selection = select(
            &records,
            &metric,
            MetricKind::Counter,
            "rule",
            &filter,
            false,
            None,
        );
        assert_eq!(selection.records, 1);
        assert_eq!(selection.groups.len(), 2);
        assert_eq!(selection.groups["//:b"][0].value, 2.0);
    }

    #[test]
    fn test_selector_infers_unknown_kind_and_caps_last() {
        let metric = Metric::new("spaces.perf.custom.duration_seconds");
        let r1 = sample_record(1, "run", &[1.0]);
        let mut r2 = sample_record(2, "run", &[2.0]);
        r2.histograms
            .push(histogram(&metric.name, &[("step", "a")], &[0.5]));
        let mut r3 = sample_record(3, "run", &[3.0]);
        r3.histograms
            .push(histogram(&metric.name, &[("step", "b")], &[1.5]));

        let mut selector = Selector::new(metric, None, "step", Filter::default(), Some(1));
        selector.push(&r1);
        assert_eq!(selector.kind(), MetricKind::Counter);
        selector.push(&r2);
        assert_eq!(selector.kind(), MetricKind::Histogram);
        selector.push(&r3);
        assert_eq!(selector.matched_records(), 3);

        let selection = selector.finish(false);
        assert_eq!(selection.records, 1);
        assert_eq!(selection.groups.len(), 1);
        assert_eq!(selection.groups["b"][0].value, 1.5);
        assert_eq!(selection.groups["b"][0].record, 2);
    }
}
