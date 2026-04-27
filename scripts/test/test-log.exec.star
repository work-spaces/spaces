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
    "log_warn",
)

# Log module test results
log_results = {
    "log_level_control": {},
    "log_format_control": {},
    "log_level_logging": {},
    "log_info_logging": {},
    "log_warn_logging": {},
    "log_error_logging": {},
}

# ============================================================================
# Log Module Tests
# ============================================================================

# Test log level setting
log_set_level("debug")
log_results["log_level_control"]["set_debug"] = True

log_set_level("info")
log_results["log_level_control"]["set_info"] = True

log_set_level("warn")
log_results["log_level_control"]["set_warn"] = True

log_set_level("error")
log_results["log_level_control"]["set_error"] = True

# Test log format setting
log_set_format("text")
log_results["log_format_control"]["set_text"] = True

log_set_format("json")
log_results["log_format_control"]["set_json"] = True

# Reset to default format
log_set_format("text")

# Test logging at different levels
log_set_level("debug")
log_debug("Debug message for testing")
log_results["log_level_logging"]["debug_message"] = True

log_info("Info message for testing")
log_results["log_info_logging"]["info_message"] = True

log_warn("Warning message for testing")
log_results["log_warn_logging"]["warn_message"] = True

log_error("Error message for testing")
log_results["log_error_logging"]["error_message"] = True

# Test that log_info works correctly
log_info("Testing log_info function")
log_results["log_info_logging"]["log_info_works"] = True

# ============================================================================
# Output Results
# ============================================================================

print("Log Module Test Results:")
print("=======================")
print("")
print(json_dumps(log_results, is_pretty = True))
print("")
print("All log functions executed successfully!")
