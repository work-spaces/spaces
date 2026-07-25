use crate::is_lsp_mode;
use anyhow::{Context, bail};
use anyhow_source_location::format_context;
#[cfg(unix)]
use signal_hook::SigId;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::none::{NoneOr, NoneType};
use starlark::values::typing::{FrozenStarlarkCallable, StarlarkCallable};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

#[cfg(unix)]
use std::os::fd::RawFd;

const SIGNAL_RECORD_SIZE: usize = std::mem::size_of::<i32>();

#[cfg(unix)]
struct TrapEntry {
    registration_id: SigId,
    handler: FrozenStarlarkCallable,
}

#[cfg(unix)]
struct SignalRuntime {
    read_fd: RawFd,
    write_fd: RawFd,
    partial_record: Vec<u8>,
    pending: VecDeque<i32>,
    traps: HashMap<i32, TrapEntry>,
}

#[cfg(unix)]
impl SignalRuntime {
    fn new() -> anyhow::Result<Self> {
        let (read_fd, write_fd) = create_pipe()?;
        Ok(Self {
            read_fd,
            write_fd,
            partial_record: Vec::new(),
            pending: VecDeque::new(),
            traps: HashMap::new(),
        })
    }
}

#[cfg(unix)]
static SIGNAL_RUNTIME: OnceLock<Mutex<SignalRuntime>> = OnceLock::new();

#[cfg(unix)]
fn runtime() -> anyhow::Result<&'static Mutex<SignalRuntime>> {
    if let Some(runtime) = SIGNAL_RUNTIME.get() {
        return Ok(runtime);
    }

    let runtime = SignalRuntime::new()?;
    let _ = SIGNAL_RUNTIME.set(Mutex::new(runtime));

    SIGNAL_RUNTIME
        .get()
        .context(format_context!("Failed to initialize signal runtime"))
}

#[cfg(unix)]
fn create_pipe() -> anyhow::Result<(RawFd, RawFd)> {
    let mut fds = [0i32; 2];
    // SAFETY: `fds` points to valid writable memory for two file descriptors.
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .context(format_context!("Failed to create signal pipe"));
    }

    let read_fd = fds[0];
    let write_fd = fds[1];

    set_nonblocking(read_fd).context(format_context!(
        "Failed to set signal read pipe as non-blocking"
    ))?;
    set_nonblocking(write_fd).context(format_context!(
        "Failed to set signal write pipe as non-blocking"
    ))?;
    set_cloexec(read_fd).context(format_context!(
        "Failed to set close-on-exec for signal read pipe"
    ))?;
    set_cloexec(write_fd).context(format_context!(
        "Failed to set close-on-exec for signal write pipe"
    ))?;

    Ok((read_fd, write_fd))
}

#[cfg(unix)]
fn set_nonblocking(fd: RawFd) -> anyhow::Result<()> {
    // SAFETY: `fcntl` is called with a valid file descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(std::io::Error::last_os_error())
            .context(format_context!("fcntl(F_GETFL) failed for fd {}", fd));
    }

    // SAFETY: `fcntl` is called with a valid file descriptor and flag bitset.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if rc == -1 {
        return Err(std::io::Error::last_os_error())
            .context(format_context!("fcntl(F_SETFL) failed for fd {}", fd));
    }

    Ok(())
}

#[cfg(unix)]
fn set_cloexec(fd: RawFd) -> anyhow::Result<()> {
    // SAFETY: `fcntl` is called with a valid file descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 {
        return Err(std::io::Error::last_os_error())
            .context(format_context!("fcntl(F_GETFD) failed for fd {}", fd));
    }

    // SAFETY: `fcntl` is called with a valid file descriptor and flag bitset.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
    if rc == -1 {
        return Err(std::io::Error::last_os_error())
            .context(format_context!("fcntl(F_SETFD) failed for fd {}", fd));
    }

    Ok(())
}

