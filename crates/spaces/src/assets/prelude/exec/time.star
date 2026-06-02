"""
Spaces Time Module

This module provides ergonomic wrappers around time and date operations.
It supports time measurements, delays, date/time formatting, and monotonic
timing for performance measurement.

All functions are designed to be intuitive and well-documented with practical examples.

Examples:
    # Measure elapsed time
    start = time_monotonic()
    # ... do work ...
    elapsed = time_monotonic() - start
    print(f"Work took {elapsed} ms")

    # Sleep for a specific duration
    time_sleep(1.5)  # Sleep for 1.5 seconds

    # Get current time
    now = time_now()
    print(f"Current timestamp: {now}")

    # Format a timestamp
    timestamp = 1234567890
    formatted = time_format_datetime(timestamp, "%Y-%m-%d %H:%M:%S")
    print(formatted)

    # Parse a datetime string
    timestamp = time_parse_datetime("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S")
    print(timestamp)

    # Use timers for benchmarking
    timer = time_timer_start()
    # ... do work ...
    elapsed_ms = time_timer_elapsed_ms(timer)
    print(f"Operation took {elapsed_ms} ms")
    time_timer_stop(timer)
"""

# ============================================================================
# Current Time Functions
# ============================================================================

def time_now() -> float:
    """
    Gets the current time in seconds since the Unix epoch (UTC).

    Returns the current moment in time as a floating-point number of seconds
    since January 1, 1970, 00:00:00 UTC. This includes fractional seconds
    (nanosecond precision).

    Returns:
        float: Current Unix timestamp with nanosecond precision

    Examples:
        # Get current timestamp
        now = time_now()
        print(f"Current time: {now}")

        # Measure elapsed time
        start = time_now()
        # ... do some work ...
        elapsed = time_now() - start
        print(f"Elapsed time: {elapsed} seconds")

        # Compare timestamps
        timestamp_a = time_now()
        # ... some delay ...
        timestamp_b = time_now()
        if timestamp_b > timestamp_a:
            print("Time has passed!")
    """
    secs, nsec = time.now()
    return float(float(secs) + float(nsec) / 1e9)

def time_unix_seconds() -> int:
    """
    Gets the current Unix timestamp in whole seconds.

    Returns the number of seconds elapsed since the Unix epoch (January 1, 1970).
    This is useful for simple timestamp operations where sub-second precision
    is not needed.

    Returns:
        int: Current Unix timestamp in seconds

    Examples:
        # Get current timestamp in seconds
        now_seconds = time_unix_seconds()
        print(f"Seconds since epoch: {now_seconds}")

        # Log with timestamp
        current_time = time_unix_seconds()
        print(f"[{current_time}] Event occurred")

        # Check if current time is after a specific timestamp
        cutoff = 1704067200  # January 1, 2024
        if time_unix_seconds() > cutoff:
            print("Year 2024 has started!")
    """
    return time.unix()

def time_unix_milliseconds() -> int:
    """
    Gets the current Unix timestamp in milliseconds.

    Returns the number of milliseconds elapsed since the Unix epoch.
    Useful for timing intervals with millisecond precision without
    dealing with floating-point numbers.

    Returns:
        int: Current Unix timestamp in milliseconds

    Examples:
        # Get current timestamp in milliseconds
        now_ms = time_unix_milliseconds()
        print(f"Milliseconds since epoch: {now_ms}")

        # Measure elapsed time in milliseconds
        start_ms = time_unix_milliseconds()
        time_sleep(0.1)  # Sleep 100ms
        end_ms = time_unix_milliseconds()
        elapsed_ms = end_ms - start_ms
        print(f"Elapsed: {elapsed_ms} ms")
    """
    return time.unix_ms()

def time_monotonic() -> int:
    """
    Gets process-local monotonic time in milliseconds.

    Returns elapsed milliseconds since the first call to `time_monotonic()` (effectively process start for most scripts). This clock is
    monotonic, meaning it always moves forward and is not affected by
    system clock adjustments. Ideal for measuring elapsed time and benchmarking.

    Returns:
        int: Milliseconds elapsed since the first call to `time_monotonic()`

    Examples:
        # Measure operation duration
        start = time_monotonic()
        # ... perform operation ...
        elapsed = time_monotonic() - start
        print(f"Operation took {elapsed} ms")

        # Track multiple operations
        op1_start = time_monotonic()
        # ... operation 1 ...
        op1_duration = time_monotonic() - op1_start

        op2_start = time_monotonic()
        # ... operation 2 ...
        op2_duration = time_monotonic() - op2_start

        print(f"Op1: {op1_duration}ms, Op2: {op2_duration}ms")
    """
    return time.monotonic_ms()

