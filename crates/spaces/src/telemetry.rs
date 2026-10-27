//! Metrics collection for spaces.
//!
//! A single in-memory recorder collects all `metrics` samples during the process. At exit,
//! `finish()` partitions them by name prefix and appends one JSON line per sink:
//! - `spaces.perf.*` -> `<workspace>/.spaces/metrics/spaces.perf.jsonl`
//! - `spaces.usage.*` -> `<store>/metrics/spaces.usage.jsonl`
//!
//! The record schema, metric names, and paths are shared with the viewer in
//! `utils::metrics_log` (`spaces metrics`).
//!
//! Collection is controlled by `Feature::CollectMetrics`. Errors never fail a command.
//! `Feature::EnableRunMetrics` uses the same in-memory samples to show the longest running
//! tasks at the end of `run`, checkout, and `sync` commands (see `show_run_summary`).

use anyhow::Context;
use anyhow_source_location::format_context;
use metrics::{Counter, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit};
use metrics_util::registry::{AtomicStorage, Registry};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use utils::metrics_log::{CounterRecord, HistogramRecord, Record};
use utils::{features, fs_mutex, metrics_log, rcache, ws};

pub use utils::metrics_log::{
    PERF_COMMAND_DURATION, PERF_EVALUATE_DURATION, PERF_MODULE_DURATION, PERF_PREFIX,
    PERF_REPO_STEP_DURATION, PERF_RULE_DURATION, PERF_RULE_HASH_INPUTS_DURATION,
    PERF_STEP_DURATION, USAGE_COMMAND, USAGE_PREFIX, USAGE_RULE, USAGE_SEARCH, USAGE_TARGET,
};

type MetricsRegistry = Registry<Key, AtomicStorage>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum Status {
    Ok,
    Err,
}

/// What `spaces search` searched, used as the `scope` label of search metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum SearchScope {
    /// Rules in the current workspace (`spaces query search`).
    Rules,
    /// Checkout workflows/repos in `co.spaces.toml` (`spaces query-co search`).
    Workflows,
}

/// Counts each keyword passed to `spaces search`, `spaces query search`, or
/// `spaces query-co search`. Keywords are lowercased because matching is case-insensitive.
pub fn record_search(scope: SearchScope, keywords: &[Arc<str>]) {
    let scope_label: &'static str = scope.into();
    for keyword in keywords {
        metrics::counter!(
            USAGE_SEARCH,
            "scope" => scope_label,
            "keyword" => keyword.to_lowercase()
        )
        .increment(1);
    }
}

/// Label value for the `cache` label of rule metrics.
pub fn cache_status_label(cache_status: &rcache::CacheStatus) -> &'static str {
    match cache_status {
        rcache::CacheStatus::None => "none",
        rcache::CacheStatus::Skipped(_) => "skipped",
        rcache::CacheStatus::Executed(_) => "executed",
        rcache::CacheStatus::Restored(_) => "restored",
    }
}

/// Records the time from creation until drop, so early returns and errors are included.
#[must_use = "the elapsed time is recorded when the timer is dropped"]
pub struct Timer {
    histogram: Histogram,
    start_time: std::time::Instant,
}

