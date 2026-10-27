use crate::lock;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry, fmt, reload};

pub use console::{FinalType, make_finalize_line};

/// File name of the trace written to the run log folder.
pub const TRACE_FILE_NAME: &str = "spaces.trace.log";

/// File name of the trace written directly to `.spaces/logs` by commands that
/// don't create a run log folder.
pub fn get_standalone_trace_file_name(timestamp: &str) -> String {
    format!("spaces.trace.{timestamp}.log")
}

/// Maximum number of bytes buffered before a trace file is attached.
const TRACE_BUFFER_LIMIT: usize = 8 * 1024 * 1024;

/// Target used for events bridged from `Logger`.
const LOGGER_TRACE_TARGET: &str = "spaces";

/// Crates that receive the requested trace level. Everything else stays at `warn`.
const SPACES_TRACE_TARGETS: &[&str] = &[
    "spaces",
    "spaces_utils",
    "spaces_console",
    "spaces_archiver",
    "starstd",
];

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, clap::ValueEnum, strum::Display,
)]
#[strum(serialize_all = "snake_case")]
pub enum TraceLevel {
    #[default]
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl TraceLevel {
    fn as_directive(self) -> Option<&'static str> {
        match self {
            TraceLevel::Off => None,
            TraceLevel::Error => Some("error"),
            TraceLevel::Warn => Some("warn"),
            TraceLevel::Info => Some("info"),
            TraceLevel::Debug => Some("debug"),
            TraceLevel::Trace => Some("trace"),
        }
    }
}

/// Builds the `EnvFilter` directives for a trace level: spaces crates get
/// `level`, dependencies are capped at `warn`.
fn filter_directives(level: TraceLevel) -> String {
    let Some(spaces_level) = level.as_directive() else {
        return "off".to_string();
    };
    let dependency_level = level.min(TraceLevel::Warn).as_directive().unwrap_or("off");

    let mut directives = dependency_level.to_string();
    for target in SPACES_TRACE_TARGETS {
        directives.push_str(&format!(",{target}={spaces_level}"));
    }
    directives
}

#[derive(Debug, Clone, Default)]
pub struct TraceOptions {
    /// Explicit level (`--trace-level`). Takes precedence over the `tracing-*` features.
    pub level: Option<TraceLevel>,
    /// Level from `SPACES_ENV_TRACING_*`. Used from startup until the workspace
    /// features are applied via `apply_trace_level`.
    pub env_level: Option<TraceLevel>,
    /// Full `EnvFilter` override (`--trace-filter` / `SPACES_ENV_TRACE_FILTER`).
    pub filter: Option<Arc<str>>,
    /// Explicit trace file (`--trace-file`). Attached immediately, bypassing buffering.
    pub file: Option<Arc<str>>,
}

enum SinkState {
    Off,
    Buffering {
        buffer: Vec<u8>,
        dropped_bytes: usize,
    },
    File {
        path: Arc<str>,
        writer: std::io::BufWriter<std::fs::File>,
    },
}

/// Destination for formatted trace output. Buffers in memory until a trace
/// file is attached (e.g. once the run log folder is known).
#[derive(Clone)]
struct TraceSink {
    state: Arc<Mutex<SinkState>>,
    buffer_limit: usize,
}

fn create_trace_file(path: &str) -> anyhow::Result<std::io::BufWriter<std::fs::File>> {
    if let Some(parent) = std::path::Path::new(path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).context(format_context!(
            "Failed to create trace file directory {}",
            parent.display()
        ))?;
    }
    let file = std::fs::File::create(path)
        .context(format_context!("Failed to create trace file {path}"))?;
    Ok(std::io::BufWriter::new(file))
}