# ============================================================================
# Sleep/Delay Functions
# ============================================================================

def time_sleep(seconds: float):
    """
    Pauses execution for the specified number of seconds.

    Suspends the current thread for the given duration. Supports fractional
    seconds for sub-second delays (e.g., 0.5 for 500 milliseconds).

    Args:
        seconds: Number of seconds to sleep. Can be fractional (e.g., 1.5)

    Examples:
        # Sleep for 1 second
        print("Going to sleep...")
        time_sleep(1)
        print("Woke up!")

        # Sleep for 100 milliseconds
        time_sleep(0.1)

        # Retry loop with exponential backoff
        for attempt in range(5):
            try:
                result = attempt_operation()
                break
            except:
                delay = 0.1 * (2 ** attempt)  # 0.1s, 0.2s, 0.4s, 0.8s, 1.6s
                print(f"Retry in {delay} seconds...")
                time_sleep(delay)
    """
    if seconds < 0:
        fail("time_sleep: seconds must be non-negative, got: " + str(seconds))
    time.sleep(int(seconds * 1e9))

def time_sleep_milliseconds(milliseconds: int):
    """
    Pauses execution for the specified number of milliseconds.

    Suspends the current thread for the given number of milliseconds.
    More precise than time_sleep for short delays.

    Args:
        milliseconds: Number of milliseconds to sleep

    Examples:
        # Sleep for 500 milliseconds (0.5 seconds)
        time_sleep_milliseconds(500)

        # Polling loop with fixed interval
        for i in range(10):
            data = check_status()
            print(f"Status: {data}")
            time_sleep_milliseconds(100)  # Check every 100ms
    """
    time.sleep_ms(milliseconds)

def time_sleep_seconds(seconds: int):
    """
    Pauses execution for the specified number of whole seconds.

    Suspends the current thread for an integer number of seconds.
    Use this for longer delays where sub-second precision is not needed.

    Args:
        seconds: Number of whole seconds to sleep

    Examples:
        # Sleep for 5 seconds
        time_sleep_seconds(5)

        # Wait between retries
        for attempt in range(3):
            try:
                result = risky_operation()
                break
            except:
                if attempt < 2:
                    print("Failed, waiting 2 seconds before retry...")
                    time_sleep_seconds(2)
    """
    time.sleep_seconds(seconds)

# ============================================================================
# Date/Time Formatting Functions
# ============================================================================

def time_format_datetime(unix_seconds: int, format_string: str) -> str:
    """
    Formats a Unix timestamp as a human-readable datetime string.

    Converts a Unix timestamp (seconds since epoch) into a formatted date/time
    string using strftime format codes. The timestamp is interpreted as UTC.

    Args:
        unix_seconds: Unix timestamp in seconds (as an integer)
        format_string: strftime format string (e.g., "%Y-%m-%d %H:%M:%S")

    Returns:
        str: Formatted datetime string

    Raises:
        Error: If the timestamp is invalid or formatting fails

    Examples:
        # Format current time
        now = time_unix_seconds()
        formatted = time_format_datetime(now, "%Y-%m-%d")
        print(f"Today is: {formatted}")

        # Format with time components
        timestamp = 1234567890
        date_str = time_format_datetime(timestamp, "%A, %B %d, %Y")
        print(date_str)  # Output: Friday, February 13, 2009

        # ISO-like format
        iso_format = time_format_datetime(now, "%Y-%m-%dT%H:%M:%SZ")
        print(iso_format)  # Output: 2024-01-15T10:30:45Z

        # Custom format patterns
        patterns = {
            "date": "%Y-%m-%d",
            "time": "%H:%M:%S",
            "weekday": "%A",
            "iso8601": "%Y-%m-%dT%H:%M:%SZ",
            "rfc2822": "%a, %d %b %Y %H:%M:%S +0000",
        }
    """
    return time.format(unix_seconds, format_string)