impl Timer {
    pub fn new(histogram: Histogram) -> Self {
        Self {
            histogram,
            start_time: std::time::Instant::now(),
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.histogram.record(self.start_time.elapsed());
    }
}

/// Times a workspace-level step (e.g. `sync.build_plan`).
pub fn time_step(step: &'static str) -> Timer {
    Timer::new(metrics::histogram!(PERF_STEP_DURATION, "step" => step))
}

/// Times a step applied to a single repository.
pub fn time_repo_step(step: &'static str, repo: Arc<str>) -> Timer {
    Timer::new(metrics::histogram!(
        PERF_REPO_STEP_DURATION,
        "step" => step,
        "repo" => repo
    ))
}

/// Records the evaluation time of one starlark module. The module is labeled relative to
/// the workspace. `cache` is `hit`, `miss`, or `bypass` (module cache not used).
pub fn record_module(
    workspace_path: &str,
    module: &str,
    cache: &'static str,
    elapsed: std::time::Duration,
) {
    let module = module
        .strip_prefix(workspace_path)
        .and_then(|relative| relative.strip_prefix('/'))
        .unwrap_or(module)
        .to_string();
    metrics::histogram!(PERF_MODULE_DURATION, "module" => module, "cache" => cache).record(elapsed);
}

struct SpacesRecorder {
    registry: Arc<MetricsRegistry>,
}

impl SpacesRecorder {
    fn new() -> Self {
        Self {
            registry: Arc::new(Registry::atomic()),
        }
    }
}

impl Recorder for SpacesRecorder {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
        self.registry
            .get_or_create_counter(key, |handle| Counter::from_arc(handle.clone()))
    }

    fn register_gauge(&self, key: &Key, _: &Metadata<'_>) -> Gauge {
        self.registry
            .get_or_create_gauge(key, |handle| Gauge::from_arc(handle.clone()))
    }

    fn register_histogram(&self, key: &Key, _: &Metadata<'_>) -> Histogram {
        self.registry
            .get_or_create_histogram(key, |handle| Histogram::from_arc(handle.clone()))
    }
}

#[derive(Debug, Clone)]
struct WorkspaceBinding {
    workspace_path: Arc<str>,
    store_path: Arc<str>,
    is_enabled: bool,
    is_run_metrics_enabled: bool,
}

struct TelemetryState {
    registry: Arc<MetricsRegistry>,
    binding: Mutex<Option<WorkspaceBinding>>,
}

static TELEMETRY_STATE: OnceLock<TelemetryState> = OnceLock::new();

fn is_feature_disabled_by_env(feature: features::Feature) -> bool {
    std::env::var(feature.env_var_name()).is_ok_and(|value| !value.eq_ignore_ascii_case("ON"))
}

fn is_disabled_by_env() -> bool {
    is_feature_disabled_by_env(features::Feature::CollectMetrics)
        && is_feature_disabled_by_env(features::Feature::EnableRunMetrics)
}

/// Installs the global metrics recorder. Only env vars are known at this point, so the
/// recorder is skipped (macros become no-ops) only if both `SPACES_ENV_COLLECT_METRICS`
/// and `SPACES_ENV_ENABLE_RUN_METRICS` disable it.
pub fn init() {
    if TELEMETRY_STATE.get().is_some() || is_disabled_by_env() {
        return;
    }
    let recorder = SpacesRecorder::new();
    let registry = recorder.registry.clone();
    if metrics::set_global_recorder(recorder).is_err() {
        return;
    }
    let _ = TELEMETRY_STATE.set(TelemetryState {
        registry,
        binding: Mutex::new(None),
    });
}

/// Associates the collected metrics with a workspace. The last call wins.
pub fn bind_workspace(
    workspace_path: Arc<str>,
    store_path: Arc<str>,
    features: &features::Features,
) {
    let Some(state) = TELEMETRY_STATE.get() else {
        return;
    };
    *state.binding.lock().unwrap_or_else(PoisonError::into_inner) = Some(WorkspaceBinding {
        workspace_path,
        store_path,
        is_enabled: features.is_enabled(features::Feature::CollectMetrics),
        is_run_metrics_enabled: features.is_enabled(features::Feature::EnableRunMetrics),
    });
}

/// Shows the longest running tasks recorded so far if `enable-run-metrics` is enabled for
/// the bound workspace. Doesn't consume the samples, so call it before `finish()`.
pub fn show_run_summary(console: &console::Console, elapsed: std::time::Duration) {
    let Some(state) = TELEMETRY_STATE.get() else {
        return;
    };
    let is_run_metrics_enabled = state
        .binding
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|binding| binding.is_run_metrics_enabled);
    if !is_run_metrics_enabled {
        return;
    }
    let histograms = get_histograms(&state.registry);
    metrics_log::show_run_summary(console, &histograms, elapsed, console.is_tty());
}

