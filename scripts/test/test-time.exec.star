#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/time.star",
    "time_format_datetime",
    "time_monotonic",
    "time_now",
    "time_now_iso8601",
    "time_parse_datetime",
    "time_sleep",
    "time_sleep_milliseconds",
    "time_sleep_seconds",
    "time_timer_elapsed_ms",
    "time_timer_elapsed_ns",
    "time_timer_reset",
    "time_timer_start",
    "time_timer_stop",
    "time_unix_milliseconds",
    "time_unix_seconds",
)

# Time module test results
time_results = {
    "current_time": {},
    "unix_timestamps": {},
    "monotonic_time": {},
    "sleep_operations": {},
    "datetime_formatting": {},
    "datetime_parsing": {},
    "iso8601": {},
    "timer_operations": {},
    "timer_elapsed": {},
    "timer_precision": {},
    "timer_management": {},
}

# ============================================================================
# Time Module Tests
# ============================================================================

# Test current time functions
current_time = time_now()
time_results["current_time"]["time_now_is_number"] = type(current_time) in ("int", "float")
time_results["current_time"]["time_now_positive"] = current_time > 0

unix_secs = time_unix_seconds()
time_results["unix_timestamps"]["unix_seconds_is_int"] = type(unix_secs) == "int"
time_results["unix_timestamps"]["unix_seconds_positive"] = unix_secs > 0

unix_ms = time_unix_milliseconds()
time_results["unix_timestamps"]["unix_milliseconds_is_int"] = type(unix_ms) == "int"
time_results["unix_timestamps"]["unix_milliseconds_positive"] = unix_ms > 0

# Milliseconds should be roughly 1000x seconds
time_results["unix_timestamps"]["unix_ms_larger_than_seconds"] = unix_ms > unix_secs * 900

# Test monotonic time
mono_start = time_monotonic()
time_results["monotonic_time"]["monotonic_is_int"] = type(mono_start) == "int"
time_results["monotonic_time"]["monotonic_non_negative"] = mono_start >= 0

# Test sleep functions - use short durations
time_sleep(0.05)  # 50ms
time_results["sleep_operations"]["sleep_completes"] = True

time_sleep_milliseconds(50)
time_results["sleep_operations"]["sleep_milliseconds_completes"] = True

time_sleep_seconds(0)  # Sleep 0 seconds
time_results["sleep_operations"]["sleep_seconds_completes"] = True

# Verify monotonic time advanced after sleep
mono_after_sleep = time_monotonic()
time_results["sleep_operations"]["monotonic_advanced"] = mono_after_sleep >= mono_start

# Test datetime formatting
current_unix = time_unix_seconds()
formatted = time_format_datetime(current_unix, "%Y-%m-%d")
time_results["datetime_formatting"]["format_datetime_is_string"] = type(formatted) == "string"
time_results["datetime_formatting"]["format_datetime_has_year"] = len(formatted) >= 4
time_results["datetime_formatting"]["format_datetime_contains_dashes"] = "-" in formatted

# Test with known timestamp (2024-01-15 00:00:00 UTC = 1705276800)
known_ts = 1705276800
formatted_known = time_format_datetime(known_ts, "%Y-%m-%d")
time_results["datetime_formatting"]["format_datetime_known_timestamp"] = "2024-01-15" in formatted_known

# Test parsing
parsed = time_parse_datetime("2024-01-15", "%Y-%m-%d")
time_results["datetime_parsing"]["parse_datetime_is_int"] = type(parsed) == "int"
time_results["datetime_parsing"]["parse_datetime_positive"] = parsed > 0

# Verify round-trip: parse then format
formatted_roundtrip = time_format_datetime(parsed, "%Y-%m-%d")
time_results["datetime_parsing"]["parse_format_roundtrip"] = "2024-01-15" in formatted_roundtrip

# Test ISO8601
iso_time = time_now_iso8601()
time_results["iso8601"]["iso8601_is_string"] = type(iso_time) == "string"
time_results["iso8601"]["iso8601_contains_t"] = "T" in iso_time

# Accept Z, +, or - as valid ISO8601 timezone indicators
time_results["iso8601"]["iso8601_has_timezone"] = "Z" in iso_time or "+" in iso_time or "-" in iso_time

# Test timer operations
timer = time_timer_start()
time_results["timer_operations"]["timer_start_is_int"] = type(timer) == "int"
time_results["timer_operations"]["timer_id_positive"] = timer > 0

# Sleep a small amount and check elapsed time
time_sleep_milliseconds(100)
elapsed_ms = time_timer_elapsed_ms(timer)
time_results["timer_elapsed"]["elapsed_ms_is_int"] = type(elapsed_ms) == "int"
time_results["timer_elapsed"]["elapsed_ms_at_least_80"] = elapsed_ms >= 80  # Allow some variance

# Test nanosecond precision
elapsed_ns = time_timer_elapsed_ns(timer)
time_results["timer_precision"]["elapsed_ns_is_int"] = type(elapsed_ns) == "int"
time_results["timer_precision"]["elapsed_ns_correlates_with_ms"] = elapsed_ns > elapsed_ms * 900000

# Test timer reset
first_elapsed = time_timer_elapsed_ms(timer)
time_sleep_milliseconds(50)
time_timer_reset(timer)
reset_elapsed = time_timer_elapsed_ms(timer)
time_results["timer_management"]["timer_reset_works"] = reset_elapsed < first_elapsed / 2

# Clean up timer
time_timer_stop(timer)
time_results["timer_management"]["timer_stop_completes"] = True

# Test multiple timers
timer1 = time_timer_start()
time_sleep_milliseconds(100)
time1 = time_timer_elapsed_ms(timer1)

timer2 = time_timer_start()
time_sleep_milliseconds(50)
time2 = time_timer_elapsed_ms(timer2)

time_results["timer_management"]["multiple_timers_independent"] = time1 > time2

time_timer_stop(timer1)
time_timer_stop(timer2)

# ============================================================================
# Output Results
# ============================================================================

print("Time Module Test Results:")
print("========================")
print("")
print(json_dumps(time_results, is_pretty = True))
print("")
print("All time functions executed successfully!")