impl TraceSink {
    fn new(state: SinkState, buffer_limit: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            buffer_limit,
        }
    }

    fn new_off() -> Self {
        Self::new(SinkState::Off, 0)
    }

    fn new_buffering(buffer_limit: usize) -> Self {
        Self::new(
            SinkState::Buffering {
                buffer: Vec::new(),
                dropped_bytes: 0,
            },
            buffer_limit,
        )
    }

    fn new_file(path: &str) -> anyhow::Result<Self> {
        let writer = create_trace_file(path)?;
        Ok(Self::new(
            SinkState::File {
                path: path.into(),
                writer,
            },
            0,
        ))
    }

    fn lock(&self) -> MutexGuard<'_, SinkState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_bytes(&self, bytes: &[u8]) {
        // Phase 1 does not redact secrets (parity with console `Logger` output).
        // Secret redaction (e.g. `console::Secrets::redact`) would be applied here.
        let mut state = self.lock();
        match &mut *state {
            SinkState::Off => {}
            SinkState::Buffering {
                buffer,
                dropped_bytes,
            } => {
                if buffer.len() + bytes.len() <= self.buffer_limit {
                    buffer.extend_from_slice(bytes);
                } else {
                    *dropped_bytes += bytes.len();
                }
            }
            SinkState::File { writer, .. } => {
                let _ = writer.write_all(bytes);
            }
        }
    }

    /// Writes buffered output to `path` and switches to file output.
    /// No-op unless the sink is buffering.
    fn attach(&self, path: &str) -> anyhow::Result<()> {
        let mut state = self.lock();
        let SinkState::Buffering {
            buffer,
            dropped_bytes,
        } = &mut *state
        else {
            return Ok(());
        };

        let mut writer = create_trace_file(path)?;
        writer
            .write_all(buffer)
            .context(format_context!("Failed to write trace file {path}"))?;
        if *dropped_bytes > 0 {
            writeln!(
                writer,
                "[spaces] {dropped_bytes} bytes of trace output were dropped before the trace file was attached (buffer limit {} bytes)",
                self.buffer_limit
            )
            .context(format_context!("Failed to write trace file {path}"))?;
        }

        *state = SinkState::File {
            path: path.into(),
            writer,
        };
        Ok(())
    }

    /// Flushes and closes the sink. Returns the trace file path if one was attached.
    /// Buffered output that was never attached is discarded.
    fn shutdown(&self) -> Option<Arc<str>> {
        let state = std::mem::replace(&mut *self.lock(), SinkState::Off);
        match state {
            SinkState::File { path, mut writer } => {
                let _ = writer.flush();
                Some(path)
            }
            SinkState::Off | SinkState::Buffering { .. } => None,
        }
    }
}

struct TraceSinkWriter<'a> {
    sink: &'a TraceSink,
}

impl std::io::Write for TraceSinkWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.sink.write_bytes(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> fmt::MakeWriter<'a> for TraceSink {
    type Writer = TraceSinkWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        TraceSinkWriter { sink: self }
    }
}

fn new_fmt_layer<S>(sink: TraceSink) -> impl tracing_subscriber::Layer<S>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    fmt::layer()
        .with_writer(sink)
        .with_ansi(false)
        .with_thread_ids(true)
        .with_target(true)
        .with_timer(fmt::time::uptime())
        .with_span_events(FmtSpan::CLOSE)
}

struct TracingState {
    sink: TraceSink,
    filter_handle: reload::Handle<EnvFilter, Registry>,
    is_level_overridden: bool,
    is_enabled: AtomicBool,
}

static TRACING_STATE: OnceLock<TracingState> = OnceLock::new();

/// Installs the global tracing subscriber. Trace output is independent of the
/// console verbosity and is only ever written to a trace file.
pub fn init_tracing(options: TraceOptions) -> anyhow::Result<()> {
    if TRACING_STATE.get().is_some() {
        return Err(format_error!("Tracing is already initialized"));
    }

    let is_level_overridden = options.level.is_some() || options.filter.is_some();
    let level = options.level.or(options.env_level).unwrap_or_default();

    let filter = if let Some(filter) = options.filter.as_ref() {
        EnvFilter::try_new(filter.as_ref())
            .context(format_context!("Invalid trace filter `{filter}`"))?
    } else {
        EnvFilter::try_new(filter_directives(level))
            .context(format_context!("Invalid trace level `{level}`"))?
    };
    let is_enabled = options.filter.is_some() || level != TraceLevel::Off;

    let sink = if let Some(file) = options.file.as_ref() {
        TraceSink::new_file(file.as_ref())?
    } else if is_level_overridden && !is_enabled {
        TraceSink::new_off()
    } else {
        // The workspace features may still enable tracing later
        TraceSink::new_buffering(TRACE_BUFFER_LIMIT)
    };

    let (filter_layer, filter_handle) = reload::Layer::new(filter);
    let subscriber = tracing_subscriber::registry()
        .with(filter_layer)
        .with(new_fmt_layer(sink.clone()));

    // `set_global_default` (rather than `try_init`) avoids installing a `log`
    // bridge, which would conflict with the `env_logger` used by starstd.
    // Global (not thread-local) so rayon worker threads are captured.
    tracing::subscriber::set_global_default(subscriber)
        .context(format_context!("Failed to install tracing subscriber"))?;

    TRACING_STATE
        .set(TracingState {
            sink,
            filter_handle,
            is_level_overridden,
            is_enabled: AtomicBool::new(is_enabled),
        })
        .map_err(|_| format_error!("Tracing is already initialized"))?;

    Ok(())
}