def time_parse_datetime(datetime_string: str, format_string: str) -> int:
    """
    Parses a datetime string and returns the Unix timestamp.

    Converts a formatted datetime string into a Unix timestamp (seconds since epoch).
    If no timezone is specified in the string, UTC is assumed.

    Args:
        datetime_string: The datetime string to parse
        format_string: strftime format string matching the input (e.g., "%Y-%m-%d %H:%M:%S")

    Returns:
        int: Unix timestamp in seconds

    Raises:
        Error: If the string cannot be parsed with the given format

    Examples:
        # Parse a simple date
        timestamp = time_parse_datetime("2024-01-15", "%Y-%m-%d")
        print(f"Timestamp: {timestamp}")

        # Parse date and time
        timestamp = time_parse_datetime("2024-01-15 10:30:45", "%Y-%m-%d %H:%M:%S")
        print(f"Timestamp: {timestamp}")

        # Parse with timezone info (format should include timezone)
        timestamp = time_parse_datetime("2024-01-15 10:30:45+00:00", "%Y-%m-%d %H:%M:%S%z")

        # Calculate days until a specific date
        target = time_parse_datetime("2024-12-25", "%Y-%m-%d")
        now = time_unix_seconds()
        days_left = (target - now) / 86400
        print(f"Days until Christmas: {int(days_left)}")

        # Parse various formats
        timestamps = [
            time_parse_datetime("01/15/2024", "%m/%d/%Y"),
            time_parse_datetime("Jan 15 2024", "%b %d %Y"),
            time_parse_datetime("15-01-2024", "%d-%m-%Y"),
        ]
    """
    return time.parse(datetime_string, format_string)

def time_now_iso8601() -> str:
    """
    Returns the current UTC time as an ISO8601 / RFC3339 formatted string.

    Returns the current moment formatted as a standard ISO8601 string,
    which is widely used in APIs and logging systems.

    Returns:
        str: ISO8601 formatted timestamp (e.g., "2024-01-15T10:30:45.123456Z")

    Examples:
        # Get current timestamp in ISO8601 format
        timestamp = time_now_iso8601()
        print(f"Current time: {timestamp}")

        # Use in logs
        print(f"[{time_now_iso8601()}] Application started")

        # Embed in JSON
        event = {
            "timestamp": time_now_iso8601(),
            "event": "user_login",
            "user_id": 12345,
        }

        # API calls often require ISO8601
        api_request = {
            "created_at": time_now_iso8601(),
            "data": "important-info",
        }
    """
    return time.iso8601()

# ============================================================================
# Timer Functions for Performance Measurement
# ============================================================================

def time_timer_start() -> int:
    """
    Creates and starts a new timer for measuring elapsed time.

    Returns a timer handle that can be used with other timer functions
    to measure how much time has passed since the timer was created.

    Returns:
        int: Timer handle (unique identifier for this timer)

    Examples:
        # Measure operation duration
        timer = time_timer_start()
        # ... perform work ...
        elapsed_ms = time_timer_elapsed_ms(timer)
        print(f"Work took {elapsed_ms} milliseconds")
        time_timer_stop(timer)

        # Time multiple operations
        timers = {}
        timers["fetch"] = time_timer_start()
        data = fetch_data()
        timers["fetch_duration"] = time_timer_elapsed_ms(timers["fetch"])

        timers["process"] = time_timer_start()
        result = process_data(data)
        timers["process_duration"] = time_timer_elapsed_ms(timers["process"])

        timers["store"] = time_timer_start()
        store_result(result)
        timers["store_duration"] = time_timer_elapsed_ms(timers["store"])

        print(f"Timings: {timers}")
    """
    return time.timer()

def time_timer_elapsed_ms(timer_id: int) -> int:
    """
    Returns the elapsed milliseconds for a running timer.

    Queries how much time has passed since the timer was started or reset.
    The timer continues running and can be queried multiple times.

    Args:
        timer_id: Timer handle returned from time_timer_start()

    Returns:
        int: Milliseconds elapsed since timer creation/reset

    Raises:
        Error: If the timer handle is invalid

    Examples:
        # Check elapsed time during operation
        timer = time_timer_start()

        for i in range(100):
            data = process_item(i)
            if i % 10 == 0:
                elapsed = time_timer_elapsed_ms(timer)
                print(f"Progress: {i}, Elapsed: {elapsed} ms")

        time_timer_stop(timer)

        # Warn if operation takes too long
        timer = time_timer_start()
        result = long_running_operation()
        elapsed = time_timer_elapsed_ms(timer)
        if elapsed > 5000:  # More than 5 seconds
            print(f"Warning: Operation took {elapsed} ms")
        time_timer_stop(timer)
    """
    return time.timer_elapsed_ms(timer_id)

