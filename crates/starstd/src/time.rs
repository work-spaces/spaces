use crate::is_lsp_mode;
use anyhow::{Context, bail};
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static TIMER_REGISTRY: OnceLock<Mutex<HashMap<u64, Instant>>> = OnceLock::new();
static NEXT_TIMER_ID: AtomicU64 = AtomicU64::new(1);
static MONOTONIC_START: OnceLock<Instant> = OnceLock::new();

fn timer_registry() -> &'static Mutex<HashMap<u64, Instant>> {
    TIMER_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn clamp_u128_to_u64(v: u128) -> u64 {
    if v > u64::MAX as u128 {
        u64::MAX
    } else {
        v as u64
    }
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Returns the current system time as a tuple.
    ///
    /// ```python
    /// (secs, nsec) = time.now()
    /// ```
    fn now() -> anyhow::Result<(u64, u32)> {
        if is_lsp_mode() {
            return Ok((0, 0));
        }
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context(format_context!("Failed to get current time"))?;
        Ok((current_time.as_secs(), current_time.subsec_nanos()))
    }

    /// Pauses execution for the specified number of nanoseconds.
    fn sleep(nanoseconds: u64) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        std::thread::sleep(Duration::from_nanos(nanoseconds));
        Ok(NoneType)
    }

    /// Pauses execution for the specified number of milliseconds.
    fn sleep_ms(milliseconds: u64) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        std::thread::sleep(Duration::from_millis(milliseconds));
        Ok(NoneType)
    }

    /// Pauses execution for the specified number of whole seconds.
    ///
    /// Note: uses integer seconds to match supported Starlark argument types.
    fn sleep_seconds(seconds: u64) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        std::thread::sleep(Duration::from_secs(seconds));
        Ok(NoneType)
    }

    /// Returns current unix timestamp in seconds.
    fn unix() -> anyhow::Result<u64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context(format_context!("Failed to get current unix time"))?;
        Ok(d.as_secs())
    }

    /// Returns current unix timestamp in milliseconds.
    fn unix_ms() -> anyhow::Result<u64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context(format_context!("Failed to get current unix time"))?;
        Ok(clamp_u128_to_u64(d.as_millis()))
    }

    /// Returns process-local monotonic milliseconds for duration measurement.
    fn monotonic_ms() -> anyhow::Result<u64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let start = MONOTONIC_START.get_or_init(Instant::now);
        Ok(clamp_u128_to_u64(start.elapsed().as_millis()))
    }

    /// Formats unix timestamp (seconds) using strftime format.
    fn format(secs: i64, fmt: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        use chrono::{DateTime, Utc};
        let dt: DateTime<Utc> = DateTime::from_timestamp(secs, 0)
            .context(format_context!("Invalid unix seconds value"))?;
        Ok(dt.format(fmt).to_string())
    }

    /// Parses a datetime string using strftime format and returns unix seconds.
    ///
    /// If timezone is omitted, UTC is assumed.
    fn parse(s: &str, fmt: &str) -> anyhow::Result<i64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};

        if let Ok(dt) = chrono::DateTime::parse_from_str(s, fmt) {
            return Ok(dt.timestamp());
        }

        if let Ok(naive_dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(Utc.from_utc_datetime(&naive_dt).timestamp());
        }

        if let Ok(naive_date) = NaiveDate::parse_from_str(s, fmt) {
            let naive_dt = naive_date
                .and_hms_opt(0, 0, 0)
                .context(format_context!("Failed to construct midnight datetime"))?;
            return Ok(Utc.from_utc_datetime(&naive_dt).timestamp());
        }

        bail!("Failed to parse datetime with format: {}", fmt);
    }

    /// Returns current UTC time as ISO8601 / RFC3339 string.
    fn iso8601() -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        use chrono::Utc;
        Ok(Utc::now().to_rfc3339())
    }

    /// Creates a timer handle and returns its integer id.
    ///
    /// Example:
    ///   h = time.timer()
    ///   # ... work ...
    ///   ms = time.timer_elapsed_ms(h)
    fn timer() -> anyhow::Result<u64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let id = NEXT_TIMER_ID.fetch_add(1, Ordering::Relaxed);
        let mut map = timer_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("Timer registry lock poisoned"))?;
        map.insert(id, Instant::now());
        Ok(id)
    }

    /// Returns elapsed milliseconds for a timer handle.
    fn timer_elapsed_ms(timer_id: u64) -> anyhow::Result<u64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let map = timer_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("Timer registry lock poisoned"))?;
        let started = map
            .get(&timer_id)
            .context(format_context!("Invalid timer handle: {}", timer_id))?;
        Ok(clamp_u128_to_u64(started.elapsed().as_millis()))
    }

    /// Returns elapsed nanoseconds for a timer handle.
    fn timer_elapsed_ns(timer_id: u64) -> anyhow::Result<u64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let map = timer_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("Timer registry lock poisoned"))?;
        let started = map
            .get(&timer_id)
            .context(format_context!("Invalid timer handle: {}", timer_id))?;
        Ok(clamp_u128_to_u64(started.elapsed().as_nanos()))
    }

    /// Resets a timer handle to start counting from now.
    fn timer_reset(timer_id: u64) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let mut map = timer_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("Timer registry lock poisoned"))?;
        let started = map
            .get_mut(&timer_id)
            .context(format_context!("Invalid timer handle: {}", timer_id))?;
        *started = Instant::now();
        Ok(NoneType)
    }

    /// Removes a timer handle from the registry.
    fn timer_drop(timer_id: u64) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let mut map = timer_registry()
            .lock()
            .map_err(|_| anyhow::anyhow!("Timer registry lock poisoned"))?;
        if map.remove(&timer_id).is_none() {
            bail!("Invalid timer handle: {}", timer_id);
        }
        Ok(NoneType)
    }
}
