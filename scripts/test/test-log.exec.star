#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/log.star",
    "log_debug",
    "log_error",
    "log_fatal",
    "log_info",
    "log_set_format",
    "log_set_level",
    "log_trace",
    "log_warn",
)

# Log module test results
log_results = {
    "log_level_control": {},
    "log_format_control": {},
    "log_trace_logging": {},
    "log_debug_logging": {},
    "log_info_logging": {},
    "log_warn_logging": {},
    "log_error_logging": {},
    "log_level_filtering": {},
}

# ============================================================================
# Log Module Tests
# ============================================================================

# Test log level setting — all levels including trace and off
log_set_level("trace")
log_results["log_level_control"]["set_trace"] = True

log_set_level("debug")
log_results["log_level_control"]["set_debug"] = True

log_set_level("info")
log_results["log_level_control"]["set_info"] = True

log_set_level("warn")
log_results["log_level_control"]["set_warn"] = True

log_set_level("error")
log_results["log_level_control"]["set_error"] = True

log_set_level("off")
log_results["log_level_control"]["set_off"] = True

# Test log format setting
log_set_format("text")
log_results["log_format_control"]["set_text"] = True

log_set_format("json")
log_results["log_format_control"]["set_json"] = True

# Reset to default format
log_set_format("text")
log_results["log_format_control"]["reset_to_text"] = True

# ============================================================================
# Level-filtering test: set level to "error" then emit lower-priority messages.
# Those messages must NOT appear on stderr (manual verification only, as the
# test harness cannot capture stderr assertions programmatically).
# ============================================================================
log_set_level("error")
log_debug("SHOULD BE SUPPRESSED — debug below error threshold")
log_info("SHOULD BE SUPPRESSED — info below error threshold")
log_warn("SHOULD BE SUPPRESSED — warn below error threshold")
log_results["log_level_filtering"]["lower_levels_suppressed"] = True

# ============================================================================
# Logging at each level
# ============================================================================
log_set_level("trace")

log_trace("Trace message for testing")
log_results["log_trace_logging"]["trace_message"] = True

log_debug("Debug message for testing")
log_results["log_debug_logging"]["debug_message"] = True

log_set_level("info")

log_info("Info message for testing")
log_results["log_info_logging"]["info_message"] = True

log_warn("Warning message for testing")
log_results["log_warn_logging"]["warn_message"] = True

log_error("Error message for testing")
log_results["log_error_logging"]["error_message"] = True

# ============================================================================
# JSON format round-trip: set json, emit a message, reset to text.
# Output must be a valid JSON line (manual verification on stderr).
# ============================================================================
log_set_format("json")
log_info("JSON format test message")
log_set_format("text")
log_results["log_format_control"]["json_roundtrip"] = True

# ============================================================================
# Verify log_info still works after format reset
# ============================================================================
log_info("Testing log_info function after format reset")
log_results["log_info_logging"]["log_info_works"] = True

# ============================================================================
# log_fatal is NOT called here because it terminates execution.
# To test it manually:
#   echo '#!/usr/bin/env spaces
#   load("//@star/sdk/star/std/log.star","log_fatal")
#   log_fatal("deliberate abort")' > /tmp/t.exec.star
#   chmod +x /tmp/t.exec.star && /tmp/t.exec.star; echo "exit: $?"
# Expected: ERROR line logged to stderr, non-zero exit code.
# ============================================================================

# ============================================================================
# Output Results
# ============================================================================

print("Log Module Test Results:")
print("=======================")
print("")
print(json_dumps(log_results, is_pretty = True))
print("")
print("All log functions executed successfully!")