/// True if the trace level was set explicitly (`--trace-level` / `--trace-filter`)
/// and must not be changed by the workspace features.
pub fn is_trace_level_overridden() -> bool {
    TRACING_STATE
        .get()
        .is_some_and(|state| state.is_level_overridden)
}

/// True if trace output is currently being captured.
pub fn is_tracing_enabled() -> bool {
    TRACING_STATE
        .get()
        .is_some_and(|state| state.is_enabled.load(Ordering::Relaxed))
}

/// Replaces the active trace filter with the default filter for `level`.
pub fn apply_trace_level(level: TraceLevel) {
    let Some(state) = TRACING_STATE.get() else {
        return;
    };
    if state.is_level_overridden {
        return;
    }
    let Ok(filter) = EnvFilter::try_new(filter_directives(level)) else {
        return;
    };
    if state.filter_handle.reload(filter).is_ok() {
        state
            .is_enabled
            .store(level != TraceLevel::Off, Ordering::Relaxed);
    }
}

/// Writes any buffered trace output to `path` and continues writing there.
/// No-op if tracing is disabled or a trace file is already attached.
pub fn attach_trace_file(path: &str) -> anyhow::Result<()> {
    let Some(state) = TRACING_STATE.get() else {
        return Ok(());
    };
    if !state.is_enabled.load(Ordering::Relaxed) {
        return Ok(());
    }
    state.sink.attach(path)
}

/// Flushes trace output. Returns the trace file path if a trace file was attached.
pub fn shutdown_tracing() -> Option<Arc<str>> {
    TRACING_STATE.get().and_then(|state| state.sink.shutdown())
}

fn to_tracing_level(level: console::Level) -> Option<tracing::Level> {
    match level {
        console::Level::Trace => Some(tracing::Level::TRACE),
        console::Level::Debug => Some(tracing::Level::DEBUG),
        console::Level::Message
        | console::Level::Info
        | console::Level::App
        | console::Level::Passthrough => Some(tracing::Level::INFO),
        console::Level::Warning => Some(tracing::Level::WARN),
        console::Level::Error => Some(tracing::Level::ERROR),
        console::Level::Silent => None,
    }
}

fn emit_trace_event(level: console::Level, label: &str, message: &str) {
    // The tracing macros require a constant level
    match to_tracing_level(level) {
        Some(tracing::Level::TRACE) => {
            tracing::trace!(target: LOGGER_TRACE_TARGET, label = %label, "{message}")
        }
        Some(tracing::Level::DEBUG) => {
            tracing::debug!(target: LOGGER_TRACE_TARGET, label = %label, "{message}")
        }
        Some(tracing::Level::INFO) => {
            tracing::info!(target: LOGGER_TRACE_TARGET, label = %label, "{message}")
        }
        Some(tracing::Level::WARN) => {
            tracing::warn!(target: LOGGER_TRACE_TARGET, label = %label, "{message}")
        }
        Some(tracing::Level::ERROR) => {
            tracing::error!(target: LOGGER_TRACE_TARGET, label = %label, "{message}")
        }
        None => {}
    }
}

pub struct Logger {
    console: console::Console,
    label: Arc<str>,
}

enum DeferredError {
    Message(Arc<str>),
    Lines(Vec<console::Line>),
}

impl std::fmt::Debug for DeferredError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeferredError::Message(message) => f
                .debug_struct("Message")
                .field("len", &message.len())
                .finish(),
            DeferredError::Lines(lines) => f
                .debug_struct("Lines")
                .field("count", &lines.len())
                .finish(),
        }
    }
}

