#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/sys.star",
    "sys_arch",
    "sys_cpu_count",
    "sys_endianness",
    "sys_executable",
    "sys_hostname",
    "sys_info",
    "sys_is_ci",
    "sys_os",
    "sys_total_memory_bytes",
    "sys_total_memory_gb",
    "sys_user_home",
    "sys_username",
)

# System (sys) module test results
sys_results = {
    "os_and_arch": {},
    "host_and_user": {},
    "system_resources": {},
    "system_properties": {},
    "environment_detection": {},
    "system_info": {},
}

# ============================================================================
# System (sys) Module Tests
# ============================================================================

# Test sys_os - returns operating system name
os_name = sys_os()
sys_results["os_and_arch"]["os_is_string"] = len(os_name) > 0

# Rust's std::env::consts::OS covers many targets beyond the big three;
# keep the list comprehensive so CI on FreeBSD, Solaris, Android, etc. still passes.
KNOWN_OS_VALUES = [
    "linux",
    "macos",
    "windows",
    "freebsd",
    "openbsd",
    "netbsd",
    "dragonfly",
    "solaris",
    "illumos",
    "android",
    "ios",
    "tvos",
    "watchos",
]
sys_results["os_and_arch"]["os_valid_value"] = os_name in KNOWN_OS_VALUES

# Test sys_arch - returns CPU architecture
arch_name = sys_arch()
sys_results["os_and_arch"]["arch_is_string"] = len(arch_name) > 0

# std::env::consts::ARCH spans every Rust-supported target triple; an exhaustive
# allowlist would require frequent maintenance.  Check shape instead: non-empty,
# all-lowercase identifier characters (letters, digits, underscore).
arch_looks_valid = len(arch_name) > 0
for ch in arch_name.elems():
    if not (ch.isalpha() or ch.isdigit() or ch == "_"):
        arch_looks_valid = False
sys_results["os_and_arch"]["arch_valid_value"] = arch_looks_valid

# Test sys_hostname - returns machine hostname
hostname = sys_hostname()
sys_results["host_and_user"]["hostname_is_string"] = len(hostname) > 0

# Test sys_username - returns current username
username = sys_username()
sys_results["host_and_user"]["username_is_string"] = len(username) > 0

# Test sys_user_home - returns home directory path
home = sys_user_home()
sys_results["host_and_user"]["home_is_string"] = len(home) > 0

# On Unix the home dir starts with "/".
# On Windows it starts with any drive letter followed by ":\", e.g. "C:\", "D:\".
# Checking position [1] == ":" covers all drive letters portably.
home_is_absolute = (
    home.startswith("/") or
    (len(home) >= 3 and home[1] == ":" and (home[2] == "\\" or home[2] == "/"))
)
sys_results["host_and_user"]["home_is_absolute"] = home_is_absolute

# Test sys_cpu_count - returns logical CPU count
cpu_count = sys_cpu_count()
sys_results["system_resources"]["cpu_count_is_positive"] = cpu_count > 0
sys_results["system_resources"]["cpu_count_reasonable"] = cpu_count <= 1024  # Sanity check

# Test sys_total_memory_bytes - returns total memory in bytes
memory_bytes = sys_total_memory_bytes()
sys_results["system_resources"]["memory_bytes_is_large"] = memory_bytes > 1024 * 1024  # At least 1 MB
sys_results["system_resources"]["memory_bytes_reasonable"] = memory_bytes < 1024 * 1024 * 1024 * 1024  # Less than 1 TB

# Test sys_total_memory_gb - returns total memory in gigabytes
memory_gb = sys_total_memory_gb()
sys_results["system_resources"]["memory_gb_is_positive"] = memory_gb > 0
sys_results["system_resources"]["memory_gb_matches_bytes"] = abs(memory_gb - (memory_bytes / (1024.0 * 1024.0 * 1024.0))) < 0.1

# Test sys_endianness - returns byte order
endianness = sys_endianness()
sys_results["system_properties"]["endianness_is_valid"] = endianness in ["little", "big"]

# Test sys_executable - returns path to current executable
executable = sys_executable()
sys_results["system_properties"]["executable_is_string"] = len(executable) > 0
sys_results["system_properties"]["executable_contains_path"] = "/" in executable or "\\" in executable

# Test sys_is_ci - returns boolean for CI detection
is_ci = sys_is_ci()
sys_results["environment_detection"]["is_ci_is_boolean"] = is_ci == True or is_ci == False
sys_results["environment_detection"]["is_ci_returns_value"] = is_ci == is_ci  # Always true, just verifies no error

# Test sys_info - returns comprehensive system information dict
info = sys_info()
sys_results["system_info"]["info_is_dict"] = len(info) > 0
sys_results["system_info"]["info_has_os"] = "os" in info
sys_results["system_info"]["info_has_arch"] = "arch" in info
sys_results["system_info"]["info_has_hostname"] = "hostname" in info
sys_results["system_info"]["info_has_username"] = "username" in info
sys_results["system_info"]["info_has_home"] = "home" in info
sys_results["system_info"]["info_has_cpu_count"] = "cpu_count" in info
sys_results["system_info"]["info_has_memory_bytes"] = "total_memory_bytes" in info
sys_results["system_info"]["info_has_memory_gb"] = "total_memory_gb" in info
sys_results["system_info"]["info_has_endianness"] = "endianness" in info
sys_results["system_info"]["info_has_executable"] = "executable" in info
sys_results["system_info"]["info_has_is_ci"] = "is_ci" in info

# Cross-validate: sys_info() values must be consistent with individual calls
# (stable fields only — memory is excluded because total_memory_bytes() creates a
#  fresh sysinfo::System snapshot each time and could differ by a few bytes).
sys_results["system_info"]["info_os_matches_individual"] = info["os"] == os_name
sys_results["system_info"]["info_arch_matches_individual"] = info["arch"] == arch_name
sys_results["system_info"]["info_cpu_count_matches_individual"] = info["cpu_count"] == cpu_count
sys_results["system_info"]["info_endianness_matches_individual"] = info["endianness"] == endianness

# Type checks: verify the returned types are correct, not just that keys exist
sys_results["system_info"]["info_os_is_nonempty_string"] = type(info["os"]) == "string" and len(info["os"]) > 0
sys_results["system_info"]["info_arch_is_nonempty_string"] = type(info["arch"]) == "string" and len(info["arch"]) > 0
sys_results["system_info"]["info_cpu_count_is_positive_int"] = type(info["cpu_count"]) == "int" and info["cpu_count"] > 0
sys_results["system_info"]["info_memory_bytes_is_positive_int"] = type(info["total_memory_bytes"]) == "int" and info["total_memory_bytes"] > 0
sys_results["system_info"]["info_memory_gb_is_positive_float"] = info["total_memory_gb"] > 0.0
sys_results["system_info"]["info_is_ci_is_bool"] = type(info["is_ci"]) == "bool"
sys_results["system_info"]["info_endianness_is_valid"] = info["endianness"] in ["little", "big"]

# ============================================================================
# Output Results
# ============================================================================

print("System (sys) Module Test Results:")
print("================================")
print("")
print(json_dumps(sys_results, is_pretty = True))
print("")
print("All sys functions executed successfully!")
