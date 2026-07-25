"""
Spaces Signal Module

This module provides ergonomic wrappers for exec-mode signal trapping.
Use these helpers to register cleanup handlers for signals like INT/TERM
(on Unix) or INT (on Windows) and dispatch trapped signals at explicit
safe points.

Use `signal_supported()` and `signal_supported_names()` to branch behavior
for platform-specific signal capabilities.

Example:
    load("//@star/prelude/exec/signal.star", "signal_trap", "signal_wait")

    def on_interrupt(sig):
        print("Caught", sig)

    signal_trap(["INT", "TERM"], on_interrupt)

    while True:
        observed = signal_wait(timeout_ms = 1000)
        if observed != None:
            break
"""

def _signal_list(signal_names: str | list[str]) -> list[str]:
    """Normalize a single signal name or list of names to `list[str]`."""
    signal_type = type(signal_names)
    if signal_type == "string":
        return [signal_names]

    if signal_type == "list":
        for item in signal_names:
            if type(item) != "string":
                fail("signal_names must be a string or list of strings; got list item of type: " + type(item))
        return signal_names

    fail("signal_names must be a string or list of strings; got: " + signal_type)

def signal_supported() -> bool:
    """Return whether signal trapping APIs are supported on this platform."""
    return signal.supported()

def signal_supported_names() -> list[str]:
    """Return canonical signal names supported on this platform."""
    return signal.supported_names()

def signal_trap(signal_names: str | list[str], handler):
    """
    Register/replace a trap handler for one or more signals.

    Args:
        signal_names: Signal name (`"INT"`, `"SIGINT"`, etc.) or list of names.
            On Windows, only `INT`/`SIGINT` is supported.
        handler: Callable invoked as `handler(signal_name)`.
    """
    for name in _signal_list(signal_names):
        signal.trap(name, handler)

def signal_untrap(signal_names: str | list[str]):
    """Remove trap handlers for one or more signals."""
    for name in _signal_list(signal_names):
        signal.untrap(name)

def signal_clear():
    """Remove all registered trap handlers."""
    signal.clear()

def signal_pending() -> list[str]:
    """Return queued signal names without invoking handlers."""
    return signal.pending()

def signal_dispatch() -> int:
    """Dispatch queued signals and return the number of handlers invoked."""
    return signal.dispatch()

def signal_wait(timeout_ms: int | None = None) -> str | None:
    """
    Wait for a signal, dispatch handlers, and return the first queued signal name.

    Args:
        timeout_ms: Optional timeout in milliseconds. Returns `None` on timeout.
    """
    return signal.wait(timeout_ms = timeout_ms)
