#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum ModuleCacheStatus {
    Hit,
    Miss,
    Bypass,
}

#[cfg(feature = "evaluation-profiling")]
mod imp {
    use super::ModuleCacheStatus;
    use crate::task;
    use anyhow::Context;
    use anyhow_source_location::format_context;
    use serde::Serialize;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant, SystemTime};

    #[derive(Debug, Clone)]
    struct BuiltinStats {
        count: u64,
        total_ns: u128,
        max_ns: u128,
        error_count: u64,
    }

    impl BuiltinStats {
        fn add_call(&mut self, elapsed: Duration, is_error: bool) {
            let elapsed_ns = elapsed.as_nanos();
            self.count += 1;
            self.total_ns += elapsed_ns;
            self.max_ns = self.max_ns.max(elapsed_ns);
            if is_error {
                self.error_count += 1;
            }
        }
    }

    #[derive(Debug, Clone)]
    struct ModuleRecordInternal {
        module: String,
        phase: String,
        thread: String,
        cache_status: ModuleCacheStatus,
        cache_bypass_reason: Option<String>,
        queue_wait_ns: Option<u128>,
        load_ns: u128,
        parse_ns: u128,
        eval_ns: u128,
        total_ns: u128,
        error: Option<String>,
        builtins: HashMap<(String, String), BuiltinStats>,
    }

    #[derive(Debug, Default)]
    struct SessionState {
        modules: Vec<ModuleRecordInternal>,
        builtins_summary: HashMap<(String, String), BuiltinStats>,
        cache_hit: u64,
        cache_miss: u64,
        cache_bypass: u64,
    }

    #[derive(Debug)]
    struct SessionInner {
        phase: String,
        workspace_path: Arc<str>,
        started_at: SystemTime,
        started: Instant,
        state: Mutex<SessionState>,
    }

    #[derive(Clone)]
    struct EvaluationProfileSession {
        inner: Arc<SessionInner>,
    }

    #[derive(Debug)]
    struct ModuleStartMetadata {
        cache_status: ModuleCacheStatus,
        cache_bypass_reason: Option<String>,
        queue_wait: Option<Duration>,
    }

    #[derive(Debug)]
    struct ActiveModuleProfiler {
        module: String,
        phase: String,
        thread: String,
        cache_status: ModuleCacheStatus,
        cache_bypass_reason: Option<String>,
        queue_wait_ns: Option<u128>,
        started: Instant,
        load_ns: u128,
        parse_ns: u128,
        eval_ns: u128,
        error: Option<String>,
        builtins: HashMap<(String, String), BuiltinStats>,
    }

    impl ActiveModuleProfiler {
        fn new(module: &str, phase: task::Phase, metadata: Option<ModuleStartMetadata>) -> Self {
            let (cache_status, cache_bypass_reason, queue_wait_ns) =
                if let Some(metadata) = metadata {
                    (
                        metadata.cache_status,
                        metadata.cache_bypass_reason,
                        metadata.queue_wait.map(|d| d.as_nanos()),
                    )
                } else {
                    (
                        ModuleCacheStatus::Bypass,
                        Some("no-cache-path".to_string()),
                        None,
                    )
                };

            Self {
                module: module.to_string(),
                phase: phase.to_string(),
                thread: format!("{:?}", std::thread::current().id()),
                cache_status,
                cache_bypass_reason,
                queue_wait_ns,
                started: Instant::now(),
                load_ns: 0,
                parse_ns: 0,
                eval_ns: 0,
                error: None,
                builtins: HashMap::new(),
            }
        }

        fn finish(self) -> ModuleRecordInternal {
            ModuleRecordInternal {
                module: self.module,
                phase: self.phase,
                thread: self.thread,
                cache_status: self.cache_status,
                cache_bypass_reason: self.cache_bypass_reason,
                queue_wait_ns: self.queue_wait_ns,
                load_ns: self.load_ns,
                parse_ns: self.parse_ns,
                eval_ns: self.eval_ns,
                total_ns: self.started.elapsed().as_nanos(),
                error: self.error,
                builtins: self.builtins,
            }
        }
    }

    fn active_session_cell() -> &'static Mutex<Option<EvaluationProfileSession>> {
        static ACTIVE_SESSION: OnceLock<Mutex<Option<EvaluationProfileSession>>> = OnceLock::new();
        ACTIVE_SESSION.get_or_init(|| Mutex::new(None))
    }

    thread_local! {
        static NEXT_MODULE_METADATA: RefCell<Option<ModuleStartMetadata>> = const { RefCell::new(None) };
        static CURRENT_MODULE: RefCell<Option<ActiveModuleProfiler>> = const { RefCell::new(None) };
    }

    fn active_session() -> Option<EvaluationProfileSession> {
        active_session_cell()
            .lock()
            .ok()
            .and_then(|lock| lock.clone())
    }

    impl SessionInner {
        fn merge_module(&self, module: ModuleRecordInternal) {
            let mut state = match self.state.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };

            match module.cache_status {
                ModuleCacheStatus::Hit => state.cache_hit += 1,
                ModuleCacheStatus::Miss => state.cache_miss += 1,
                ModuleCacheStatus::Bypass => state.cache_bypass += 1,
            }

            for ((namespace, function), builtin) in &module.builtins {
                let entry = state
                    .builtins_summary
                    .entry((namespace.clone(), function.clone()))
                    .or_insert(BuiltinStats {
                        count: 0,
                        total_ns: 0,
                        max_ns: 0,
                        error_count: 0,
                    });
                entry.count += builtin.count;
                entry.total_ns += builtin.total_ns;
                entry.max_ns = entry.max_ns.max(builtin.max_ns);
                entry.error_count += builtin.error_count;
            }

            state.modules.push(module);
        }

        fn write_artifact(&self) -> anyhow::Result<()> {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to lock evaluation profile state"))?;

            let mut modules: Vec<ModuleProfileJson> = state
                .modules
                .iter()
                .map(|m| ModuleProfileJson {
                    module: m.module.clone(),
                    phase: m.phase.clone(),
                    thread: m.thread.clone(),
                    cache_status: cache_status_as_str(m.cache_status).to_string(),
                    cache_bypass_reason: m.cache_bypass_reason.clone(),
                    durations_ms: ModuleDurationsMs {
                        queue_wait: m.queue_wait_ns.map(ns_to_ms),
                        load: ns_to_ms(m.load_ns),
                        parse: ns_to_ms(m.parse_ns),
                        eval: ns_to_ms(m.eval_ns),
                        total: ns_to_ms(m.total_ns),
                    },
                    error: m.error.clone(),
                    builtins: builtins_to_json(&m.builtins),
                })
                .collect();

            modules.sort_by(|a, b| {
                b.durations_ms
                    .total
                    .partial_cmp(&a.durations_ms.total)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let mut builtins_summary = builtins_to_json(&state.builtins_summary);
            builtins_summary.sort_by(|a, b| {
                b.total_ms
                    .partial_cmp(&a.total_ms)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let ended_at = SystemTime::now();
            let profile = EvaluationProfileJson {
                schema_version: 1,
                phase: self.phase.clone(),
                workspace_path: self.workspace_path.to_string(),
                started_at: chrono::DateTime::<chrono::Utc>::from(self.started_at).to_rfc3339(),
                ended_at: chrono::DateTime::<chrono::Utc>::from(ended_at).to_rfc3339(),
                total_duration_ms: ns_to_ms(self.started.elapsed().as_nanos()),
                cache: CacheSummaryJson {
                    hit: state.cache_hit,
                    miss: state.cache_miss,
                    bypass: state.cache_bypass,
                },
                modules,
                builtins_summary,
            };

            let output_path = std::path::PathBuf::from(self.workspace_path.as_ref())
                .join(".spaces/evaluation-profile.spaces.json");

            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format_context!(
                        "Failed to create evaluation profile directory {}",
                        parent.display()
                    )
                })?;
            }

            let content = serde_json::to_string_pretty(&profile)
                .context(format_context!("Failed to serialize evaluation profile"))?;

            std::fs::write(&output_path, content).with_context(|| {
                format_context!(
                    "Failed to write evaluation profile artifact {}",
                    output_path.display()
                )
            })?;

            Ok(())
        }
    }

    fn cache_status_as_str(status: ModuleCacheStatus) -> &'static str {
        match status {
            ModuleCacheStatus::Hit => "hit",
            ModuleCacheStatus::Miss => "miss",
            ModuleCacheStatus::Bypass => "bypass",
        }
    }

    fn ns_to_ms(ns: u128) -> f64 {
        ns as f64 / 1_000_000.0
    }

    fn builtins_to_json(
        map: &HashMap<(String, String), BuiltinStats>,
    ) -> Vec<BuiltinAggregateJson> {
        let mut result: Vec<BuiltinAggregateJson> = map
            .iter()
            .map(|((namespace, function), stats)| BuiltinAggregateJson {
                namespace: namespace.clone(),
                function: function.clone(),
                count: stats.count,
                total_ms: ns_to_ms(stats.total_ns),
                max_ms: ns_to_ms(stats.max_ns),
                error_count: stats.error_count,
            })
            .collect();

        result.sort_by(|a, b| {
            b.total_ms
                .partial_cmp(&a.total_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        result
    }

    #[derive(Serialize)]
    struct EvaluationProfileJson {
        schema_version: u32,
        phase: String,
        workspace_path: String,
        started_at: String,
        ended_at: String,
        total_duration_ms: f64,
        cache: CacheSummaryJson,
        modules: Vec<ModuleProfileJson>,
        builtins_summary: Vec<BuiltinAggregateJson>,
    }

    #[derive(Serialize)]
    struct CacheSummaryJson {
        hit: u64,
        miss: u64,
        bypass: u64,
    }

    #[derive(Serialize)]
    struct ModuleProfileJson {
        module: String,
        phase: String,
        thread: String,
        cache_status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_bypass_reason: Option<String>,
        durations_ms: ModuleDurationsMs,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        builtins: Vec<BuiltinAggregateJson>,
    }

    #[derive(Serialize)]
    struct ModuleDurationsMs {
        #[serde(skip_serializing_if = "Option::is_none")]
        queue_wait: Option<f64>,
        load: f64,
        parse: f64,
        eval: f64,
        total: f64,
    }

    #[derive(Serialize)]
    struct BuiltinAggregateJson {
        namespace: String,
        function: String,
        count: u64,
        total_ms: f64,
        max_ms: f64,
        error_count: u64,
    }

    pub struct EvaluationProfileRunGuard {
        session: Option<EvaluationProfileSession>,
    }

    impl Drop for EvaluationProfileRunGuard {
        fn drop(&mut self) {
            let Some(session) = self.session.take() else {
                return;
            };

            if let Ok(mut active) = active_session_cell().lock()
                && active
                    .as_ref()
                    .map(|current| Arc::ptr_eq(&current.inner, &session.inner))
                    .unwrap_or(false)
            {
                *active = None;
            }

            if let Err(error) = session.inner.write_artifact() {
                eprintln!("Failed to write evaluation profile artifact: {error:#}");
            }
        }
    }

    pub struct ModuleProfileGuard {
        session: Option<EvaluationProfileSession>,
        previous: Option<ActiveModuleProfiler>,
    }

    impl Drop for ModuleProfileGuard {
        fn drop(&mut self) {
            let Some(session) = self.session.take() else {
                return;
            };

            let mut previous = self.previous.take();
            let completed = CURRENT_MODULE.with(|slot| {
                let mut slot = slot.borrow_mut();
                let completed = slot.take();
                *slot = previous.take();
                completed
            });

            if let Some(completed) = completed {
                session.inner.merge_module(completed.finish());
            }
        }
    }

    pub enum TimedSection {
        Load,
        Parse,
        Eval,
    }

    pub struct CurrentModuleSectionTimer {
        section: TimedSection,
        started: Instant,
    }

    impl Drop for CurrentModuleSectionTimer {
        fn drop(&mut self) {
            let elapsed_ns = self.started.elapsed().as_nanos();
            with_current_module_mut(|module| match self.section {
                TimedSection::Load => module.load_ns += elapsed_ns,
                TimedSection::Parse => module.parse_ns += elapsed_ns,
                TimedSection::Eval => module.eval_ns += elapsed_ns,
            });
        }
    }

    fn with_current_module_mut(f: impl FnOnce(&mut ActiveModuleProfiler)) {
        CURRENT_MODULE.with(|slot| {
            if let Some(module) = slot.borrow_mut().as_mut() {
                f(module);
            }
        });
    }

    pub fn begin_session(
        phase: task::Phase,
        workspace_path: Arc<str>,
    ) -> EvaluationProfileRunGuard {
        let session = EvaluationProfileSession {
            inner: Arc::new(SessionInner {
                phase: phase.to_string(),
                workspace_path,
                started_at: SystemTime::now(),
                started: Instant::now(),
                state: Mutex::new(SessionState::default()),
            }),
        };

        if let Ok(mut active) = active_session_cell().lock() {
            *active = Some(session.clone());
        }

        EvaluationProfileRunGuard {
            session: Some(session),
        }
    }

    pub fn set_next_module_metadata(
        cache_status: ModuleCacheStatus,
        cache_bypass_reason: Option<String>,
        queue_wait: Option<Duration>,
    ) {
        NEXT_MODULE_METADATA.with(|slot| {
            *slot.borrow_mut() = Some(ModuleStartMetadata {
                cache_status,
                cache_bypass_reason,
                queue_wait,
            });
        });
    }

    pub fn enter_module(module: &str, phase: task::Phase) -> ModuleProfileGuard {
        let metadata = NEXT_MODULE_METADATA.with(|slot| slot.borrow_mut().take());
        let Some(session) = active_session() else {
            return ModuleProfileGuard {
                session: None,
                previous: None,
            };
        };

        let current = ActiveModuleProfiler::new(module, phase, metadata);
        let previous = CURRENT_MODULE.with(|slot| {
            let mut slot = slot.borrow_mut();
            let previous = slot.take();
            *slot = Some(current);
            previous
        });

        ModuleProfileGuard {
            session: Some(session),
            previous,
        }
    }

    pub fn start_load_timer() -> CurrentModuleSectionTimer {
        CurrentModuleSectionTimer {
            section: TimedSection::Load,
            started: Instant::now(),
        }
    }

    pub fn start_parse_timer() -> CurrentModuleSectionTimer {
        CurrentModuleSectionTimer {
            section: TimedSection::Parse,
            started: Instant::now(),
        }
    }

    pub fn start_eval_timer() -> CurrentModuleSectionTimer {
        CurrentModuleSectionTimer {
            section: TimedSection::Eval,
            started: Instant::now(),
        }
    }

    pub fn record_current_module_error(error: String) {
        with_current_module_mut(|module| {
            module.error = Some(error);
        });
    }

    pub fn record_cache_hit_module(
        module: &str,
        phase: task::Phase,
        queue_wait: Option<Duration>,
        total_duration: Duration,
    ) {
        let Some(session) = active_session() else {
            return;
        };

        session.inner.merge_module(ModuleRecordInternal {
            module: module.to_string(),
            phase: phase.to_string(),
            thread: format!("{:?}", std::thread::current().id()),
            cache_status: ModuleCacheStatus::Hit,
            cache_bypass_reason: None,
            queue_wait_ns: queue_wait.map(|d| d.as_nanos()),
            load_ns: 0,
            parse_ns: 0,
            eval_ns: 0,
            total_ns: total_duration.as_nanos(),
            error: None,
            builtins: HashMap::new(),
        });
    }

    pub fn profile_builtin_call<T, F>(namespace: &str, function: &str, body: F) -> anyhow::Result<T>
    where
        F: FnOnce() -> anyhow::Result<T>,
    {
        let started = Instant::now();
        let result = body();
        let is_error = result.is_err();

        with_current_module_mut(|module| {
            let entry = module
                .builtins
                .entry((namespace.to_string(), function.to_string()))
                .or_insert(BuiltinStats {
                    count: 0,
                    total_ns: 0,
                    max_ns: 0,
                    error_count: 0,
                });
            entry.add_call(started.elapsed(), is_error);
        });

        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn builtin_stats_aggregate_correctly() {
            let mut stats = BuiltinStats {
                count: 0,
                total_ns: 0,
                max_ns: 0,
                error_count: 0,
            };

            stats.add_call(Duration::from_millis(2), false);
            stats.add_call(Duration::from_millis(5), true);

            assert_eq!(stats.count, 2);
            assert_eq!(stats.error_count, 1);
            assert!(stats.total_ns >= Duration::from_millis(7).as_nanos());
            assert!(stats.max_ns >= Duration::from_millis(5).as_nanos());
        }

        #[test]
        fn profile_json_has_stable_schema_version() {
            let json = EvaluationProfileJson {
                schema_version: 1,
                phase: "Run".to_string(),
                workspace_path: "/tmp/ws".to_string(),
                started_at: "2026-06-20T00:00:00Z".to_string(),
                ended_at: "2026-06-20T00:00:01Z".to_string(),
                total_duration_ms: 1.0,
                cache: CacheSummaryJson {
                    hit: 1,
                    miss: 2,
                    bypass: 3,
                },
                modules: vec![],
                builtins_summary: vec![],
            };

            let value = serde_json::to_value(json).unwrap();
            assert_eq!(value["schema_version"], serde_json::json!(1));
            assert!(value.get("cache").is_some());
        }

        #[test]
        fn merge_module_is_thread_safe() {
            let session = Arc::new(SessionInner {
                phase: "Run".to_string(),
                workspace_path: Arc::from("."),
                started_at: SystemTime::now(),
                started: Instant::now(),
                state: Mutex::new(SessionState::default()),
            });

            let mut handles = Vec::new();
            for i in 0..8 {
                let session = session.clone();
                handles.push(std::thread::spawn(move || {
                    session.merge_module(ModuleRecordInternal {
                        module: format!("m{i}"),
                        phase: "Run".to_string(),
                        thread: "t".to_string(),
                        cache_status: ModuleCacheStatus::Miss,
                        cache_bypass_reason: None,
                        queue_wait_ns: None,
                        load_ns: 1,
                        parse_ns: 2,
                        eval_ns: 3,
                        total_ns: 6,
                        error: None,
                        builtins: HashMap::new(),
                    });
                }));
            }

            for handle in handles {
                handle.join().unwrap();
            }

            let state = session.state.lock().unwrap();
            assert_eq!(state.modules.len(), 8);
            assert_eq!(state.cache_miss, 8);
        }
    }
}

#[cfg(not(feature = "evaluation-profiling"))]
mod imp {
    use super::ModuleCacheStatus;
    use crate::task;
    use std::sync::Arc;
    use std::time::Duration;

    pub struct EvaluationProfileRunGuard;

    pub struct ModuleProfileGuard;

    pub struct CurrentModuleSectionTimer;

    pub fn begin_session(
        _phase: task::Phase,
        _workspace_path: Arc<str>,
    ) -> EvaluationProfileRunGuard {
        EvaluationProfileRunGuard
    }

    pub fn set_next_module_metadata(
        _cache_status: ModuleCacheStatus,
        _cache_bypass_reason: Option<String>,
        _queue_wait: Option<Duration>,
    ) {
    }

    pub fn enter_module(_module: &str, _phase: task::Phase) -> ModuleProfileGuard {
        ModuleProfileGuard
    }

    pub fn start_load_timer() -> CurrentModuleSectionTimer {
        CurrentModuleSectionTimer
    }

    pub fn start_parse_timer() -> CurrentModuleSectionTimer {
        CurrentModuleSectionTimer
    }

    pub fn start_eval_timer() -> CurrentModuleSectionTimer {
        CurrentModuleSectionTimer
    }

    pub fn record_current_module_error(_error: String) {}

    pub fn record_cache_hit_module(
        _module: &str,
        _phase: task::Phase,
        _queue_wait: Option<Duration>,
        _total_duration: Duration,
    ) {
    }

    pub fn profile_builtin_call<T, F>(
        _namespace: &str,
        _function: &str,
        body: F,
    ) -> anyhow::Result<T>
    where
        F: FnOnce() -> anyhow::Result<T>,
    {
        body()
    }
}

pub use imp::*;