def time_timer_elapsed_ns(timer_id: int) -> int:
    """
    Returns the elapsed nanoseconds for a running timer.

    Queries elapsed time in nanoseconds since timer creation/reset.
    Use when you need nanosecond-precision timing for benchmarking.

    Args:
        timer_id: Timer handle returned from time_timer_start()

    Returns:
        int: Nanoseconds elapsed since timer creation/reset

    Raises:
        Error: If the timer handle is invalid

    Examples:
        # Measure operation with nanosecond precision
        timer = time_timer_start()
        result = fast_operation()
        elapsed_ns = time_timer_elapsed_ns(timer)
        elapsed_us = elapsed_ns / 1000  # Convert to microseconds
        print(f"Operation took {elapsed_us} microseconds")
        time_timer_stop(timer)

        # Benchmark comparison in nanoseconds
        timers = {
            "algorithm_a": time_timer_start(),
        }
        result_a = algorithm_a()
        timers["algorithm_a_ns"] = time_timer_elapsed_ns(timers["algorithm_a"])

        timers["algorithm_b"] = time_timer_start()
        result_b = algorithm_b()
        timers["algorithm_b_ns"] = time_timer_elapsed_ns(timers["algorithm_b"])

        if timers["algorithm_a_ns"] < timers["algorithm_b_ns"]:
            print("Algorithm A is faster")
    """
    return time.timer_elapsed_ns(timer_id)

def time_timer_reset(timer_id: int):
    """
    Resets a timer to zero without destroying the timer handle.

    Restarts the timer measurement from the current moment, clearing
    all previously elapsed time. The timer handle remains valid and
    can continue to be used.

    Args:
        timer_id: Timer handle returned from time_timer_start()

    Raises:
        Error: If the timer handle is invalid

    Examples:
        # Measure multiple intervals with the same timer
        timer = time_timer_start()

        # First interval
        result1 = operation_one()
        interval1 = time_timer_elapsed_ms(timer)
        print(f"Operation 1 took: {interval1} ms")

        time_timer_reset(timer)

        # Second interval
        result2 = operation_two()
        interval2 = time_timer_elapsed_ms(timer)
        print(f"Operation 2 took: {interval2} ms")

        # Calculate average time per operation
        timer = time_timer_start()
        total_count = 100
        for i in range(total_count):
            do_work()

        total_ms = time_timer_elapsed_ms(timer)
        avg_ms = total_ms / total_count
        print(f"Average time per operation: {avg_ms} ms")

        time_timer_stop(timer)
    """
    time.timer_reset(timer_id)

def time_timer_stop(timer_id: int):
    """
    Stops and removes a timer from the registry.

    Cleans up a timer handle, freeing its resources. After calling this,
    the timer handle can no longer be used. Call this when you're done
    measuring to prevent resource leaks.

    Args:
        timer_id: Timer handle returned from time_timer_start()

    Raises:
        Error: If the timer handle is invalid or already stopped

    Examples:
        # Proper timer lifecycle
        timer = time_timer_start()
        try:
            result = do_work()
        finally:
            elapsed = time_timer_elapsed_ms(timer)
            print(f"Work completed in {elapsed} ms")
            time_timer_stop(timer)

        # Managing multiple timers
        def benchmark_operations():
            timers = {
                "fetch": time_timer_start(),
                "process": None,
                "store": None,
            }

            # Phase 1: Fetch
            data = fetch()
            timers["fetch_time"] = time_timer_elapsed_ms(timers["fetch"])
            time_timer_stop(timers["fetch"])

            # Phase 2: Process
            timers["process"] = time_timer_start()
            processed = process(data)
            timers["process_time"] = time_timer_elapsed_ms(timers["process"])
            time_timer_stop(timers["process"])

            # Phase 3: Store
            timers["store"] = time_timer_start()
            store(processed)
            timers["store_time"] = time_timer_elapsed_ms(timers["store"])
            time_timer_stop(timers["store"])

            return timers
    """
    time.timer_drop(timer_id)
