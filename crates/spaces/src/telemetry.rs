//! Metrics collection for spaces.
//!
//! A single in-memory recorder collects all `metrics` samples during the process. At exit,
//! `finish()` partitions them by name prefix and appends one JSON line per sink:
//! - `spaces.perf.*` -> `<workspace>/.spaces/metrics/perf.jsonl`
//! - `spaces.usage.*` -> `<store>/metrics/usage.jsonl`
//!
//! Collection is controlled by `Feature::CollectMetrics`. Errors never fail a command.

use anyhow::Context;
use anyhow_source_location::format_context;
use metrics::{Counter, Gauge, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit};
use metrics_util::registry::{AtomicStorage, Registry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use utils::{features, fs_mutex, rcache, store, ws};

pub const PERF_PREFIX: &str = "spaces.perf.";
pub const USAGE_PREFIX: &str = "spaces.usage.";

pub const PERF_COMMAND_DURATION: &str = "spaces.perf.command.duration_seconds";
pub const PERF_RULE_DURATION: &str = "spaces.perf.rule.duration_seconds";
pub const PERF_EVALUATE_DURATION: &str = "spaces.perf.evaluate.duration_seconds";
pub const PERF_STEP_DURATION: &str = "spaces.perf.step.duration_seconds";
pub const PERF_REPO_STEP_DURATION: &str = "spaces.perf.repo_step.duration_seconds";
pub const USAGE_COMMAND: &str = "spaces.usage.command";
pub const USAGE_TARGET: &str = "spaces.usage.target";
pub const USAGE_RULE: &str = "spaces.usage.rule";
pub const USAGE_SEARCH: &str = "spaces.usage.search";

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
}

struct TelemetryState {
    registry: Arc<MetricsRegistry>,
    binding: Mutex<Option<WorkspaceBinding>>,
}

static TELEMETRY_STATE: OnceLock<TelemetryState> = OnceLock::new();

fn is_disabled_by_env() -> bool {
    std::env::var(features::Feature::CollectMetrics.env_var_name())
        .is_ok_and(|value| !value.eq_ignore_ascii_case("ON"))
}

/// Installs the global metrics recorder. Only env vars are known at this point, so the
/// recorder is skipped (macros become no-ops) only if `SPACES_ENV_COLLECT_METRICS` disables it.
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
    });
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
    Path::new(store_path)
        .join(store::SPACES_STORE_METRICS)
        .join(store::USAGE_METRICS_FILE_NAME)
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
    let perf_path = workspace_path.is_dir().then(|| {
        workspace_path
            .join(ws::METRICS_DIR_NAME)
            .join(ws::PERF_METRICS_FILE_NAME)
    });

    Some(Sinks {
        workspace_path: Some(binding.workspace_path.clone()),
        perf_path,
        usage_path: get_usage_path(binding.store_path.as_ref()),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CounterRecord {
    name: String,
    labels: BTreeMap<String, String>,
    value: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct HistogramRecord {
    name: String,
    labels: BTreeMap<String, String>,
    samples: Vec<f64>,
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

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    timestamp_ms: u64,
    spaces_version: String,
    command: String,
    status: String,
    workspace: Option<String>,
    counters: Vec<CounterRecord>,
    histograms: Vec<HistogramRecord>,
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
            .append(true)
            .open(path)
            .context(format_context!(
                "Failed to open metrics file {}",
                path.display()
            ))?;
        file.write_all(line.as_bytes()).context(format_context!(
            "Failed to write metrics file {}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
    }

    fn get_perf_path(workspace: &Path) -> PathBuf {
        workspace
            .join(ws::METRICS_DIR_NAME)
            .join(ws::PERF_METRICS_FILE_NAME)
    }

    #[test]
    fn test_metrics_are_partitioned_by_prefix() {
        let workspace = tempfile::tempdir().unwrap();
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
    fn test_finish_twice_appends_two_lines() {
        let workspace = tempfile::tempdir().unwrap();
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
    fn test_missing_workspace_skips_perf() {
        let store_dir = tempfile::tempdir().unwrap();
        let workspace = store_dir.path().join("removed-workspace");
        let binding = new_binding(&workspace, store_dir.path(), true);
        let sinks = resolve_sinks(Some(&binding), || unreachable!()).unwrap();
        assert!(sinks.perf_path.is_none());
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
