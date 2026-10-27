//! `spaces metrics` subcommands and their output.

use super::query::{LabelFilter, format_local, validate_group_by};
use super::stats::rank;
use super::{
    BinCount, Bucket, Filter, Metric, MetricKind, PruneReport, Record, Scale, Selection, Source,
    Stat, Stats, When, get_perf_path, get_short_name, get_usage_path, make_bins,
    read_records_filtered, select, trend,
};
use crate::features;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use bytesize::ByteSize;
use console::bootstrap::{self, Align, Variant};
use console::plot::{self, BarChart, Bin, HistogramChart, LineChart, PlotOptions, Series};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const DEFAULT_HEIGHT: usize = 20;
const MAX_WIDTH: usize = 120;
const NON_TTY_WIDTH: usize = 100;
const DEFAULT_TOP_LIMIT: usize = 15;
const DEFAULT_TREND_LIMIT: usize = 5;
const TREND_AUTO_BUCKET_POINTS: usize = 200;
const ENABLE_HINT: &str = "spaces features enable collect-metrics";

/// Options shared by the query subcommands.
#[derive(Debug, Clone, clap::Args)]
pub struct MetricsQueryArgs {
    /// Label to group by (default depends on the metric). Pseudo-keys select record
    /// fields: @command, @status, @version, @workspace, @day.
    #[arg(long, value_name = "KEY")]
    pub group_by: Option<String>,
    /// Filter on label values (repeatable). Example: --label cache=executed --label 'rule=//spaces:*'
    #[arg(long, value_name = "KEY=GLOB", value_parser = LabelFilter::parse)]
    pub label: Vec<LabelFilter>,
    /// Filter on the spaces command that recorded the metrics (e.g. run, sync, checkout).
    #[arg(long, value_name = "NAME")]
    pub command: Option<String>,
    /// Filter on the command status.
    #[arg(long, value_enum)]
    pub status: Option<StatusFilter>,
    /// Filter on the spaces version (glob).
    #[arg(long, value_name = "GLOB")]
    pub version: Option<String>,
    /// Filter on the workspace path (glob). Use `--workspace=.` for the current workspace.
    #[arg(long, value_name = "GLOB")]
    pub workspace: Option<String>,
    /// Only include records at or after this time: 30m, 12h, 7d, 2w, 2026-10-01, or RFC 3339.
    #[arg(long, value_name = "WHEN", value_parser = When::parse)]
    pub since: Option<When>,
    /// Only include records at or before this time (same format as --since).
    #[arg(long, value_name = "WHEN", value_parser = When::parse)]
    pub until: Option<When>,
    /// Only keep the last N matching records (applied after the other filters).
    #[arg(long, value_name = "N")]
    pub last: Option<usize>,
    /// Statistic for ranking and trends. Default: mean for histograms, total for counters.
    #[arg(long, value_enum)]
    pub stat: Option<Stat>,
    /// Maximum number of groups shown (default: 15 for top, 5 series for trend, all for summary).
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
    /// Read this JSONL file instead of the default metrics file.
    #[arg(long, value_hint = clap::ValueHint::FilePath)]
    pub file: Option<PathBuf>,
    /// Output format. json and yaml print the aggregated data instead of a chart.
    #[arg(long, value_enum)]
    pub format: Option<console::Format>,
    /// Plot width in columns (default: terminal width up to 120, or 100 when not a TTY).
    #[arg(long, value_name = "COLS")]
    pub width: Option<usize>,
    /// Plot height in rows (default: 20).
    #[arg(long, value_name = "ROWS")]
    pub height: Option<usize>,
    /// Only use ASCII characters. Applied automatically with --ci or when output is not a
    /// terminal; use --ascii=false to override.
    #[arg(long, num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub ascii: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum StatusFilter {
    Ok,
    Err,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum PruneSource {
    Perf,
    Usage,
    All,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum MetricsCommand {
    /// Show the collection status, the metrics files, and the metrics they contain.
    Info {
        /// Read this JSONL file instead of the default metrics files.
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        file: Option<PathBuf>,
        /// Output format.
        #[arg(long, value_enum)]
        format: Option<console::Format>,
    },
    /// Show a statistics table for a metric, grouped by a label.
    Summary {
        /// Metric: a short name like perf.rule or usage.command, or a full name (default: perf.command).
        #[arg(value_parser = Metric::parse)]
        metric: Option<Metric>,
        #[command(flatten)]
        query: MetricsQueryArgs,
    },
    /// Plot the distribution of a histogram metric.
    #[command(visible_alias = "hist")]
    Histogram {
        /// Histogram metric, for example perf.command or perf.rule (default: perf.command).
        #[arg(value_parser = Metric::parse)]
        metric: Option<Metric>,
        /// Number of bins.
        #[arg(long, default_value_t = 20)]
        bins: usize,
        /// Bin scale (default: log for *_seconds metrics, linear otherwise).
        #[arg(long, value_enum)]
        scale: Option<Scale>,
        /// Sum the samples of each record before binning.
        #[arg(long)]
        per_record: bool,
        #[command(flatten)]
        query: MetricsQueryArgs,
    },
    /// Rank groups by a statistic (slowest rules, most-used commands, top search keywords, ...).
    Top {
        /// Metric (default: perf.rule).
        #[arg(value_parser = Metric::parse)]
        metric: Option<Metric>,
        #[command(flatten)]
        query: MetricsQueryArgs,
    },
    /// Plot a metric over time with one series per group.
    Trend {
        /// Metric (default: perf.command).
        #[arg(value_parser = Metric::parse)]
        metric: Option<Metric>,
        /// Time bucket (default: none for fewer than 200 records, otherwise day).
        #[arg(long, value_enum)]
        bucket: Option<Bucket>,
        #[command(flatten)]
        query: MetricsQueryArgs,
    },
    /// Remove records older than --age days from the metrics files.
    Prune {
        /// Remove records this age or older in days (inclusive). Use --age=0 to remove all records.
        #[arg(long, default_value = "30")]
        age: u16,
        /// Show what would be removed without changing the files.
        #[arg(long)]
        dry_run: bool,
        /// Which metrics files to prune.
        #[arg(long, value_enum, default_value_t = PruneSource::All)]
        source: PruneSource,
        /// Prune this JSONL file instead of the default metrics files.
        #[arg(long, value_hint = clap::ValueHint::FilePath)]
        file: Option<PathBuf>,
        /// Output format.
        #[arg(long, value_enum)]
        format: Option<console::Format>,
    },
}

/// Where the metrics live and how output is shown.
#[derive(Debug, Clone)]
pub struct MetricsContext {
    /// The current workspace, if any. Required for `perf.*` metrics unless `--file` is used.
    pub workspace_path: Option<PathBuf>,
    pub store_path: PathBuf,
    /// False when output is not a terminal or with `--ci`.
    pub is_tty: bool,
}

pub fn execute(
    console: &console::Console,
    context: &MetricsContext,
    command: MetricsCommand,
) -> anyhow::Result<()> {
    let now_ms = now_ms();
    match command {
        MetricsCommand::Info { file, format } => {
            show_info(console, context, file, format.unwrap_or_default())
        }
        MetricsCommand::Summary { metric, query } => {
            let metric = metric.unwrap_or_else(|| Metric::new(super::PERF_COMMAND_DURATION));
            let loaded = Loaded::new(console, context, &metric, &query, now_ms)?;
            show_summary(console, context, &loaded, &query)
        }
        MetricsCommand::Histogram {
            metric,
            bins,
            scale,
            per_record,
            query,
        } => {
            let metric = metric.unwrap_or_else(|| Metric::new(super::PERF_COMMAND_DURATION));
            let loaded = Loaded::new(console, context, &metric, &query, now_ms)?;
            show_histogram(console, context, &loaded, &query, bins, scale, per_record)
        }
        MetricsCommand::Top { metric, query } => {
            let metric = metric.unwrap_or_else(|| Metric::new(super::PERF_RULE_DURATION));
            let loaded = Loaded::new(console, context, &metric, &query, now_ms)?;
            show_top(console, context, &loaded, &query)
        }
        MetricsCommand::Trend {
            metric,
            bucket,
            query,
        } => {
            let metric = metric.unwrap_or_else(|| Metric::new(super::PERF_COMMAND_DURATION));
            let loaded = Loaded::new(console, context, &metric, &query, now_ms)?;
            show_trend(console, context, &loaded, &query, bucket)
        }
        MetricsCommand::Prune {
            age,
            dry_run,
            source,
            file,
            format,
        } => {
            let cutoff_ms = now_ms.saturating_sub(age as u64 * super::query::DAY_MS);
            prune(
                console,
                context,
                PruneOptions {
                    cutoff_ms,
                    age,
                    dry_run,
                    source,
                    file,
                },
                format.unwrap_or_default(),
            )
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

/// Three significant digits (fewer when the trailing digits are zero).
fn format_significant(value: f64) -> String {
    let magnitude = value.abs();
    let text = if magnitude >= 100.0 {
        format!("{value:.0}")
    } else if magnitude >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    };
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text
    }
}

pub(crate) fn format_seconds(seconds: f64, ascii: bool) -> String {
    if !seconds.is_finite() {
        return "-".to_string();
    }
    if seconds == 0.0 {
        return "0s".to_string();
    }
    let magnitude = seconds.abs();
    if magnitude >= 60.0 {
        return console::format_duration(seconds);
    }
    let (value, unit) = if magnitude >= 1.0 {
        (seconds, "s")
    } else if magnitude >= 1e-3 {
        (seconds * 1e3, "ms")
    } else if magnitude >= 1e-6 {
        (seconds * 1e6, if ascii { "us" } else { "µs" })
    } else {
        (seconds * 1e9, "ns")
    };
    format!("{}{unit}", format_significant(value))
}

pub(crate) fn format_count(value: f64) -> String {
    if !value.is_finite() {
        return "-".to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    format_significant(value)
}

/// Formats metric values: durations for `*_seconds` histograms, plain numbers otherwise.
#[derive(Debug, Clone, Copy)]
struct ValueFormat {
    is_seconds: bool,
    ascii: bool,
}

impl ValueFormat {
    fn new(metric: &Metric, kind: MetricKind, ascii: bool) -> Self {
        Self {
            is_seconds: kind == MetricKind::Histogram && metric.is_seconds(),
            ascii,
        }
    }

    fn format(&self, value: f64) -> String {
        if self.is_seconds {
            format_seconds(value, self.ascii)
        } else {
            format_count(value)
        }
    }

    fn format_stat(&self, stat: Stat, value: f64) -> String {
        if stat == Stat::Count {
            format_count(value)
        } else {
            self.format(value)
        }
    }
}

fn emit_serialized<T: Serialize>(
    console: &console::Console,
    format: &console::Format,
    value: &T,
) -> anyhow::Result<()> {
    let text = match format {
        console::Format::Yaml => serde_yaml::to_string(value).context(format_context!(
            "Internal Error: failed to serialize metrics as YAML"
        ))?,
        _ => serde_json::to_string_pretty(value).context(format_context!(
            "Internal Error: failed to serialize metrics as JSON"
        ))?,
    };
    console.write(&text)
}

fn plot_options(query: &MetricsQueryArgs, context: &MetricsContext) -> PlotOptions {
    let is_ascii_mode =
        bootstrap::typography_mode() == bootstrap::TypographyMode::Ascii || !context.is_tty;
    let is_no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    PlotOptions {
        ascii: query.ascii.unwrap_or(is_ascii_mode),
        color: context.is_tty && !is_no_color,
    }
}

fn plot_width(query: &MetricsQueryArgs, context: &MetricsContext) -> usize {
    query.width.unwrap_or_else(|| {
        if context.is_tty {
            plot::terminal_width()
                .map(|width| width.saturating_sub(1))
                .unwrap_or(NON_TTY_WIDTH)
                .min(MAX_WIDTH)
        } else {
            NON_TTY_WIDTH
        }
    })
}

fn subtitle(text: String) -> bootstrap::Paragraph {
    bootstrap::Paragraph::new(text).variant(Variant::Secondary)
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

fn resolve_path(
    source: Source,
    file: Option<&PathBuf>,
    context: &MetricsContext,
) -> anyhow::Result<PathBuf> {
    if let Some(file) = file {
        return Ok(file.clone());
    }
    match source {
        Source::Perf => context
            .workspace_path
            .as_deref()
            .map(get_perf_path)
            .ok_or_else(|| {
                format_error!(
                    "Metrics command must be run from within a workspace to read `perf.*` metrics (or use --file)"
                )
            }),
        Source::Usage => Ok(get_usage_path(&context.store_path)),
    }
}

/// The selected samples of one metric plus everything needed to describe them.
struct Loaded {
    metric: Metric,
    kind: MetricKind,
    group_by: String,
    filter: Filter,
    filter_descriptions: Vec<String>,
    path: PathBuf,
    exists: bool,
    records_read: usize,
    skipped_lines: usize,
    records: Vec<Record>,
}

impl Loaded {
    fn new(
        console: &console::Console,
        context: &MetricsContext,
        metric: &Metric,
        query: &MetricsQueryArgs,
        now_ms: u64,
    ) -> anyhow::Result<Self> {
        let group_by = query
            .group_by
            .clone()
            .unwrap_or_else(|| metric.default_group_by().to_string());
        validate_group_by(&group_by).map_err(|message| format_error!("{message}"))?;

        let workspace = query.workspace.as_ref().map(|workspace| {
            if workspace == "." {
                context
                    .workspace_path
                    .clone()
                    .or_else(|| std::env::current_dir().ok())
                    .map(|path| path.to_string_lossy().to_string())
                    .unwrap_or_else(|| workspace.clone())
            } else {
                workspace.clone()
            }
        });
        let filter = Filter {
            since_ms: query.since.as_ref().map(|when| when.resolve(now_ms)),
            until_ms: query.until.as_ref().map(|when| when.resolve(now_ms)),
            labels: query.label.clone(),
            command: query.command.clone(),
            status: query.status.map(|status| status.to_string()),
            version: query.version.clone(),
            workspace,
        };

        let path = resolve_path(metric.source(), query.file.as_ref(), context)?;
        let exists = path.exists();
        let (records, skipped_lines) = if exists {
            let outcome = read_records_filtered(&path, |record| filter.matches_record(record))
                .context(format_context!("while reading {}", path.display()))?;
            (outcome.records, outcome.skipped)
        } else {
            (Vec::new(), 0)
        };
        if skipped_lines > 0 {
            console.debug(
                "metrics",
                format!(
                    "skipped {skipped_lines} malformed line(s) in {}",
                    path.display()
                ),
            )?;
        }

        let mut filter_descriptions = Vec::new();
        if let Some(since) = &query.since {
            filter_descriptions.push(format!("since {}", since.text));
        }
        if let Some(until) = &query.until {
            filter_descriptions.push(format!("until {}", until.text));
        }
        for (name, value) in [
            ("command", &query.command),
            ("version", &query.version),
            ("workspace", &filter.workspace),
        ] {
            if let Some(value) = value {
                filter_descriptions.push(format!("{name}={value}"));
            }
        }
        if let Some(status) = &filter.status {
            filter_descriptions.push(format!("status={status}"));
        }
        for label in &query.label {
            filter_descriptions.push(format!("{}={}", label.key, label.glob));
        }
        if let Some(last) = query.last {
            filter_descriptions.push(format!("last {last}"));
        }

        Ok(Self {
            kind: metric.kind(&records),
            metric: metric.clone(),
            group_by,
            filter,
            filter_descriptions,
            path,
            exists,
            records_read: records.len(),
            skipped_lines,
            records,
        })
    }

    fn select(&self, query: &MetricsQueryArgs, per_record: bool) -> Selection {
        select(
            &self.records,
            &self.metric,
            self.kind,
            &self.group_by,
            &self.filter,
            per_record,
            query.last,
        )
    }

    fn default_stat(&self, query: &MetricsQueryArgs) -> Stat {
        query.stat.unwrap_or(match self.kind {
            MetricKind::Histogram => Stat::Mean,
            MetricKind::Counter => Stat::Total,
        })
    }

    fn description(&self, selection: &Selection, extra: &[String]) -> String {
        let mut parts = vec![
            self.metric.short_name().to_string(),
            format!("by {}", self.group_by),
        ];
        parts.extend(extra.iter().cloned());
        parts.extend(self.filter_descriptions.iter().cloned());
        parts.push(format!(
            "{} record{}",
            selection.records,
            if selection.records == 1 { "" } else { "s" }
        ));
        parts.join(" · ")
    }

    /// Shows why there is nothing to display. Returns true if the selection is empty.
    fn emit_if_empty(&self, console: &console::Console, selection: &Selection) -> bool {
        if !selection.is_empty() {
            return false;
        }
        let lines = if self.exists {
            vec![
                bootstrap::Paragraph::new(format!(
                    "No `{}` samples match the filters.",
                    self.metric.short_name()
                ))
                .variant(Variant::Warning),
                subtitle(format!(
                    "{} record(s) read from {}",
                    self.records_read,
                    self.path.display()
                )),
            ]
        } else {
            vec![
                bootstrap::Paragraph::new(format!(
                    "No metrics recorded: {} does not exist.",
                    self.path.display()
                ))
                .variant(Variant::Warning),
                subtitle(format!("Enable metrics collection with `{ENABLE_HINT}`.")),
            ]
        };
        let mut container = bootstrap::Container::new();
        for line in lines {
            container.add(line);
        }
        console.emit_container(&container);
        true
    }

    fn output<'a>(&'a self, selection: &Selection) -> QueryOutput<'a> {
        QueryOutput {
            metric: &self.metric.name,
            kind: self.kind,
            file: self.path.display().to_string(),
            group_by: &self.group_by,
            filters: &self.filter,
            records: selection.records,
            skipped_lines: self.skipped_lines,
            stat: None,
            groups: Vec::new(),
            other: None,
            scale: None,
            per_record: None,
            stats: None,
            bins: None,
            bucket: None,
            series: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Query output (json/yaml)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct GroupOutput {
    key: String,
    #[serde(flatten)]
    stats: Stats,
}

#[derive(Debug, Serialize)]
struct OtherOutput {
    groups: usize,
    #[serde(flatten)]
    stats: Stats,
}

#[derive(Debug, Serialize)]
struct SeriesOutput {
    name: String,
    points: Vec<(u64, f64)>,
}

#[derive(Debug, Serialize)]
struct QueryOutput<'a> {
    metric: &'a str,
    kind: MetricKind,
    file: String,
    group_by: &'a str,
    filters: &'a Filter,
    records: usize,
    skipped_lines: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    stat: Option<Stat>,
    groups: Vec<GroupOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    other: Option<OtherOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<Scale>,
    #[serde(skip_serializing_if = "Option::is_none")]
    per_record: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<Stats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bins: Option<Vec<BinCount>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bucket: Option<Bucket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    series: Option<Vec<SeriesOutput>>,
}

/// Ranked groups limited to `limit`; the rest are folded into one "other" entry.
fn ranked_groups(
    selection: &Selection,
    stat: Stat,
    limit: Option<usize>,
) -> (Vec<GroupOutput>, Option<OtherOutput>) {
    let mut ranked = rank(&selection.groups, stat);
    let limit = limit.unwrap_or(ranked.len());
    let rest: Vec<(String, Stats)> = if ranked.len() > limit {
        ranked.split_off(limit)
    } else {
        Vec::new()
    };
    let other = if rest.is_empty() {
        None
    } else {
        let values: Vec<f64> = rest
            .iter()
            .flat_map(|(key, _)| selection.groups[key].iter().map(|sample| sample.value))
            .collect();
        Stats::new(&values).map(|stats| OtherOutput {
            groups: rest.len(),
            stats,
        })
    };
    let groups = ranked
        .into_iter()
        .map(|(key, stats)| GroupOutput { key, stats })
        .collect();
    (groups, other)
}

fn stats_table(
    loaded: &Loaded,
    groups: &[GroupOutput],
    other: Option<&OtherOutput>,
    format: ValueFormat,
) -> bootstrap::Table {
    let is_histogram = loaded.kind == MetricKind::Histogram;
    let (headers, alignments): (Vec<&str>, Vec<Align>) = if is_histogram {
        (
            vec![
                loaded.group_by.as_str(),
                "count",
                "total",
                "mean",
                "p50",
                "p90",
                "p99",
                "max",
            ],
            [vec![Align::Left], vec![Align::Right; 7]].concat(),
        )
    } else {
        (
            vec![loaded.group_by.as_str(), "records", "total", "mean", "max"],
            [vec![Align::Left], vec![Align::Right; 4]].concat(),
        )
    };
    let row = |key: String, stats: &Stats| -> Vec<String> {
        let mut row = vec![
            key,
            format_count(stats.count as f64),
            format.format(stats.total),
        ];
        if is_histogram {
            row.extend(
                [stats.mean, stats.p50, stats.p90, stats.p99, stats.max].map(|v| format.format(v)),
            );
        } else {
            row.extend([format_count(stats.mean), format_count(stats.max)]);
        }
        row
    };
    let mut rows: Vec<Vec<String>> = groups
        .iter()
        .map(|group| row(group.key.clone(), &group.stats))
        .collect();
    if let Some(other) = other {
        rows.push(row(format!("(other {})", other.groups), &other.stats));
    }
    bootstrap::Table::new()
        .headers(headers.into_iter().map(str::to_string).collect())
        .alignments(alignments)
        .rows(rows)
}

// ---------------------------------------------------------------------------
// summary / top / histogram / trend
// ---------------------------------------------------------------------------

fn show_summary(
    console: &console::Console,
    context: &MetricsContext,
    loaded: &Loaded,
    query: &MetricsQueryArgs,
) -> anyhow::Result<()> {
    let format = query.format.clone().unwrap_or_default();
    let selection = loaded.select(query, false);
    let stat = loaded.default_stat(query);
    let (groups, other) = ranked_groups(&selection, stat, query.limit);

    if !matches!(format, console::Format::Pretty) {
        let mut output = loaded.output(&selection);
        output.stat = Some(stat);
        output.groups = groups;
        output.other = other;
        return emit_serialized(console, &format, &output);
    }
    if loaded.emit_if_empty(console, &selection) {
        return Ok(());
    }
    let value_format = ValueFormat::new(
        &loaded.metric,
        loaded.kind,
        plot_options(query, context).ascii,
    );
    let mut container = bootstrap::Container::new();
    container.add(subtitle(
        loaded.description(&selection, &[format!("sorted by {stat}")]),
    ));
    container.add(stats_table(loaded, &groups, other.as_ref(), value_format));
    console.emit_container(&container);
    Ok(())
}

fn show_top(
    console: &console::Console,
    context: &MetricsContext,
    loaded: &Loaded,
    query: &MetricsQueryArgs,
) -> anyhow::Result<()> {
    let format = query.format.clone().unwrap_or_default();
    let selection = loaded.select(query, false);
    let stat = loaded.default_stat(query);
    let limit = query.limit.unwrap_or(DEFAULT_TOP_LIMIT);
    let (groups, other) = ranked_groups(&selection, stat, Some(limit));

    if !matches!(format, console::Format::Pretty) {
        let mut output = loaded.output(&selection);
        output.stat = Some(stat);
        output.groups = groups;
        output.other = other;
        return emit_serialized(console, &format, &output);
    }
    if loaded.emit_if_empty(console, &selection) {
        return Ok(());
    }

    let options = plot_options(query, context);
    let value_format = ValueFormat::new(&loaded.metric, loaded.kind, options.ascii);
    let bars = groups
        .iter()
        .map(|group| (group.key.clone(), group.stats.get(stat)))
        .collect();
    let chart = BarChart::new(bars)
        .title(loaded.description(&selection, &[format!("top {limit} by {stat}")]))
        .value_formatter(move |value| value_format.format_stat(stat, value))
        .width(plot_width(query, context))
        .options(options);

    let mut container = bootstrap::Container::new();
    container.add(chart);
    container.add(bootstrap::VerticalSpacer::new(1));
    container.add(stats_table(loaded, &groups, other.as_ref(), value_format));
    console.emit_container(&container);
    Ok(())
}

fn show_histogram(
    console: &console::Console,
    context: &MetricsContext,
    loaded: &Loaded,
    query: &MetricsQueryArgs,
    bins: usize,
    scale: Option<Scale>,
    per_record: bool,
) -> anyhow::Result<()> {
    if loaded.kind != MetricKind::Histogram {
        return Err(format_error!(
            "`{}` is a counter; `histogram` needs a histogram metric (perf.*). Use `top` or `trend` for counters.",
            loaded.metric.short_name()
        ));
    }
    let format = query.format.clone().unwrap_or_default();
    let scale = scale.unwrap_or(if loaded.metric.is_seconds() {
        Scale::Log
    } else {
        Scale::Linear
    });
    let selection = loaded.select(query, per_record);
    let values = selection.values();
    let stats = Stats::new(&values);
    let bin_counts = make_bins(&values, bins.max(1), scale);

    if !matches!(format, console::Format::Pretty) {
        let mut output = loaded.output(&selection);
        let (groups, _) = ranked_groups(&selection, loaded.default_stat(query), None);
        output.groups = groups;
        output.scale = Some(scale);
        output.per_record = Some(per_record);
        output.stats = stats;
        output.bins = Some(bin_counts);
        return emit_serialized(console, &format, &output);
    }
    if loaded.emit_if_empty(console, &selection) {
        return Ok(());
    }

    let options = plot_options(query, context);
    let value_format = ValueFormat::new(&loaded.metric, loaded.kind, options.ascii);
    let footer = stats
        .map(|stats| {
            format!(
                "n={}  min={}  p50={}  p90={}  p99={}  max={}",
                stats.count,
                value_format.format(stats.min),
                value_format.format(stats.p50),
                value_format.format(stats.p90),
                value_format.format(stats.p99),
                value_format.format(stats.max),
            )
        })
        .unwrap_or_default();
    let unit = if per_record {
        "per record"
    } else {
        "per sample"
    };
    let chart = HistogramChart::new(
        bin_counts
            .into_iter()
            .map(|bin| Bin {
                lo: bin.lo,
                hi: bin.hi,
                count: bin.count,
            })
            .collect(),
    )
    .title(loaded.description(&selection, &[format!("{scale} bins"), unit.to_string()]))
    .x_formatter(move |value| value_format.format(value))
    .footer(footer)
    .width(plot_width(query, context))
    .height(query.height.unwrap_or(DEFAULT_HEIGHT))
    .options(options);

    let mut container = bootstrap::Container::new();
    container.add(chart);
    console.emit_container(&container);
    Ok(())
}

fn time_format(span_ms: u64) -> &'static str {
    if span_ms <= super::query::DAY_MS {
        "%H:%M"
    } else if span_ms <= 90 * super::query::DAY_MS {
        "%m-%d"
    } else {
        "%Y-%m-%d"
    }
}

fn show_trend(
    console: &console::Console,
    context: &MetricsContext,
    loaded: &Loaded,
    query: &MetricsQueryArgs,
    bucket: Option<Bucket>,
) -> anyhow::Result<()> {
    let format = query.format.clone().unwrap_or_default();
    let selection = loaded.select(query, false);
    let stat = loaded.default_stat(query);
    let bucket = bucket.unwrap_or(if selection.records < TREND_AUTO_BUCKET_POINTS {
        Bucket::None
    } else {
        Bucket::Day
    });
    let limit = query.limit.unwrap_or(DEFAULT_TREND_LIMIT);
    let (groups, other) = ranked_groups(&selection, stat, Some(limit));
    let series: Vec<SeriesOutput> = groups
        .iter()
        .map(|group| SeriesOutput {
            name: group.key.clone(),
            points: trend(&selection.groups[&group.key], bucket, stat),
        })
        .collect();

    if !matches!(format, console::Format::Pretty) {
        let mut output = loaded.output(&selection);
        output.stat = Some(stat);
        output.bucket = Some(bucket);
        output.groups = groups;
        output.other = other;
        output.series = Some(series);
        return emit_serialized(console, &format, &output);
    }
    if loaded.emit_if_empty(console, &selection) {
        return Ok(());
    }

    let options = plot_options(query, context);
    let value_format = ValueFormat::new(&loaded.metric, loaded.kind, options.ascii);
    let timestamps = series.iter().flat_map(|s| s.points.iter().map(|p| p.0));
    let (first, last) = timestamps.fold((u64::MAX, 0), |(lo, hi), ts| (lo.min(ts), hi.max(ts)));
    let date_format = time_format(last.saturating_sub(first));
    let per = match bucket {
        Bucket::None => "per record".to_string(),
        bucket => format!("per {bucket}"),
    };
    let mut chart = LineChart::new(
        series
            .iter()
            .map(|s| Series {
                name: s.name.clone(),
                points: s.points.iter().map(|(ts, v)| (*ts as f64, *v)).collect(),
            })
            .collect(),
    )
    .title(loaded.description(&selection, &[format!("{stat} {per}")]))
    .x_formatter(move |value| format_local(value.max(0.0) as u64, date_format))
    .y_formatter(move |value| value_format.format_stat(stat, value))
    .width(plot_width(query, context))
    .height(query.height.unwrap_or(DEFAULT_HEIGHT))
    .options(options);
    if let Some(other) = &other {
        chart = chart.footer(format!(
            "{} more group(s) not shown (use --limit)",
            other.groups
        ));
    }

    let mut container = bootstrap::Container::new();
    container.add(chart);
    container.add(bootstrap::VerticalSpacer::new(1));
    container.add(stats_table(loaded, &groups, other.as_ref(), value_format));
    console.emit_container(&container);
    Ok(())
}

// ---------------------------------------------------------------------------
// info
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CollectionOutput {
    enabled: bool,
    source: String,
}

#[derive(Debug, Serialize)]
struct MetricSummaryOutput {
    name: String,
    kind: String,
    labels: Vec<String>,
    /// Histogram samples, or the number of counter entries.
    samples: usize,
    /// Sum of the samples or counter values.
    total: f64,
}

#[derive(Debug, Serialize)]
struct SourceOutput {
    source: String,
    path: String,
    exists: bool,
    size_bytes: u64,
    records: usize,
    skipped_lines: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_timestamp_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_timestamp_ms: Option<u64>,
    metrics: Vec<MetricSummaryOutput>,
}

#[derive(Debug, Serialize)]
struct InfoOutput {
    collect_metrics: CollectionOutput,
    sources: Vec<SourceOutput>,
}

fn summarize_source(source: &str, path: &Path) -> anyhow::Result<SourceOutput> {
    let mut output = SourceOutput {
        source: source.to_string(),
        path: path.display().to_string(),
        exists: path.exists(),
        size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        records: 0,
        skipped_lines: 0,
        first_timestamp_ms: None,
        last_timestamp_ms: None,
        metrics: Vec::new(),
    };
    if !output.exists {
        return Ok(output);
    }

    struct Entry {
        kind: MetricKind,
        labels: BTreeSet<String>,
        samples: usize,
        total: f64,
    }
    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
    let mut add = |name: &str, kind, labels: &BTreeMap<String, String>, samples, total| {
        let entry = entries.entry(name.to_string()).or_insert(Entry {
            kind,
            labels: BTreeSet::new(),
            samples: 0,
            total: 0.0,
        });
        entry.labels.extend(labels.keys().cloned());
        entry.samples += samples;
        entry.total += total;
    };

    let (mut first, mut last, mut records) = (u64::MAX, 0, 0);
    let outcome = read_records_filtered(path, |record| {
        records += 1;
        first = first.min(record.timestamp_ms);
        last = last.max(record.timestamp_ms);
        for counter in &record.counters {
            add(
                &counter.name,
                MetricKind::Counter,
                &counter.labels,
                1,
                counter.value as f64,
            );
        }
        for histogram in &record.histograms {
            add(
                &histogram.name,
                MetricKind::Histogram,
                &histogram.labels,
                histogram.samples.len(),
                histogram.samples.iter().filter(|v| v.is_finite()).sum(),
            );
        }
        false
    })
    .context(format_context!("while reading {}", path.display()))?;

    output.records = records;
    output.skipped_lines = outcome.skipped;
    if records > 0 {
        output.first_timestamp_ms = Some(first);
        output.last_timestamp_ms = Some(last);
    }
    output.metrics = entries
        .into_iter()
        .map(|(name, entry)| MetricSummaryOutput {
            name,
            kind: entry.kind.to_string(),
            labels: entry.labels.into_iter().collect(),
            samples: entry.samples,
            total: entry.total,
        })
        .collect();
    Ok(output)
}

fn show_info(
    console: &console::Console,
    context: &MetricsContext,
    file: Option<PathBuf>,
    format: console::Format,
) -> anyhow::Result<()> {
    let features = features::Features::new_from_json(&context.store_path)
        .context(format_context!("while loading features"))?;
    let (enabled, feature_source) =
        features.get_status_with_source(features::Feature::CollectMetrics);

    let mut sources = Vec::new();
    if let Some(file) = file {
        sources.push(summarize_source("file", &file)?);
    } else {
        if let Some(workspace_path) = &context.workspace_path {
            sources.push(summarize_source(
                &Source::Perf.to_string(),
                &get_perf_path(workspace_path),
            )?);
        }
        sources.push(summarize_source(
            &Source::Usage.to_string(),
            &get_usage_path(&context.store_path),
        )?);
    }

    let output = InfoOutput {
        collect_metrics: CollectionOutput {
            enabled,
            source: feature_source.to_string(),
        },
        sources,
    };
    if !matches!(format, console::Format::Pretty) {
        return emit_serialized(console, &format, &output);
    }

    let mut container = bootstrap::Container::new();
    container.add(bootstrap::Header::h1("Metrics").variant(Variant::Primary));
    let status = format!(
        "{} ({})",
        if enabled { "ON" } else { "OFF" },
        output.collect_metrics.source
    );
    container.add(
        bootstrap::DescriptionList::new()
            .compact(true)
            .item("Collection", status),
    );
    if context.workspace_path.is_none() {
        container.add(subtitle(
            "Not in a workspace: perf metrics are only available inside a workspace.".to_string(),
        ));
    }

    let ascii = bootstrap::typography_mode() == bootstrap::TypographyMode::Ascii;
    for source in &output.sources {
        container.add(bootstrap::Header::h2(source.source.clone()).variant(Variant::Primary));
        let mut list = bootstrap::DescriptionList::new()
            .compact(true)
            .item("Path", source.path.clone());
        if !source.exists {
            list = list.item("Status", "not created yet");
            container.add(list);
            continue;
        }
        let range = match (source.first_timestamp_ms, source.last_timestamp_ms) {
            (Some(first), Some(last)) => format!(
                "{} {} {}",
                format_local(first, "%Y-%m-%d %H:%M"),
                bootstrap::op_arrow_right(),
                format_local(last, "%Y-%m-%d %H:%M")
            ),
            _ => "-".to_string(),
        };
        list = list
            .item("Size", ByteSize(source.size_bytes).display().to_string())
            .item("Records", source.records.to_string())
            .item("Range", range);
        if source.skipped_lines > 0 {
            list = list.item("Skipped lines", source.skipped_lines.to_string());
        }
        container.add(list);

        if !source.metrics.is_empty() {
            let rows = source
                .metrics
                .iter()
                .map(|metric| {
                    let is_seconds = metric.kind == MetricKind::Histogram.to_string()
                        && metric.name.ends_with("_seconds");
                    vec![
                        get_short_name(&metric.name).to_string(),
                        metric.kind.clone(),
                        metric.labels.join(", "),
                        metric.samples.to_string(),
                        if is_seconds {
                            format_seconds(metric.total, ascii)
                        } else {
                            format_count(metric.total)
                        },
                    ]
                })
                .collect();
            container.add(
                bootstrap::Table::new()
                    .headers(
                        ["metric", "kind", "labels", "samples", "total"]
                            .map(str::to_string)
                            .to_vec(),
                    )
                    .alignments(vec![
                        Align::Left,
                        Align::Left,
                        Align::Left,
                        Align::Right,
                        Align::Right,
                    ])
                    .rows(rows),
            );
        }
    }

    let has_data = output.sources.iter().any(|source| source.exists);
    container.add(bootstrap::VerticalSpacer::new(1));
    if !has_data {
        container.add(
            bootstrap::Alert::new("No metrics have been recorded yet.")
                .title("No metrics")
                .variant(Variant::Warning),
        );
        container.add(subtitle(format!("Enable collection with `{ENABLE_HINT}`.")));
    } else if !enabled {
        container.add(subtitle(format!(
            "Metrics collection is off. Enable it with `{ENABLE_HINT}`."
        )));
    }
    console.emit_container(&container);
    Ok(())
}

// ---------------------------------------------------------------------------
// prune
// ---------------------------------------------------------------------------

struct PruneOptions {
    cutoff_ms: u64,
    age: u16,
    dry_run: bool,
    source: PruneSource,
    file: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct PruneOutput {
    age_days: u16,
    cutoff_timestamp_ms: u64,
    dry_run: bool,
    files: Vec<PruneReport>,
}

fn prune(
    console: &console::Console,
    context: &MetricsContext,
    options: PruneOptions,
    format: console::Format,
) -> anyhow::Result<()> {
    let mut paths = Vec::new();
    if let Some(file) = &options.file {
        paths.push(file.clone());
    } else {
        if matches!(options.source, PruneSource::Perf | PruneSource::All) {
            match (&context.workspace_path, options.source) {
                (Some(workspace_path), _) => paths.push(get_perf_path(workspace_path)),
                (None, PruneSource::Perf) => {
                    return Err(format_error!(
                        "Metrics command must be run from within a workspace to prune `perf` metrics (or use --file)"
                    ));
                }
                (None, _) => {}
            }
        }
        if matches!(options.source, PruneSource::Usage | PruneSource::All) {
            paths.push(get_usage_path(&context.store_path));
        }
    }

    let mut files = Vec::new();
    for path in paths.iter().filter(|path| path.exists()) {
        files.push(
            super::prune_file(path, options.cutoff_ms, options.dry_run)
                .context(format_context!("while pruning {}", path.display()))?,
        );
    }

    let output = PruneOutput {
        age_days: options.age,
        cutoff_timestamp_ms: options.cutoff_ms,
        dry_run: options.dry_run,
        files,
    };
    if !matches!(format, console::Format::Pretty) {
        return emit_serialized(console, &format, &output);
    }

    let mut container = bootstrap::Container::new();
    if output.files.is_empty() {
        container.add(subtitle(format!(
            "No metrics files to prune ({}).",
            paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
        console.emit_container(&container);
        return Ok(());
    }
    let removed_header = if output.dry_run {
        "would remove"
    } else {
        "removed"
    };
    let rows = output
        .files
        .iter()
        .map(|report| {
            vec![
                report.path.display().to_string(),
                report.kept.to_string(),
                report.removed.to_string(),
                report.unparseable.to_string(),
            ]
        })
        .collect();
    container.add(subtitle(format!(
        "Records {} days or older (before {})",
        output.age_days,
        format_local(output.cutoff_timestamp_ms, "%Y-%m-%d %H:%M")
    )));
    container.add(
        bootstrap::Table::new()
            .headers(
                ["file", "kept", removed_header, "unparseable (kept)"]
                    .map(str::to_string)
                    .to_vec(),
            )
            .alignments(vec![Align::Left, Align::Right, Align::Right, Align::Right])
            .rows(rows),
    );
    if output.dry_run {
        container.add(subtitle("Dry run: no files were changed.".to_string()));
    }
    console.emit_container(&container);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_seconds() {
        assert_eq!(format_seconds(0.0, false), "0s");
        assert_eq!(format_seconds(9.738, false), "9.74s");
        assert_eq!(format_seconds(2.0, false), "2s");
        assert_eq!(format_seconds(0.1, false), "100ms");
        assert_eq!(format_seconds(0.0073, false), "7.3ms");
        assert_eq!(format_seconds(0.0000072, false), "7.2µs");
        assert_eq!(format_seconds(0.0000072, true), "7.2us");
        assert_eq!(format_seconds(2e-7, false), "200ns");
        assert_eq!(format_seconds(14.2, false), "14.2s");
        assert_eq!(format_seconds(125.0, false), "2m05s");
        assert_eq!(format_seconds(f64::NAN, false), "-");
    }

    #[test]
    fn test_format_count() {
        assert_eq!(format_count(3.0), "3");
        assert_eq!(format_count(2.5), "2.5");
        assert_eq!(format_count(1.0 / 3.0), "0.33");
        assert_eq!(format_count(123.456), "123");
    }

    #[test]
    fn test_ranked_groups_folds_other() {
        use super::super::Sample;
        let sample = |value| Sample {
            timestamp_ms: 0,
            value,
            record: 0,
        };
        let selection = Selection {
            groups: BTreeMap::from([
                ("a".to_string(), vec![sample(1.0)]),
                ("b".to_string(), vec![sample(3.0)]),
                ("c".to_string(), vec![sample(2.0), sample(4.0)]),
            ]),
            records: 1,
        };
        let (groups, other) = ranked_groups(&selection, Stat::Total, Some(1));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, "c");
        let other = other.unwrap();
        assert_eq!(other.groups, 2);
        assert_eq!(other.stats.total, 4.0);
        assert_eq!(other.stats.count, 2);

        let (groups, other) = ranked_groups(&selection, Stat::Total, None);
        assert_eq!(groups.len(), 3);
        assert!(other.is_none());
    }
}