#[derive(Default, Debug)]
struct DeferredMessages {
    deprecation_warnings_enabled: bool,
    warnings: Vec<Arc<str>>,
    errors: Vec<DeferredError>,
}

static DEFERRED_MESSAGES: state::InitCell<lock::StateLock<DeferredMessages>> =
    state::InitCell::new();

fn get_deferred_messages_state() -> &'static lock::StateLock<DeferredMessages> {
    if let Some(state) = DEFERRED_MESSAGES.try_get() {
        return state;
    }

    DEFERRED_MESSAGES.set(lock::StateLock::new(DeferredMessages::default()));
    DEFERRED_MESSAGES.get()
}

fn push_deferred_warning(warning: Arc<str>) {
    let mut state = get_deferred_messages_state().write();
    state.warnings.push(warning);
}

pub fn push_deferred_error<Message: std::fmt::Display>(error: Message) {
    let mut state = get_deferred_messages_state().write();
    state
        .errors
        .push(DeferredError::Message(error.to_string().into()));
}

pub fn push_deferred_error_lines(lines: Vec<console::Line>) {
    let mut state = get_deferred_messages_state().write();
    state.errors.push(DeferredError::Lines(lines));
}

fn take_deferred_errors() -> Vec<DeferredError> {
    let mut state = get_deferred_messages_state().write();
    std::mem::take(&mut state.errors)
}

pub fn show_deferred_errors(console: console::Console) {
    let deferred_errors = take_deferred_errors();
    if deferred_errors.is_empty() {
        return;
    }

    for deferred_error in deferred_errors {
        match deferred_error {
            DeferredError::Lines(lines) => {
                console.emit_lines(lines);
            }
            DeferredError::Message(message) => {
                let mut container = console::bootstrap::Container::new();
                container.add(console::bootstrap::VerticalSpacer::new(1));
                let mut quote = console::bootstrap::Blockquote::new()
                    .variant(console::bootstrap::Variant::Danger);
                for line in message.lines().filter(|line| !line.trim().is_empty()) {
                    quote.push_line(line.to_string());
                }
                container.add(quote);
                console.emit_container(&container);
            }
        }
    }
}

pub fn enable_deprecation_warnings() {
    let mut state = get_deferred_messages_state().write();
    state.deprecation_warnings_enabled = true;
}

pub fn push_deprecation_warning<Message: std::fmt::Display>(
    module: Option<Arc<str>>,
    warning: Message,
) {
    let is_deprecation_warning_enabled = {
        let state = get_deferred_messages_state().read();
        state.deprecation_warnings_enabled
    };
    if is_deprecation_warning_enabled {
        let module = module.unwrap_or("unknown".into());
        push_deferred_warning(format!("{module} => {warning}").into());
    }
}

pub fn get_deferred_warnings() -> Vec<Arc<str>> {
    let state = get_deferred_messages_state().read();
    state.warnings.clone()
}

impl Logger {
    pub fn new(console: console::Console, label: Arc<str>) -> Logger {
        Logger { console, label }
    }

    pub fn trace(&self, message: &str) {
        self.log(console::Level::Trace, message);
    }

    pub fn debug(&self, message: &str) {
        self.log(console::Level::Debug, message);
    }

    pub fn message(&self, message: &str) {
        self.log(console::Level::Message, message);
    }

    pub fn info(&self, message: &str) {
        self.log(console::Level::Info, message);
    }

    pub fn app(&self, message: &str) {
        self.log(console::Level::App, message);
    }

    pub fn raw(&self, message: &str) {
        let _ = self.console.raw(message);
    }

    pub fn warning(&self, message: &str) {
        emit_trace_event(console::Level::Warning, &self.label, message);
        let deferred = format!("[{}] {message}", self.label);
        push_deferred_warning(deferred.into());
    }

    pub fn error(&self, message: &str) {
        self.log(console::Level::Error, message);
    }

