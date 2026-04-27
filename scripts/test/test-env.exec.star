#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/env.star",
    "env_all",
    "env_chdir",
    "env_cwd",
    "env_get",
    "env_has",
    "env_path_join",
    "env_path_list",
    "env_set",
    "env_unset",
    "env_which",
    "env_which_all",
)
load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)

# Env module test results
env_results = {
    "get_set_has": {},
    "unset": {},
    "all": {},
    "cwd_chdir": {},
    "path_list": {},
    "which": {},
}

# ============================================================================
# get / set / has
# ============================================================================

# Basic round-trip
env_set("_TEST_ENV_FOO", "hello_world")
env_results["get_set_has"]["set_then_get"] = env_get("_TEST_ENV_FOO") == "hello_world"
env_results["get_set_has"]["has_after_set"] = env_has("_TEST_ENV_FOO")

# Override an existing variable
env_set("_TEST_ENV_FOO", "new_value")
env_results["get_set_has"]["overwrite_value"] = env_get("_TEST_ENV_FOO") == "new_value"

# Missing variable without default → None  (D2 fix: distinguishable from "")
env_results["get_set_has"]["get_missing_no_default"] = env_get("_TEST_MISSING_VAR_XYZ99") == None

# Missing variable with explicit default → the default string, never None
env_results["get_set_has"]["get_missing_with_default"] = (
    env_get("_TEST_MISSING_VAR_XYZ99", default = "/custom/default") == "/custom/default"
)

# has() returns False for genuinely absent variable
env_results["get_set_has"]["has_missing_is_false"] = not env_has("_TEST_MISSING_VAR_XYZ99")

# A variable explicitly set to the empty string is present: get returns "", not None
env_set("_TEST_ENV_EMPTY", "")
env_results["get_set_has"]["empty_value_get_is_empty_str"] = env_get("_TEST_ENV_EMPTY") == ""
env_results["get_set_has"]["empty_value_has_is_true"] = env_has("_TEST_ENV_EMPTY")

# Key D2 assertion: "set to empty" and "not set" are now distinguishable
env_results["get_set_has"]["none_and_empty_are_distinct"] = (
    env_get("_TEST_ENV_EMPTY") != env_get("_TEST_MISSING_VAR_XYZ99")
)

# Default is NOT used when the variable is present (even when its value is "")
env_results["get_set_has"]["default_not_used_when_present"] = (
    env_get("_TEST_ENV_EMPTY", default = "SHOULD_NOT_APPEAR") == ""
)

# ============================================================================
# unset
# ============================================================================

env_set("_TEST_ENV_TO_UNSET", "will_be_removed")
env_results["unset"]["has_before_unset"] = env_has("_TEST_ENV_TO_UNSET")

env_unset("_TEST_ENV_TO_UNSET")
env_results["unset"]["has_after_unset"] = not env_has("_TEST_ENV_TO_UNSET")

# After unset with no default, get returns None — not ""
env_results["unset"]["get_after_unset_is_none"] = env_get("_TEST_ENV_TO_UNSET") == None

# After unset with a default, get returns the default
env_results["unset"]["get_after_unset_with_default"] = (
    env_get("_TEST_ENV_TO_UNSET", default = "fallback") == "fallback"
)

# Unsetting a variable that was never set is a safe no-op
env_unset("_TEST_ENV_NEVER_EXISTED_XYZ99")
env_results["unset"]["unset_missing_is_noop"] = not env_has("_TEST_ENV_NEVER_EXISTED_XYZ99")

# ============================================================================
# all
# ============================================================================

env_set("_TEST_ENV_ALL_SENTINEL", "sentinel_value_42")
all_vars = env_all()

env_results["all"]["returns_nonempty"] = len(all_vars) > 0
env_results["all"]["contains_sentinel"] = "_TEST_ENV_ALL_SENTINEL" in all_vars
env_results["all"]["sentinel_value_correct"] = (
    all_vars.get("_TEST_ENV_ALL_SENTINEL") == "sentinel_value_42"
)

# PATH is almost always present
env_results["all"]["path_present"] = "PATH" in all_vars

# Snapshot consistency: has() and all() agree on the sentinel key
env_results["all"]["snapshot_matches_has"] = (
    env_has("_TEST_ENV_ALL_SENTINEL") and "_TEST_ENV_ALL_SENTINEL" in all_vars
)

# After unsetting the sentinel, a fresh all() no longer contains it
env_unset("_TEST_ENV_ALL_SENTINEL")
all_after_unset = env_all()
env_results["all"]["unset_removed_from_all"] = "_TEST_ENV_ALL_SENTINEL" not in all_after_unset

# ============================================================================
# cwd / chdir
# ============================================================================

original_dir = env_cwd()
env_results["cwd_chdir"]["cwd_returns_nonempty"] = len(original_dir) > 0

# On Unix the CWD is absolute (starts with /); on Windows it starts with a drive letter
env_results["cwd_chdir"]["cwd_looks_absolute"] = (
    original_dir.startswith("/") or
    (len(original_dir) >= 2 and original_dir[1] == ":")
)