#[cfg(unix)]
fn read_signals_into_queue(runtime: &mut SignalRuntime) -> anyhow::Result<()> {
    let mut buf = [0u8; 256];

    loop {
        // SAFETY: `buf` is valid writable memory and `runtime.read_fd` is a live file descriptor.
        let n = unsafe { libc::read(runtime.read_fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 {
            let n = usize::try_from(n).context(format_context!("Failed to convert read size"))?;
            runtime.partial_record.extend_from_slice(&buf[..n]);

            let mut consumed = 0usize;
            while runtime.partial_record.len().saturating_sub(consumed) >= SIGNAL_RECORD_SIZE {
                let chunk = &runtime.partial_record[consumed..consumed + SIGNAL_RECORD_SIZE];
                let signal = i32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                runtime.pending.push_back(signal);
                consumed += SIGNAL_RECORD_SIZE;
            }

            if consumed > 0 {
                runtime.partial_record.drain(..consumed);
            }
            continue;
        }

        if n == 0 {
            // No writers available right now; nothing else to drain.
            break;
        }

        let io_error = std::io::Error::last_os_error();
        match io_error.kind() {
            std::io::ErrorKind::WouldBlock => break,
            std::io::ErrorKind::Interrupted => continue,
            _ => {
                return Err(io_error).context(format_context!("Failed reading from signal pipe"));
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
fn wait_for_signal_event(timeout_ms: Option<i64>) -> anyhow::Result<bool> {
    let read_fd = {
        let runtime = runtime()?;
        let guard = runtime
            .lock()
            .map_err(|_| anyhow::anyhow!("signal runtime lock poisoned"))?;
        guard.read_fd
    };

    let started = std::time::Instant::now();

    loop {
        let poll_timeout = match timeout_ms {
            Some(limit) => {
                if limit < 0 {
                    bail!("timeout_ms must be non-negative, got {}", limit);
                }

                let elapsed_ms_u128 = started.elapsed().as_millis();
                let elapsed_ms = i64::try_from(elapsed_ms_u128).unwrap_or(i64::MAX);
                let remaining = limit.saturating_sub(elapsed_ms);

                if remaining == 0 {
                    return Ok(false);
                }

                i32::try_from(remaining).unwrap_or(i32::MAX)
            }
            None => -1,
        };

        let mut pfd = libc::pollfd {
            fd: read_fd,
            events: libc::POLLIN,
            revents: 0,
        };

        // SAFETY: `pfd` points to valid pollfd memory for one entry.
        let rc = unsafe { libc::poll(&mut pfd as *mut libc::pollfd, 1, poll_timeout) };
        if rc > 0 {
            return Ok(true);
        }
        if rc == 0 {
            return Ok(false);
        }

        let io_error = std::io::Error::last_os_error();
        if io_error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }

        return Err(io_error).context(format_context!("poll() failed while waiting for signal"));
    }
}

fn normalized_name(name: &str) -> anyhow::Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!(
            "Signal name cannot be empty. Supported signals: {}",
            supported_signal_names().join(", ")
        );
    }

    let upper = trimmed.to_ascii_uppercase();
    let normalized = upper.strip_prefix("SIG").unwrap_or(upper.as_str());
    if normalized.is_empty() {
        bail!(
            "Signal name cannot be empty. Supported signals: {}",
            supported_signal_names().join(", ")
        );
    }

    Ok(normalized.to_string())
}

#[cfg(unix)]
fn signal_from_name(name: &str) -> anyhow::Result<i32> {
    let normalized = normalized_name(name)?;

    if normalized == "KILL" || normalized == "STOP" {
        bail!(
            "Signal `{}` cannot be trapped (KILL and STOP are not trappable)",
            name
        );
    }

    match normalized.as_str() {
        "INT" => Ok(libc::SIGINT),
        "TERM" => Ok(libc::SIGTERM),
        "HUP" => Ok(libc::SIGHUP),
        "QUIT" => Ok(libc::SIGQUIT),
        "USR1" => Ok(libc::SIGUSR1),
        "USR2" => Ok(libc::SIGUSR2),
        "ALRM" => Ok(libc::SIGALRM),
        _ => bail!(
            "Unsupported signal `{}`. Supported signals: {}",
            name,
            supported_signal_names().join(", ")
        ),
    }
}

#[cfg(not(unix))]
fn signal_from_name(name: &str) -> anyhow::Result<i32> {
    let normalized = normalized_name(name)?;

    if normalized == "KILL" || normalized == "STOP" {
        bail!(
            "Signal `{}` cannot be trapped (KILL and STOP are not trappable)",
            name
        );
    }

    match normalized.as_str() {
        "INT" => Ok(libc::SIGINT),
        "TERM" => Ok(libc::SIGTERM),
        _ => bail!(
            "Unsupported signal `{}` on this platform. Supported signals: {}",
            name,
            supported_signal_names().join(", ")
        ),
    }
}

#[cfg(unix)]
fn signal_name(signal: i32) -> &'static str {
    match signal {
        x if x == libc::SIGINT => "INT",
        x if x == libc::SIGTERM => "TERM",
        x if x == libc::SIGHUP => "HUP",
        x if x == libc::SIGQUIT => "QUIT",
        x if x == libc::SIGUSR1 => "USR1",
        x if x == libc::SIGUSR2 => "USR2",
        x if x == libc::SIGALRM => "ALRM",
        _ => "UNKNOWN",
    }
}

#[cfg(not(unix))]
fn signal_name(signal: i32) -> &'static str {
    match signal {
        x if x == libc::SIGINT => "INT",
        x if x == libc::SIGTERM => "TERM",
        _ => "UNKNOWN",
    }
}

#[cfg(unix)]
fn register_signal_handler(signal: i32, write_fd: RawFd) -> anyhow::Result<SigId> {
    // SAFETY: The callback only performs async-signal-safe operations: converting
    // a fixed-size integer to bytes and writing to a non-blocking file descriptor.
    let registration_id = unsafe {
        signal_hook::low_level::register(signal, move || {
            let payload = signal.to_ne_bytes();
            // SAFETY: `write_fd` is initialized once and remains open for process lifetime.
            // The pointer points to a stack byte array valid for this call.
            let _ = libc::write(write_fd, payload.as_ptr().cast(), payload.len());
        })
    }
    .context(format_context!(
        "Failed to register signal handler for {}",
        signal_name(signal)
    ))?;

    Ok(registration_id)
}

fn supported_signal_names() -> &'static [&'static str] {
    #[cfg(unix)]
    {
        &["INT", "TERM", "HUP", "QUIT", "USR1", "USR2", "ALRM"]
    }

    #[cfg(not(unix))]
    {
        &["INT", "TERM"]
    }
}

