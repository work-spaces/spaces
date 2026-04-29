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
    "explicit_cleanup": {},
    "uniqueness": {},
}

# ============================================================================
# Temporary (tmp) Module Tests
# ============================================================================

# Test tmp_dir - basic temporary directory creation
temp_dir = tmp_dir(prefix = "test_basic_")
tmp_results["directory_creation"]["tmp_dir_returns_string"] = type(temp_dir) == "string"
tmp_results["directory_creation"]["tmp_dir_path_not_empty"] = len(temp_dir) > 0
tmp_results["directory_creation"]["tmp_dir_path_contains_prefix"] = "test_basic_" in temp_dir

# Verify directory exists immediately after creation
tmp_results["directory_creation"]["tmp_dir_creates_directory"] = fs_exists(temp_dir)

# Test tmp_file - basic temporary file creation
temp_file = tmp_file(suffix = ".test")
tmp_results["file_creation"]["tmp_file_returns_string"] = type(temp_file) == "string"
tmp_results["file_creation"]["tmp_file_path_not_empty"] = len(temp_file) > 0
tmp_results["file_creation"]["tmp_file_has_suffix"] = temp_file.endswith(".test")

# Verify file exists immediately after creation
tmp_results["file_creation"]["tmp_file_creates_file"] = fs_exists(temp_file)
file_content = fs_read_text(temp_file)
tmp_results["file_creation"]["tmp_file_is_initially_empty"] = len(file_content) == 0

# Test tmp_file with different suffix
temp_log = tmp_file(suffix = ".log")
tmp_results["file_creation"]["tmp_file_with_log_suffix"] = (
    temp_log.endswith(".log") and fs_exists(temp_log)
)

# Test tmp_dir_keep - persistent directory creation
keep_dir = tmp_dir_keep(prefix = "keep_test_")
tmp_results["persistent_resources"]["tmp_dir_keep_returns_string"] = type(keep_dir) == "string"
tmp_results["persistent_resources"]["tmp_dir_keep_path_contains_prefix"] = "keep_test_" in keep_dir
tmp_results["persistent_resources"]["tmp_dir_keep_creates_directory"] = fs_exists(keep_dir)

# Test writing to temp file and reading it back
fs_write_text(temp_file, "test data")
temp_content = fs_read_text(temp_file)
tmp_results["file_creation"]["tmp_file_write_and_read"] = temp_content == "test data"

# ============================================================================
# Uniqueness Tests
# ============================================================================

# Every creation call must return a distinct path.
uniq_dir_a = tmp_dir(prefix = "uniq_")
uniq_dir_b = tmp_dir(prefix = "uniq_")
uniq_file_a = tmp_file(suffix = ".u")
uniq_file_b = tmp_file(suffix = ".u")

tmp_results["uniqueness"]["two_dirs_have_distinct_paths"] = uniq_dir_a != uniq_dir_b
tmp_results["uniqueness"]["two_files_have_distinct_paths"] = uniq_file_a != uniq_file_b
tmp_results["uniqueness"]["dir_and_file_have_distinct_paths"] = uniq_dir_a != uniq_file_a

# Clean up the uniqueness test resources along with the main ones below.

# ============================================================================
# cleanup_all — removes non-keep resources, preserves keep resources
# ============================================================================
tmp_cleanup_all()

# Verify regular temp resources are deleted
tmp_results["cleanup_operations"]["cleanup_all_removes_temp_dir"] = not fs_exists(temp_dir)
tmp_results["cleanup_operations"]["cleanup_all_removes_temp_file"] = not fs_exists(temp_file)
tmp_results["cleanup_operations"]["cleanup_all_removes_temp_log"] = not fs_exists(temp_log)
tmp_results["cleanup_operations"]["cleanup_all_removes_uniq_dirs"] = (
    not fs_exists(uniq_dir_a) and not fs_exists(uniq_dir_b)
)
tmp_results["cleanup_operations"]["cleanup_all_removes_uniq_files"] = (
    not fs_exists(uniq_file_a) and not fs_exists(uniq_file_b)
)

# keep_dir must still be present — cleanup_all must not touch it
tmp_results["cleanup_operations"]["cleanup_all_keeps_persistent_dir"] = fs_exists(keep_dir)

# Calling cleanup_all on an already-empty (non-keep) registry is a no-op
tmp_cleanup_all()
tmp_results["cleanup_operations"]["cleanup_all_on_empty_registry_is_noop"] = fs_exists(keep_dir)

# ============================================================================
# Multiple resources, then cleanup_all
# ============================================================================
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

tmp_cleanup_all()

tmp_results["cleanup_operations"]["cleanup_all_removes_multiple_dirs"] = (
    not fs_exists(multi_dir_1) and
    not fs_exists(multi_dir_2)
)
tmp_results["cleanup_operations"]["cleanup_all_removes_multiple_files"] = (
    not fs_exists(multi_file_1) and
    not fs_exists(multi_file_2)
)

# ============================================================================
# Explicit per-path cleanup via tmp_cleanup(path)
# ============================================================================

explicit_dir = tmp_dir(prefix = "explicit_dir_")
explicit_file = tmp_file(suffix = ".explicit")
bystander_dir = tmp_dir(prefix = "bystander_")

tmp_results["explicit_cleanup"]["resources_exist_before_explicit_cleanup"] = (
    fs_exists(explicit_dir) and
    fs_exists(explicit_file) and
    fs_exists(bystander_dir)
)

# Clean up only explicit_dir and explicit_file, leaving bystander_dir alone.
tmp_cleanup(explicit_dir)
tmp_cleanup(explicit_file)

tmp_results["explicit_cleanup"]["explicit_cleanup_removes_dir"] = not fs_exists(explicit_dir)
tmp_results["explicit_cleanup"]["explicit_cleanup_removes_file"] = not fs_exists(explicit_file)
tmp_results["explicit_cleanup"]["explicit_cleanup_leaves_bystander_intact"] = fs_exists(bystander_dir)

# Drain remaining tracked resources (bystander_dir) so we leave no test artifacts.
tmp_cleanup_all()
tmp_results["explicit_cleanup"]["bystander_removed_by_final_cleanup_all"] = not fs_exists(bystander_dir)

# ============================================================================
# Final cleanup of the persistent keep_dir test artifact
# ============================================================================

# keep_dir was intentionally not tracked for auto-cleanup.  Remove it now so
# that repeated test runs do not accumulate directories in the system temp dir.
tmp_cleanup(keep_dir)
tmp_results["persistent_resources"]["keep_dir_removed_after_explicit_cleanup"] = not fs_exists(keep_dir)

# ============================================================================
# Output Results
# ============================================================================

print("Temporary (tmp) Module Test Results:")
print("====================================")
print("")
print(json_dumps(tmp_results, is_pretty = True))
print("")
print("All tmp functions executed successfully!")