/// Reads the histogram samples without draining them.
fn get_histograms(registry: &MetricsRegistry) -> Vec<HistogramRecord> {
    let mut histograms = Vec::new();
    registry.visit_histograms(|key, bucket| {
        let samples = bucket.data();
        if !samples.is_empty() {
            histograms.push(HistogramRecord {
                name: key.name().to_string(),
                labels: get_labels(key),
                samples,
            });
        }
    });
    histograms
}

#[derive(Debug, Default)]
pub struct FinishReport {
    pub written: Vec<PathBuf>,
    pub errors: Vec<anyhow::Error>,
}

/// Writes the collected metrics if collection is enabled. Returns `None` if metrics
/// are not being collected.
pub fn finish(command: &str, status: Status) -> Option<FinishReport> {
    let state = TELEMETRY_STATE.get()?;
    let binding = state
        .binding
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let sinks = resolve_sinks(binding.as_ref(), ws::get_checkout_store_path)?;
    Some(write_records(
        &state.registry,
        &sinks,
        command,
        status,
        get_timestamp_ms(),
    ))
}

fn get_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[derive(Debug)]
struct Sinks {
    workspace_path: Option<Arc<str>>,
    perf_path: Option<PathBuf>,
    usage_path: PathBuf,
}

fn get_usage_path(store_path: &str) -> PathBuf {
    metrics_log::get_usage_path(Path::new(store_path))
}

/// Decides whether metrics are enabled and where they go. Uses the workspace binding if
/// present, otherwise the features of the default store.
fn resolve_sinks(
    binding: Option<&WorkspaceBinding>,
    get_default_store_path: impl FnOnce() -> Arc<str>,
) -> Option<Sinks> {
    let Some(binding) = binding else {
        let store_path = get_default_store_path();
        let is_enabled = features::Features::new_from_json(Path::new(store_path.as_ref()))
            .is_ok_and(|features| features.is_enabled(features::Feature::CollectMetrics));
        return is_enabled.then(|| Sinks {
            workspace_path: None,
            perf_path: None,
            usage_path: get_usage_path(store_path.as_ref()),
        });
    };

    if !binding.is_enabled {
        return None;
    }

    // The workspace directory is removed when a checkout fails; don't recreate it.
    let workspace_path = Path::new(binding.workspace_path.as_ref());
    let perf_path = (workspace_path.is_dir()
        && workspace_path
            .join(crate::workspace::ENV_FILE_NAME)
            .is_file())
    .then(|| metrics_log::get_perf_path(workspace_path));

    Some(Sinks {
        workspace_path: Some(binding.workspace_path.clone()),
        perf_path,
        usage_path: get_usage_path(binding.store_path.as_ref()),
    })
}

#[derive(Debug, Default)]
struct Snapshot {
    counters: Vec<CounterRecord>,
    histograms: Vec<HistogramRecord>,
}