fn dispatch_pending<'v>(eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<i32> {
    let mut dispatched = 0i32;

    #[cfg(unix)]
    {
        loop {
            let next = {
                let runtime = runtime()?;
                let mut guard = runtime
                    .lock()
                    .map_err(|_| anyhow::anyhow!("signal runtime lock poisoned"))?;

                read_signals_into_queue(&mut guard)?;

                let signal = match guard.pending.pop_front() {
                    Some(signal) => signal,
                    None => break,
                };

                let handler = guard.traps.get(&signal).map(|entry| entry.handler);
                Some((signal, handler))
            };

            let Some((signal, maybe_handler)) = next else {
                break;
            };

            let Some(handler) = maybe_handler else {
                continue;
            };

            let signal_name = signal_name(signal).to_string();
            let heap = eval.heap();

            eval.eval_function(
                handler.0.to_value(),
                &[heap.alloc(signal_name.clone())],
                &[],
            )
            .map_err(|error| {
                anyhow::anyhow!(format_context!(
                    "signal handler failed for {}: {}",
                    signal_name,
                    error
                ))
            })?;

            dispatched += 1;
        }
    }

    #[cfg(not(unix))]
    {
        let _ = eval;
    }

    Ok(dispatched)
}

// This defines the functions that are visible to Starlark.
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Register or replace a trap handler for a signal.
    ///
    /// `name` accepts canonical names (for example `"INT"`) and `SIG*`
    /// aliases (for example `"SIGINT"`).
    fn trap<'v>(name: &str, handler: StarlarkCallable<'v>) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        let signal = signal_from_name(name)?;
        let frozen_handler = handler.unpack_frozen().context(format_context!(
            "signal.trap requires a frozen callable handler (for example a top-level function)"
        ))?;

        #[cfg(unix)]
        {
            let runtime = runtime()?;
            let mut guard = runtime
                .lock()
                .map_err(|_| anyhow::anyhow!("signal runtime lock poisoned"))?;

            read_signals_into_queue(&mut guard)?;

            if let Some(existing) = guard.traps.get_mut(&signal) {
                existing.handler = frozen_handler;
                return Ok(NoneType);
            }

            let registration_id = register_signal_handler(signal, guard.write_fd)?;
            guard.traps.insert(
                signal,
                TrapEntry {
                    registration_id,
                    handler: frozen_handler,
                },
            );
        }

        #[cfg(not(unix))]
        {
            let _ = signal;
            let _ = frozen_handler;
            bail!("signal.trap is not supported on this platform");
        }

        Ok(NoneType)
    }

    /// Remove a trap handler for a signal.
    fn untrap(name: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        let signal = signal_from_name(name)?;

        #[cfg(unix)]
        {
            let runtime = runtime()?;
            let mut guard = runtime
                .lock()
                .map_err(|_| anyhow::anyhow!("signal runtime lock poisoned"))?;

            if let Some(entry) = guard.traps.remove(&signal) {
                signal_hook::low_level::unregister(entry.registration_id);
            }
        }

        #[cfg(not(unix))]
        {
            let _ = signal;
            bail!("signal.untrap is not supported on this platform");
        }

        Ok(NoneType)
    }

    /// Remove all registered trap handlers.
    fn clear() -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        #[cfg(unix)]
        {
            let runtime = runtime()?;
            let mut guard = runtime
                .lock()
                .map_err(|_| anyhow::anyhow!("signal runtime lock poisoned"))?;

            let registration_ids: Vec<SigId> = guard
                .traps
                .drain()
                .map(|(_, entry)| entry.registration_id)
                .collect();

            for registration_id in registration_ids {
                signal_hook::low_level::unregister(registration_id);
            }
        }

        #[cfg(not(unix))]
        {
            bail!("signal.clear is not supported on this platform");
        }

        Ok(NoneType)
    }

    /// Return queued signals as a list without invoking handlers.
    fn pending() -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        #[cfg(unix)]
        {
            let runtime = runtime()?;
            let mut guard = runtime
                .lock()
                .map_err(|_| anyhow::anyhow!("signal runtime lock poisoned"))?;

            read_signals_into_queue(&mut guard)?;
            return Ok(guard
                .pending
                .iter()
                .map(|signal| signal_name(*signal).to_string())
                .collect());
        }

        #[cfg(not(unix))]
        {
            Ok(Vec::new())
        }
    }

    /// Dispatch queued signals to registered handlers.
    ///
    /// Returns the number of handlers invoked.
    fn dispatch<'v>(eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<i32> {
        if is_lsp_mode() {
            return Ok(0);
        }

        dispatch_pending(eval)
    }

    /// Wait until a signal arrives (or timeout), then dispatch queued handlers.
    ///
    /// Returns the first queued signal name, or `None` on timeout.
    fn wait<'v>(
        #[starlark(require = named)] timeout_ms: Option<i64>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneOr<String>> {
        if is_lsp_mode() {
            return Ok(NoneOr::None);
        }

        #[cfg(unix)]
        {
            {
                let runtime = runtime()?;
                let mut guard = runtime
                    .lock()
                    .map_err(|_| anyhow::anyhow!("signal runtime lock poisoned"))?;
                read_signals_into_queue(&mut guard)?;
            }

            let has_pending = {
                let runtime = runtime()?;
                let guard = runtime
                    .lock()
                    .map_err(|_| anyhow::anyhow!("signal runtime lock poisoned"))?;
                !guard.pending.is_empty()
            };

            if !has_pending {
                let arrived = wait_for_signal_event(timeout_ms)?;
                if !arrived {
                    return Ok(NoneOr::None);
                }
            }

            let first_signal = {
                let runtime = runtime()?;
                let mut guard = runtime
                    .lock()
                    .map_err(|_| anyhow::anyhow!("signal runtime lock poisoned"))?;
                read_signals_into_queue(&mut guard)?;
                guard.pending.front().copied()
            };

            let Some(first_signal) = first_signal else {
                return Ok(NoneOr::None);
            };

            let first_signal_name = signal_name(first_signal).to_string();
            let _ = dispatch_pending(eval)?;
            return Ok(NoneOr::Other(first_signal_name));
        }

        #[cfg(not(unix))]
        {
            let _ = timeout_ms;
            let _ = eval;
            Ok(NoneOr::None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_sig_prefix() {
        assert_eq!(normalized_name("SIGINT").unwrap(), "INT");
        assert_eq!(normalized_name("term").unwrap(), "TERM");
    }

    #[test]
    fn rejects_empty_signal_name() {
        let error = normalized_name("  ").unwrap_err().to_string();
        assert!(error.contains("Signal name cannot be empty"));

        let error = normalized_name("SIG").unwrap_err().to_string();
        assert!(error.contains("Signal name cannot be empty"));

        let error = normalized_name("  sig  ").unwrap_err().to_string();
        assert!(error.contains("Signal name cannot be empty"));
    }

    #[test]
    fn rejects_untrappable_signals() {
        let error = signal_from_name("SIGKILL").unwrap_err().to_string();
        assert!(error.contains("cannot be trapped"));

        let error = signal_from_name("STOP").unwrap_err().to_string();
        assert!(error.contains("cannot be trapped"));
    }
}
