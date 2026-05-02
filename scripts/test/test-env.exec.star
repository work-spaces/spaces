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
    "env_which",
    "env_which_all",
)
load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)

# Env module test results
env_results = {
    "get_has": {},
    "all": {},
    "cwd_chdir": {},
    "path_list": {},
    "which": {},
}

# ============================================================================
# get / has
# ============================================================================

# Missing variable without default → None
env_results["get_has"]["get_missing_no_default"] = env_get("_TEST_MISSING_VAR_XYZ99") == None

# Missing variable with explicit default → the default string, never None
env_results["get_has"]["get_missing_with_default"] = (
    env_get("_TEST_MISSING_VAR_XYZ99", default = "/custom/default") == "/custom/default"
)

# has() returns False for genuinely absent variable
env_results["get_has"]["has_missing_is_false"] = not env_has("_TEST_MISSING_VAR_XYZ99")

# get()/has() consistency for a common key
path_value = env_get("PATH")
env_results["get_has"]["path_has_matches_get"] = env_has("PATH") == (path_value != None)

# Default is NOT used when the variable is present
env_results["get_has"]["default_not_used_when_present"] = (
    not env_has("PATH") or env_get("PATH", default = "SHOULD_NOT_APPEAR") == path_value
)

# ============================================================================
# all
# ============================================================================

all_vars = env_all()

env_results["all"]["returns_nonempty"] = len(all_vars) > 0

env_results["all"]["path_key_matches_has"] = ("PATH" in all_vars) == env_has("PATH")

env_results["all"]["path_value_matches_get"] = (
    ("PATH" in all_vars and all_vars.get("PATH") == env_get("PATH")) or
    (not ("PATH" in all_vars) and env_get("PATH") == None)
)

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
# path_join_entries
# ============================================================================

# Joining a known pair produces a non-empty result containing both directories
joined_two = env_path_join(["/usr/bin", "/usr/local/bin"])
env_results["path_list"]["path_join_nonempty"] = len(joined_two) > 0
env_results["path_list"]["path_join_contains_first_entry"] = "/usr/bin" in joined_two
env_results["path_list"]["path_join_contains_second_entry"] = "/usr/local/bin" in joined_two

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
# Output Results
# ============================================================================

print("Env Module Test Results:")
print("========================")
print("")
print(json_dumps(env_results, is_pretty = True))
print("")
print("All env functions executed successfully!")