impl Snapshot {
    fn with_prefix(&self, prefix: &str) -> Snapshot {
        Snapshot {
            counters: self
                .counters
                .iter()
                .filter(|counter| counter.name.starts_with(prefix))
                .cloned()
                .collect(),
            histograms: self
                .histograms
                .iter()
                .filter(|histogram| histogram.name.starts_with(prefix))
                .cloned()
                .collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.counters.is_empty() && self.histograms.is_empty()
    }
}

fn get_labels(key: &Key) -> BTreeMap<String, String> {
    key.labels()
        .map(|label| (label.key().to_string(), label.value().to_string()))
        .collect()
}

/// Drains all counters and histogram samples from the registry.
fn take_snapshot(registry: &MetricsRegistry) -> Snapshot {
    let mut snapshot = Snapshot::default();
    registry.visit_counters(|key, counter| {
        let value = counter.swap(0, Ordering::Relaxed);
        if value > 0 {
            snapshot.counters.push(CounterRecord {
                name: key.name().to_string(),
                labels: get_labels(key),
                value,
            });
        }
    });
    registry.visit_histograms(|key, bucket| {
        let mut samples = Vec::new();
        bucket.clear_with(|block| samples.extend_from_slice(block));
        if !samples.is_empty() {
            snapshot.histograms.push(HistogramRecord {
                name: key.name().to_string(),
                labels: get_labels(key),
                samples,
            });
        }
    });
    snapshot
        .counters
        .sort_by(|a, b| (&a.name, &a.labels).cmp(&(&b.name, &b.labels)));
    snapshot
        .histograms
        .sort_by(|a, b| (&a.name, &a.labels).cmp(&(&b.name, &b.labels)));
    snapshot
}

fn write_records(
    registry: &MetricsRegistry,
    sinks: &Sinks,
    command: &str,
    status: Status,
    timestamp_ms: u64,
) -> FinishReport {
    let snapshot = take_snapshot(registry);
    let status: &'static str = status.into();

    let mut outputs = vec![(
        sinks.usage_path.as_path(),
        snapshot.with_prefix(USAGE_PREFIX),
    )];
    if let Some(perf_path) = sinks.perf_path.as_deref() {
        outputs.push((perf_path, snapshot.with_prefix(PERF_PREFIX)));
    }

    let mut report = FinishReport::default();
    for (path, snapshot) in outputs {
        if snapshot.is_empty() {
            continue;
        }
        let record = Record {
            timestamp_ms,
            spaces_version: env!("CARGO_PKG_VERSION").to_string(),
            command: command.to_string(),
            status: status.to_string(),
            workspace: sinks.workspace_path.as_ref().map(|path| path.to_string()),
            counters: snapshot.counters,
            histograms: snapshot.histograms,
        };
        match append_record(path, &record) {
            Ok(()) => report.written.push(path.to_path_buf()),
            Err(error) => report.errors.push(error),
        }
    }
    report
}

fn append_record(path: &Path, record: &Record) -> anyhow::Result<()> {
    let mut line = serde_json::to_string(record)
        .context(format_context!("Failed to serialize metrics record"))?;
    line.push('\n');

    let mut mutex = fs_mutex::FsMutex::new(path.into());
    mutex.with_lock(|path| {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create metrics directory {}",
                parent.display()
            ))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)
            .context(format_context!(
                "Failed to open metrics file {}",
                path.display()
            ))?;
        if file.metadata()?.len() > 0 {
            file.seek(SeekFrom::End(-1))?;
            let mut last_byte = [0];
            file.read_exact(&mut last_byte)?;
            if last_byte[0] != b'\n' {
                file.write_all(b"\n").context(format_context!(
                    "Failed to write metrics file {}",
                    path.display()
                ))?;
            }
        }
        file.write_all(line.as_bytes()).context(format_context!(
            "Failed to write metrics file {}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use utils::store;

    fn record_sample_metrics(recorder: &SpacesRecorder) {
        metrics::with_local_recorder(recorder, || {
            metrics::counter!(USAGE_COMMAND, "command" => "run", "status" => "ok").increment(1);
            metrics::counter!(USAGE_TARGET, "command" => "run", "target" => "//spaces:check")
                .increment(1);
            metrics::histogram!(PERF_RULE_DURATION, "rule" => "//spaces:check", "cache" => "executed")
                .record(std::time::Duration::from_millis(1500));
            metrics::histogram!(PERF_COMMAND_DURATION, "command" => "run").record(2.0);
        });
    }

    fn read_records(path: &Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn get_names(record: &serde_json::Value, kind: &str) -> Vec<String> {
        record[kind]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap().to_string())
            .collect()
    }

    fn new_binding(workspace: &Path, store: &Path, is_enabled: bool) -> WorkspaceBinding {
        WorkspaceBinding {
            workspace_path: workspace.to_string_lossy().into(),
            store_path: store.to_string_lossy().into(),
            is_enabled,
            is_run_metrics_enabled: false,
        }
    }

    fn new_workspace() -> tempfile::TempDir {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join(crate::workspace::ENV_FILE_NAME), "").unwrap();
        workspace
    }

    fn get_perf_path(workspace: &Path) -> PathBuf {
        metrics_log::get_perf_path(workspace)
    }

    #[test]
    fn test_written_records_round_trip_through_metrics_log() {
        let workspace = new_workspace();
        let store_dir = tempfile::tempdir().unwrap();
        let binding = new_binding(workspace.path(), store_dir.path(), true);
        let sinks = resolve_sinks(Some(&binding), || unreachable!()).unwrap();

        let recorder = SpacesRecorder::new();
        record_sample_metrics(&recorder);
        let report = write_records(&recorder.registry, &sinks, "run", Status::Ok, 42);
        assert!(report.errors.is_empty(), "{:?}", report.errors);

        let perf = metrics_log::read_records(&get_perf_path(workspace.path())).unwrap();
        assert_eq!(perf.skipped, 0);
        assert_eq!(perf.records.len(), 1);
        let record = &perf.records[0];
        assert_eq!(record.timestamp_ms, 42);
        assert_eq!(record.command, "run");
        assert_eq!(record.status, "ok");
        assert_eq!(
            record.workspace.as_deref(),
            Some(binding.workspace_path.as_ref())
        );
        assert_eq!(record.histograms.len(), 2);
        assert_eq!(record.histograms[1].name, PERF_RULE_DURATION);
        assert_eq!(record.histograms[1].samples, vec![1.5]);

        let usage = metrics_log::read_records(&get_usage_path(&binding.store_path)).unwrap();
        assert_eq!(usage.records.len(), 1);
        assert_eq!(usage.records[0].counters.len(), 2);
        assert_eq!(usage.records[0].counters[0].name, USAGE_COMMAND);

        // The viewer selects the written samples by short metric name.
        let metric = metrics_log::Metric::parse("perf.rule").unwrap();
        let selection = metrics_log::select(
            &perf.records,
            &metric,
            metric.kind(&perf.records),
            metric.default_group_by(),
            &metrics_log::Filter::default(),
            false,
            None,
        );
        assert_eq!(selection.groups["//spaces:check"][0].value, 1.5);
    }

    #[test]
    fn test_metrics_are_partitioned_by_prefix() {
        let workspace = new_workspace();
        let store_dir = tempfile::tempdir().unwrap();
        let binding = new_binding(workspace.path(), store_dir.path(), true);
        let sinks = resolve_sinks(Some(&binding), || unreachable!()).unwrap();

        let recorder = SpacesRecorder::new();
        record_sample_metrics(&recorder);
        let report = write_records(&recorder.registry, &sinks, "run", Status::Ok, 1);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.written.len(), 2);

        let usage = read_records(&get_usage_path(&binding.store_path));
        assert_eq!(usage.len(), 1);
        assert_eq!(
            get_names(&usage[0], "counters"),
            vec![USAGE_COMMAND, USAGE_TARGET]
        );
        assert!(get_names(&usage[0], "histograms").is_empty());

        let perf = read_records(&get_perf_path(workspace.path()));
        assert_eq!(perf.len(), 1);
        assert!(get_names(&perf[0], "counters").is_empty());
        assert_eq!(
            get_names(&perf[0], "histograms"),
            vec![PERF_COMMAND_DURATION, PERF_RULE_DURATION]
        );
        let rule = &perf[0]["histograms"][1];
        assert_eq!(rule["labels"]["rule"], "//spaces:check");
        assert_eq!(rule["labels"]["cache"], "executed");
        assert_eq!(rule["samples"][0], 1.5);
    }

