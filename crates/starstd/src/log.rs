use crate::is_lsp_mode;
use log::LevelFilter;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use std::sync::{Mutex, OnceLock};

struct LogConfig {
    level: LevelFilter,
    format: String,
    initialized: bool,
}

static LOG_CONFIG: OnceLock<Mutex<LogConfig>> = OnceLock::new();

fn get_config() -> &'static Mutex<LogConfig> {
    LOG_CONFIG.get_or_init(|| {
        Mutex::new(LogConfig {
            level: LevelFilter::Info,
            format: "text".to_string(),
            initialized: false,
        })
    })
}

fn ensure_initialized() {
    let mut config = get_config().lock().unwrap();
    if !config.initialized {
        // Check SPACES_ENV_LOG environment variable
        if let Ok(env_log) = std::env::var("SPACES_ENV_LOG") {
            let level_filter = match env_log.to_lowercase().as_str() {
                "debug" => LevelFilter::Debug,
                "info" => LevelFilter::Info,
                "warn" => LevelFilter::Warn,
                "error" => LevelFilter::Error,
                "trace" => LevelFilter::Trace,
                "off" => LevelFilter::Off,
                _ => LevelFilter::Info, // default to Info if invalid
            };
            config.level = level_filter;
        }

        env_logger::Builder::new()
            .filter_level(config.level)
            .format_module_path(false)
            .format_timestamp_millis()
            .try_init()
            .ok();
        config.initialized = true;
    }
}

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Set the log level.
    ///
    /// ```python
    /// log.set_level("debug")
    /// log.set_level("info")
    /// log.set_level("warn")
    /// log.set_level("error")
    /// ```
    fn set_level(level: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let level_filter = match level.to_lowercase().as_str() {
            "debug" => LevelFilter::Debug,
            "info" => LevelFilter::Info,
            "warn" => LevelFilter::Warn,
            "error" => LevelFilter::Error,
            _ => return Err(anyhow::anyhow!("Invalid log level: {}", level)),
        };

        let mut config = get_config().lock().unwrap();
        config.level = level_filter;
        config.initialized = false;
        drop(config);

        ensure_initialized();
        Ok(NoneType)
    }

    /// Set the log format.
    ///
    /// ```python
    /// log.set_format("text")
    /// log.set_format("json")
    /// ```
    fn set_format(format: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        match format.to_lowercase().as_str() {
            "text" | "json" => {
                let mut config = get_config().lock().unwrap();
                config.format = format.to_lowercase();
                Ok(NoneType)
            }
            _ => Err(anyhow::anyhow!("Invalid log format: {}", format)),
        }
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
            return Err(anyhow::anyhow!("Fatal: {}", message));
        }
        ensure_initialized();
        log::error!("{}", message);
        Err(anyhow::anyhow!("Fatal: {}", message))
    }

    /// Convenience no-op to ensure module is loaded.
    fn _log_module_loaded() -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        ensure_initialized();
        Ok(NoneType)
    }
}