    fn log(&self, level: console::Level, message: &str) {
        emit_trace_event(level, &self.label, message);
        let output = format!("[{}] {message}", self.label);
        let _ = self.console.log(level, &output);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShowBacktrace {
    No,
    Yes,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StripSourceLocation {
    No,
    Yes,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShowFailedBanner {
    No,
    Yes,
}

fn is_source_location_fragment(fragment: &str) -> bool {
    let Some((path, line)) = fragment.rsplit_once(':') else {
        return false;
    };

    !path.is_empty()
        && path.contains('/')
        && line.chars().all(|ch| ch.is_ascii_digit())
        && !line.is_empty()
}

fn strip_source_locations(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut remaining = line;

    while let Some(open_idx) = remaining.find('[') {
        output.push_str(&remaining[..open_idx]);

        let bracket_slice = &remaining[open_idx + 1..];
        let Some(close_rel_idx) = bracket_slice.find(']') else {
            output.push_str(&remaining[open_idx..]);
            remaining = "";
            break;
        };

        let fragment = &bracket_slice[..close_rel_idx];

        if is_source_location_fragment(fragment) {
            remaining = &bracket_slice[close_rel_idx + 1..];
            continue;
        }

        output.push_str(&remaining[open_idx..open_idx + close_rel_idx + 2]);
        remaining = &bracket_slice[close_rel_idx + 1..];
    }

    output.push_str(remaining);
    output.to_string()
}

pub fn show_error(
    console: console::Console,
    input_error_chain: Vec<String>,
    show_backtrace: ShowBacktrace,
    strip_source_location: StripSourceLocation,
    show_failed_banner: ShowFailedBanner,
) {
    let mut container = console::bootstrap::Container::new();
    container.add(console::bootstrap::VerticalSpacer::new(1));

    let mut error_lines = Vec::new();
    let mut error_chain = Vec::new();
    if show_backtrace == ShowBacktrace::Yes {
        for cause in input_error_chain.into_iter().rev() {
            error_chain.push(cause.to_string());
        }
    } else if let Some(last) = input_error_chain.first() {
        error_chain.push(last.to_string());
    }

    for cause in error_chain {
        for line in cause.lines() {
            let line = if strip_source_location == StripSourceLocation::Yes {
                strip_source_locations(line)
            } else {
                line.to_string()
            };

            if line.is_empty() {
                continue;
            }
            error_lines.push(line);
        }
    }

    if show_failed_banner == ShowFailedBanner::Yes {
        container.add(
            console::bootstrap::Banner::new(format!(
                "{} Failed",
                console::bootstrap::icon_danger()
            ))
            .width(console::bootstrap::Width::default())
            .variant(console::bootstrap::Variant::Danger),
        );

        let mut error_quote =
            console::bootstrap::Blockquote::new().variant(console::bootstrap::Variant::Danger);
        for error_line in error_lines {
            error_quote.push_line(error_line);
        }
        container.add(error_quote);
        container.add(
            console::bootstrap::Divider::new()
                .style(console::bootstrap::DividerStyle::Double)
                .width(console::bootstrap::Width::default()),
        );
    } else {
        for error_line in error_lines {
            container.add(console::bootstrap::Paragraph::new(error_line));
        }
    }

    console.emit_container(&container);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffered_text(sink: &TraceSink) -> String {
        match &*sink.lock() {
            SinkState::Buffering { buffer, .. } => String::from_utf8_lossy(buffer).to_string(),
            _ => panic!("sink is not buffering"),
        }
    }

    fn new_test_subscriber(sink: &TraceSink) -> impl tracing::Subscriber + Send + Sync {
        tracing_subscriber::registry()
            .with(EnvFilter::new(filter_directives(TraceLevel::Trace)))
            .with(new_fmt_layer(sink.clone()))
    }

    #[test]
    fn test_console_level_to_tracing_level() {
        let expected = [
            (console::Level::Trace, Some(tracing::Level::TRACE)),
            (console::Level::Debug, Some(tracing::Level::DEBUG)),
            (console::Level::Message, Some(tracing::Level::INFO)),
            (console::Level::Info, Some(tracing::Level::INFO)),
            (console::Level::App, Some(tracing::Level::INFO)),
            (console::Level::Passthrough, Some(tracing::Level::INFO)),
            (console::Level::Warning, Some(tracing::Level::WARN)),
            (console::Level::Error, Some(tracing::Level::ERROR)),
            (console::Level::Silent, None),
        ];
        for (console_level, tracing_level) in expected {
            assert_eq!(to_tracing_level(console_level), tracing_level);
        }
    }

    #[test]
    fn test_filter_directives() {
        assert_eq!(filter_directives(TraceLevel::Off), "off");
        assert_eq!(
            filter_directives(TraceLevel::Debug),
            "warn,spaces=debug,spaces_utils=debug,spaces_console=debug,spaces_archiver=debug,starstd=debug"
        );
        assert_eq!(
            filter_directives(TraceLevel::Error),
            "error,spaces=error,spaces_utils=error,spaces_console=error,spaces_archiver=error,starstd=error"
        );
        for level in [
            TraceLevel::Off,
            TraceLevel::Error,
            TraceLevel::Warn,
            TraceLevel::Info,
            TraceLevel::Debug,
            TraceLevel::Trace,
        ] {
            assert!(EnvFilter::try_new(filter_directives(level)).is_ok());
        }
    }

    #[test]
    fn test_trace_sink_buffer_attach_shutdown() {
        let temp_dir = tempfile::tempdir().unwrap();
        let trace_path = temp_dir.path().join("logs").join(TRACE_FILE_NAME);
        let trace_path = trace_path.to_string_lossy().to_string();

        let sink = TraceSink::new_buffering(TRACE_BUFFER_LIMIT);
        tracing::subscriber::with_default(new_test_subscriber(&sink), || {
            let logger = Logger::new(console::Console::new_null(), "test-label".into());
            logger.debug("before attach");
            tracing::trace!(target: "other_crate", "filtered dependency event");

            let buffered = buffered_text(&sink);
            assert!(buffered.contains("before attach"));
            assert!(buffered.contains("label=test-label"));
            assert!(buffered.contains("DEBUG"));
            assert!(!buffered.contains("filtered dependency event"));

            sink.attach(&trace_path).unwrap();
            logger.warning("after attach");
        });

        // attaching again is a no-op once a file is attached
        sink.attach("/nonexistent/should/not/be/created").unwrap();

        assert_eq!(sink.shutdown().as_deref(), Some(trace_path.as_str()));
        let contents = std::fs::read_to_string(&trace_path).unwrap();
        assert!(contents.contains("before attach"));
        assert!(contents.contains("after attach"));
        assert!(contents.contains("WARN"));
        assert!(contents.find("before attach") < contents.find("after attach"));

        // shutdown leaves the sink off
        assert!(sink.shutdown().is_none());
    }

    #[test]
    fn test_trace_sink_buffer_limit_and_discard() {
        let temp_dir = tempfile::tempdir().unwrap();
        let trace_path = temp_dir.path().join(TRACE_FILE_NAME);
        let trace_path = trace_path.to_string_lossy().to_string();

        let sink = TraceSink::new_buffering(16);
        sink.write_bytes(b"0123456789\n");
        sink.write_bytes(b"this line is dropped\n");
        assert_eq!(buffered_text(&sink), "0123456789\n");

        sink.attach(&trace_path).unwrap();
        assert!(sink.shutdown().is_some());
        let contents = std::fs::read_to_string(&trace_path).unwrap();
        assert!(contents.starts_with("0123456789\n"));
        assert!(contents.contains("21 bytes of trace output were dropped"));

        // never attached: buffer is discarded
        let sink = TraceSink::new_buffering(TRACE_BUFFER_LIMIT);
        sink.write_bytes(b"discarded\n");
        assert!(sink.shutdown().is_none());

        // off: writes are ignored and attach does nothing
        let off_path = temp_dir.path().join("off.log");
        let sink = TraceSink::new_off();
        sink.write_bytes(b"ignored\n");
        sink.attach(off_path.to_string_lossy().as_ref()).unwrap();
        assert!(!off_path.exists());
        assert!(sink.shutdown().is_none());
    }

    #[test]
    fn test_trace_sink_off_filter_emits_nothing() {
        let sink = TraceSink::new_buffering(TRACE_BUFFER_LIMIT);
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new(filter_directives(TraceLevel::Off)))
            .with(new_fmt_layer(sink.clone()));
        tracing::subscriber::with_default(subscriber, || {
            let logger = Logger::new(console::Console::new_null(), "test".into());
            logger.error("should not be captured");
        });
        assert!(buffered_text(&sink).is_empty());
    }
}