    #[test]
    fn test_append_after_partial_line_starts_new_record() {
        let workspace = new_workspace();
        let store_dir = tempfile::tempdir().unwrap();
        let binding = new_binding(workspace.path(), store_dir.path(), true);
        let sinks = resolve_sinks(Some(&binding), || unreachable!()).unwrap();

        let perf_path = sinks.perf_path.as_ref().unwrap();
        for path in [&sinks.usage_path, perf_path] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"{\"interrupted\":").unwrap();
        }

        let recorder = SpacesRecorder::new();
        record_sample_metrics(&recorder);
        let report = write_records(&recorder.registry, &sinks, "run", Status::Ok, 1);
        assert!(report.errors.is_empty(), "{:?}", report.errors);

        for path in [&sinks.usage_path, perf_path] {
            let records = metrics_log::read_records(path).unwrap();
            assert_eq!(records.skipped, 1);
            assert_eq!(records.records.len(), 1);
            assert_eq!(records.records[0].command, "run");
        }
    }

    #[test]
    fn test_finish_twice_appends_two_lines() {
        let workspace = new_workspace();
        let store_dir = tempfile::tempdir().unwrap();
        let binding = new_binding(workspace.path(), store_dir.path(), true);
        let sinks = resolve_sinks(Some(&binding), || unreachable!()).unwrap();

        let recorder = SpacesRecorder::new();
        record_sample_metrics(&recorder);
        write_records(&recorder.registry, &sinks, "run", Status::Ok, 1);
        record_sample_metrics(&recorder);
        write_records(&recorder.registry, &sinks, "inspect", Status::Err, 2);

        for path in [
            get_usage_path(&binding.store_path),
            get_perf_path(workspace.path()),
        ] {
            let records = read_records(&path);
            assert_eq!(records.len(), 2);
            for (record, (timestamp, command, status)) in records
                .iter()
                .zip([(1, "run", "ok"), (2, "inspect", "err")])
            {
                assert_eq!(record["timestamp_ms"], timestamp);
                assert_eq!(record["spaces_version"], env!("CARGO_PKG_VERSION"));
                assert_eq!(record["command"], command);
                assert_eq!(record["status"], status);
                assert_eq!(record["workspace"], binding.workspace_path.as_ref());
                // samples are drained by each finish
                let parsed: Record = serde_json::from_value(record.clone()).unwrap();
                assert!(!parsed.counters.is_empty() || !parsed.histograms.is_empty());
                assert!(parsed.counters.iter().all(|counter| counter.value == 1));
                assert!(
                    parsed
                        .histograms
                        .iter()
                        .all(|histogram| histogram.samples.len() == 1)
                );
            }
        }
    }

    #[test]
    fn test_no_workspace_writes_only_usage() {
        let store_dir = tempfile::tempdir().unwrap();
        let mut features = features::Features::new();
        features.enable(features::Feature::CollectMetrics);
        features.save(store_dir.path()).unwrap();

        let store_path: Arc<str> = store_dir.path().to_string_lossy().into();
        let sinks = resolve_sinks(None, || store_path.clone()).unwrap();
        assert!(sinks.perf_path.is_none());

        let recorder = SpacesRecorder::new();
        record_sample_metrics(&recorder);
        let report = write_records(&recorder.registry, &sinks, "store", Status::Ok, 1);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.written, vec![get_usage_path(&store_path)]);

        let usage = read_records(&get_usage_path(&store_path));
        assert_eq!(usage.len(), 1);
        assert!(usage[0]["workspace"].is_null());
    }

    #[test]
    fn test_disabled_writes_nothing() {
        let workspace = tempfile::tempdir().unwrap();
        let store_dir = tempfile::tempdir().unwrap();

        let binding = new_binding(workspace.path(), store_dir.path(), false);
        assert!(resolve_sinks(Some(&binding), || unreachable!()).is_none());

        // Default store without the feature enabled
        let store_path: Arc<str> = store_dir.path().to_string_lossy().into();
        assert!(resolve_sinks(None, || store_path.clone()).is_none());

        assert!(!workspace.path().join(ws::METRICS_DIR_NAME).exists());
        assert!(!store_dir.path().join(store::SPACES_STORE_METRICS).exists());
    }

    #[test]
    fn test_workspace_marker_controls_perf_sink() {
        let store_dir = tempfile::tempdir().unwrap();
        let workspace = store_dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let binding = new_binding(&workspace, store_dir.path(), true);

        // A checkout failure may leave the original directory but removes its contents.
        let cleaned_up_sinks = resolve_sinks(Some(&binding), || unreachable!()).unwrap();
        assert!(cleaned_up_sinks.perf_path.is_none());

        std::fs::write(workspace.join(crate::workspace::ENV_FILE_NAME), "").unwrap();
        let valid_workspace_sinks = resolve_sinks(Some(&binding), || unreachable!()).unwrap();
        assert_eq!(
            valid_workspace_sinks.perf_path,
            Some(get_perf_path(&workspace))
        );
    }

    #[test]
    fn test_timers_record_on_drop() {
        let recorder = SpacesRecorder::new();
        metrics::with_local_recorder(&recorder, || {
            let _step_timer = time_step("sync.build_plan");
            let _repo_timer = time_repo_step("sync.fetch", "repo-a".into());
        });

        let snapshot = take_snapshot(&recorder.registry);
        assert_eq!(snapshot.histograms.len(), 2);

        let repo_step = &snapshot.histograms[0];
        assert_eq!(repo_step.name, PERF_REPO_STEP_DURATION);
        assert_eq!(repo_step.labels["step"], "sync.fetch");
        assert_eq!(repo_step.labels["repo"], "repo-a");
        assert_eq!(repo_step.samples.len(), 1);

        let step = &snapshot.histograms[1];
        assert_eq!(step.name, PERF_STEP_DURATION);
        assert_eq!(step.labels["step"], "sync.build_plan");
        assert_eq!(step.samples.len(), 1);
    }

    #[test]
    fn test_run_summary_histograms_do_not_drain() {
        let recorder = SpacesRecorder::new();
        metrics::with_local_recorder(&recorder, || {
            let _step_timer = time_step("graph.execute");
        });

        let histograms = get_histograms(&recorder.registry);
        assert_eq!(histograms.len(), 1);
        assert_eq!(histograms[0].name, PERF_STEP_DURATION);
        assert_eq!(histograms[0].labels["step"], "graph.execute");

        // the samples are still available for `finish()`
        let snapshot = take_snapshot(&recorder.registry);
        assert_eq!(snapshot.histograms, histograms);
    }

    #[test]
    fn test_record_search_counts_keywords_by_scope() {
        let recorder = SpacesRecorder::new();
        metrics::with_local_recorder(&recorder, || {
            record_search(SearchScope::Rules, &["build".into(), "check".into()]);
            record_search(SearchScope::Rules, &["Build".into()]);
            record_search(SearchScope::Workflows, &["build".into()]);
        });

        let snapshot = take_snapshot(&recorder.registry);
        let counts: Vec<_> = snapshot
            .counters
            .iter()
            .map(|counter| {
                assert_eq!(counter.name, USAGE_SEARCH);
                (
                    counter.labels["scope"].as_str(),
                    counter.labels["keyword"].as_str(),
                    counter.value,
                )
            })
            .collect();
        assert_eq!(
            counts,
            vec![
                ("rules", "build", 2),
                ("workflows", "build", 1),
                ("rules", "check", 1),
            ]
        );
    }

    #[test]
    fn test_cache_status_label() {
        assert_eq!(cache_status_label(&rcache::CacheStatus::None), "none");
        assert_eq!(
            cache_status_label(&rcache::CacheStatus::Restored("digest".into())),
            "restored"
        );
    }
}
