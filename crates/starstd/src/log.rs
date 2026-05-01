use crate::is_lsp_mode;
use log::LevelFilter;
use serde_json::json;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

struct LogConfig {
    level: LevelFilter,
    initialized: bool,
}

static LOG_CONFIG: OnceLock<Mutex<LogConfig>> = OnceLock::new();

/// Tracks the active output format so the custom formatter closure can read it
/// without needing to hold `LOG_CONFIG`'s mutex (avoids potential deadlock inside
/// the env_logger format callback).
static USE_JSON_FORMAT: AtomicBool = AtomicBool::new(false);

fn get_config() -> &'static Mutex<LogConfig> {
    LOG_CONFIG.get_or_init(|| {
        Mutex::new(LogConfig {
            level: LevelFilter::Info,
            initialized: false,
        })
    })
}

/// Initialises env_logger on the first call, then **always** syncs
/// `log::set_max_level` with the current `config.level`.
///
/// Key design points:
/// - env_logger is registered with `filter_level(Trace)` so that the `log`
///   crate's own global max-level becomes the sole dynamic gate.  Calling
///   `log::set_max_level` is safe at any time and does not require
///   re-initialising the logger.
/// - The custom format closure reads `USE_JSON_FORMAT` on every call, giving
///   `set_format` immediate effect without touching the logger registration.
fn ensure_initialized() {
    let mut config = get_config().lock().unwrap();
    if !config.initialized {
        // Honour the SPACES_ENV_LOG environment variable for the initial level.
        if let Ok(env_log) = std::env::var("SPACES_ENV_LOG") {
            let level_filter = match env_log.to_lowercase().as_str() {
                "trace" => LevelFilter::Trace,
                "debug" => LevelFilter::Debug,
                "info" => LevelFilter::Info,
                "warn" => LevelFilter::Warn,
                "error" => LevelFilter::Error,
                "off" => LevelFilter::Off,
                _ => LevelFilter::Info,
            };
            config.level = level_filter;
        }

        // Register env_logger once with Trace so all records reach it;
        // dynamic filtering is performed exclusively via log::set_max_level.
        env_logger::Builder::new()
            .filter_level(LevelFilter::Trace)
            .format_module_path(false)
            .format(|buf, record| {
                if USE_JSON_FORMAT.load(Ordering::Relaxed) {
                    let entry = json!({
                        "ts": buf.timestamp_millis().to_string(),
                        "level": record.level().to_string().to_lowercase(),
                        "msg": record.args().to_string(),
                    });
                    writeln!(buf, "{}", entry)
                } else {
                    writeln!(
                        buf,
                        "[{} {:5} {}] {}",
                        buf.timestamp_millis(),
                        record.level(),
                        record.target(),
                        record.args()
                    )
                }
            })
            .try_init()
            .ok();
        config.initialized = true;
    }

    // Always keep the global log filter in sync — this is what makes
    // set_level() work after the first log call.
    log::set_max_level(config.level);
}

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Set the log level.
    ///
    /// ```python
    /// log.set_level("trace")
    /// log.set_level("debug")
    /// log.set_level("info")
    /// log.set_level("warn")
    /// log.set_level("error")
    /// log.set_level("off")
    /// ```
    fn set_level(level: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let level_filter = match level.to_lowercase().as_str() {
            "trace" => LevelFilter::Trace,
            "debug" => LevelFilter::Debug,
            "info" => LevelFilter::Info,
            "warn" => LevelFilter::Warn,
            "error" => LevelFilter::Error,
            "off" => LevelFilter::Off,
            _ => return Err(anyhow::anyhow!("Invalid log level: {}", level)),
        };

        {
            let mut config = get_config().lock().unwrap();
            config.level = level_filter;
            // Do NOT reset config.initialized — re-initialising env_logger
            // would silently fail (it only accepts one global registration).
        }

        // ensure_initialized will call log::set_max_level with the new value.
        ensure_initialized();
        Ok(NoneType)
    }

    /// Set the log format.
    ///
    /// ```python
    /// log.set_format("text")  # human-readable (default)
    /// log.set_format("json")  # structured JSON
    /// ```
    fn set_format(format: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        match format.to_lowercase().as_str() {
            "text" => {
                USE_JSON_FORMAT.store(false, Ordering::Relaxed);
                Ok(NoneType)
            }
            "json" => {
                USE_JSON_FORMAT.store(true, Ordering::Relaxed);
                Ok(NoneType)
            }
            _ => Err(anyhow::anyhow!("Invalid log format: {}", format)),
        }
    }

    /// Log at trace level.
    ///
    /// ```python
    /// log.trace("Trace message")
    /// ```
    fn trace(message: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        ensure_initialized();
        log::trace!("{}", message);
        Ok(NoneType)
    }

    /// Log at debug level.
    ///
    /// ```python
    /// log.debug("Debug message")
    /// ```
    fn debug(message: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        ensure_initialized();
        log::debug!("{}", message);
        Ok(NoneType)
    }

    /// Log at info level.
    ///
    /// ```python
    /// log.info("Info message")
    /// ```
    fn info(message: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        ensure_initialized();
        log::info!("{}", message);
        Ok(NoneType)
    }

    /// Log at warn level.
    ///
    /// ```python
    /// log.warn("Warning message")
    /// ```
    fn warn(message: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        ensure_initialized();
        log::warn!("{}", message);
        Ok(NoneType)
    }

    /// Log at error level.
    ///
    /// ```python
    /// log.error("Error message")
    /// ```
    fn error(message: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        ensure_initialized();
        log::error!("{}", message);
        Ok(NoneType)
    }

    /// Log at error level and abort execution.
    ///
    /// ```python
    /// log.fatal("Fatal error message")
    /// ```
    fn fatal(message: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        ensure_initialized();
        log::error!("{}", message);
        std::process::exit(1);
    }
}
