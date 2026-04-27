#!/usr/bin/env spaces

load("//@star/sdk/star/std/fs.star", "fs_exists", "fs_read_text", "fs_write_text")
load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/tmp.star",
    "tmp_cleanup",
    "tmp_cleanup_all",
    "tmp_dir",
    "tmp_dir_keep",
    "tmp_file",
)

# Temporary module test results
tmp_results = {
    "directory_creation": {},
    "file_creation": {},
    "persistent_resources": {},
    "cleanup_operations": {},
}

# ============================================================================
# Temporary (tmp) Module Tests
# ============================================================================

# Test tmp_dir - basic temporary directory creation
temp_dir = tmp_dir(prefix = "test_basic_")
tmp_results["directory_creation"]["tmp_dir_returns_string"] = type(temp_dir) == "string"
tmp_results["directory_creation"]["tmp_dir_path_not_empty"] = len(temp_dir) > 0
tmp_results["directory_creation"]["tmp_dir_path_contains_prefix"] = "test_basic_" in temp_dir

# Verify directory exists
tmp_results["directory_creation"]["tmp_dir_creates_directory"] = fs_exists(temp_dir)

# Test tmp_file - basic temporary file creation
temp_file = tmp_file(suffix = ".test")
tmp_results["file_creation"]["tmp_file_returns_string"] = type(temp_file) == "string"
tmp_results["file_creation"]["tmp_file_path_not_empty"] = len(temp_file) > 0
tmp_results["file_creation"]["tmp_file_has_suffix"] = ".test" in temp_file

# Verify file exists
tmp_results["file_creation"]["tmp_file_creates_file"] = fs_exists(temp_file)
file_content = fs_read_text(temp_file)
tmp_results["file_creation"]["tmp_file_is_initially_empty"] = len(file_content) == 0

# Test tmp_file with different suffix
temp_log = tmp_file(suffix = ".log")
tmp_results["file_creation"]["tmp_file_with_log_suffix"] = ".log" in temp_log and fs_exists(temp_log)

# Test tmp_dir_keep - persistent directory creation
keep_dir = tmp_dir_keep(prefix = "keep_test_")
tmp_results["persistent_resources"]["tmp_dir_keep_returns_string"] = type(keep_dir) == "string"
tmp_results["persistent_resources"]["tmp_dir_keep_path_contains_prefix"] = "keep_test_" in keep_dir
tmp_results["persistent_resources"]["tmp_dir_keep_creates_directory"] = fs_exists(keep_dir)

# Test writing to temp file
fs_write_text(temp_file, "test data")
temp_content = fs_read_text(temp_file)
tmp_results["file_creation"]["tmp_file_write_and_read"] = temp_content == "test data"

# Test tmp_cleanup_all - cleanup all tracked resources
tmp_cleanup_all()

# Verify regular temp resources are deleted
tmp_results["cleanup_operations"]["cleanup_all_removes_temp_dir"] = not fs_exists(temp_dir)
tmp_results["cleanup_operations"]["cleanup_all_removes_temp_file"] = not fs_exists(temp_file)
tmp_results["cleanup_operations"]["cleanup_all_removes_temp_log"] = not fs_exists(temp_log)

# Verify keep_dir still exists
tmp_results["cleanup_operations"]["cleanup_all_keeps_persistent_dir"] = fs_exists(keep_dir)

# Test multiple temporary directories
multi_dir_1 = tmp_dir(prefix = "multi_1_")
multi_dir_2 = tmp_dir(prefix = "multi_2_")
multi_file_1 = tmp_file(suffix = ".tmp1")
multi_file_2 = tmp_file(suffix = ".tmp2")

tmp_results["cleanup_operations"]["multiple_temp_resources_created"] = (
    fs_exists(multi_dir_1) and
    fs_exists(multi_dir_2) and
    fs_exists(multi_file_1) and
    fs_exists(multi_file_2)
)

# Clean up all new resources
tmp_cleanup_all()

# Verify all regular resources cleaned up
tmp_results["cleanup_operations"]["cleanup_all_removes_multiple_dirs"] = (
    not fs_exists(multi_dir_1) and
    not fs_exists(multi_dir_2)
)
tmp_results["cleanup_operations"]["cleanup_all_removes_multiple_files"] = (
    not fs_exists(multi_file_1) and
    not fs_exists(multi_file_2)
)

# Clean up keep_dir manually to not leave test artifacts
# Note: In practice, keep_dir would persist for long-term use
# For testing, we'll verify it exists but leave it

# ============================================================================
# Output Results
# ============================================================================

print("Temporary (tmp) Module Test Results:")
print("====================================")
print("")
print(json_dumps(tmp_results, is_pretty = True))
print("")
print("All tmp functions executed successfully!")