# Change to /tmp and verify the directory actually changed
env_chdir("/tmp")
after_chdir = env_cwd()
env_results["cwd_chdir"]["chdir_changes_cwd"] = after_chdir != original_dir

# /tmp on macOS resolves via symlink to /private/tmp; just verify it's non-empty
env_results["cwd_chdir"]["chdir_new_dir_nonempty"] = len(after_chdir) > 0

# Restore the original directory and verify we're back
env_chdir(original_dir)
env_results["cwd_chdir"]["chdir_restore"] = env_cwd() == original_dir

# Calling cwd() twice without any chdir in between must be stable
env_results["cwd_chdir"]["cwd_stable"] = env_cwd() == env_cwd()

# ============================================================================
# path_list
# ============================================================================

path_entries = env_path_list()
env_results["path_list"]["returns_nonempty"] = len(path_entries) > 0
env_results["path_list"]["entries_are_strings"] = len(path_entries) > 0 and len(path_entries[0]) > 0

# split_paths handles platform separators; no raw colon should leak into a Unix entry
# (a Windows drive letter "C:" has a colon but is at position 1)
all_entries_clean = True
for entry in path_entries:
    if ":" in entry and not entry.startswith("/") and not (len(entry) >= 2 and entry[1] == ":"):
        all_entries_clean = False
env_results["path_list"]["no_raw_unix_separators"] = all_entries_clean

# Sanity-check count
env_results["path_list"]["count_sane"] = len(path_entries) >= 1 and len(path_entries) <= 1000

# ============================================================================
# path_join_entries (D7 fix: inverse of path_list)
# ============================================================================

# Joining a known pair produces a non-empty result containing both directories
joined_two = env_path_join(["/usr/bin", "/usr/local/bin"])
env_results["path_list"]["path_join_nonempty"] = len(joined_two) > 0
env_results["path_list"]["path_join_contains_first_entry"] = "/usr/bin" in joined_two
env_results["path_list"]["path_join_contains_second_entry"] = "/usr/local/bin" in joined_two

# Round-trip: split PATH then rejoin — the rejoined string must contain the same entries
original_path = env_get("PATH", default = "")
if original_path != None and len(original_path) > 0:
    round_tripped = env_path_join(path_entries)

    # Re-splitting the rejoined string must yield the same list
    env_set("_TEST_PATH_BACKUP", original_path)
    env_set("PATH", round_tripped)
    re_split = env_path_list()
    env_results["path_list"]["path_join_round_trip_count"] = len(re_split) == len(path_entries)
    env_set("PATH", original_path)  # restore
    env_unset("_TEST_PATH_BACKUP")
else:
    env_results["path_list"]["path_join_round_trip_count"] = True  # skip on empty PATH

# Joining a single entry returns that entry unchanged
single = env_path_join(["/single/dir"])
env_results["path_list"]["path_join_single_entry"] = single == "/single/dir"

# Joining an empty list returns an empty string
env_results["path_list"]["path_join_empty_list"] = env_path_join([]) == ""

# ============================================================================
# which / which_all
# ============================================================================

# `sh` is present on every Unix-like system
sh_path = env_which("sh")
env_results["which"]["finds_sh"] = len(sh_path) > 0
env_results["which"]["sh_path_looks_absolute"] = (
    sh_path.startswith("/") or
    (len(sh_path) >= 2 and sh_path[1] == ":")
)
env_results["which"]["sh_path_ends_with_sh"] = (
    sh_path.endswith("sh") or sh_path.endswith("sh.exe")
)

# A non-existent command returns empty string — not None, not an error
env_results["which"]["not_found_returns_empty"] = (
    env_which("_totally_fake_cmd_xyzzy_12345") == ""
)

# which_all for a known command returns a non-empty list whose first entry matches which()
all_sh = env_which_all("sh")
env_results["which"]["which_all_nonempty"] = len(all_sh) > 0
env_results["which"]["which_all_first_matches_which"] = (
    len(all_sh) > 0 and all_sh[0] == sh_path
)

# which_all for a non-existent command returns an empty list
env_results["which"]["which_all_not_found_empty_list"] = (
    env_which_all("_totally_fake_cmd_xyzzy_12345") == []
)

# All entries in which_all are non-empty strings
all_entries_valid = True
for p in all_sh:
    if len(p) == 0:
        all_entries_valid = False
env_results["which"]["which_all_all_entries_nonempty"] = all_entries_valid

# Duplicate suppression: which_all must not return the same path twice
seen = {}
has_duplicate = False
for p in all_sh:
    if p in seen:
        has_duplicate = True
    seen[p] = True
env_results["which"]["which_all_no_duplicates"] = not has_duplicate

# ============================================================================
# Cleanup: remove any test variables still in the environment
# ============================================================================

env_unset("_TEST_ENV_FOO")
env_unset("_TEST_ENV_EMPTY")

# ============================================================================
# Output Results
# ============================================================================

print("Env Module Test Results:")
print("========================")
print("")
print(json_dumps(env_results, is_pretty = True))
print("")
print("All env functions executed successfully!")
