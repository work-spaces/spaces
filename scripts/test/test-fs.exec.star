#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/fs.star",
    "fs_append_text",
    "fs_chmod",
    "fs_copy",
    "fs_exists",
    "fs_is_directory",
    "fs_is_file",
    "fs_is_symlink",
    "fs_is_text_file",
    "fs_metadata",
    "fs_mkdir",
    "fs_modified",
    "fs_move",
    "fs_read_bytes",
    "fs_read_directory",
    "fs_read_json",
    "fs_read_lines",
    "fs_read_link",
    "fs_read_text",
    "fs_read_toml",
    "fs_read_yaml",
    "fs_remove",
    "fs_set_permissions",
    "fs_size",
    "fs_symlink",
    "fs_touch",
    "fs_write_bytes",
    "fs_write_json",
    "fs_write_lines",
    "fs_write_string_atomic",
    "fs_write_text",
    "fs_write_toml",
    "fs_write_yaml",
)
load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/tmp.star",
    "tmp_cleanup_all",
    "tmp_dir",
)

# ============================================================================
# Setup: all test files go under a unique tmp directory
# ============================================================================

work_dir = tmp_dir(prefix = "test-fs-")

# Ensure a clean slate in case a previous run left behind state
# (tmp_dir reuses the same path when the counter hasn't advanced)
fs_remove(work_dir, recursive = True, missing_ok = True)
fs_mkdir(work_dir, exist_ok = True)

def p(rel):
    """Join work_dir with a relative path."""
    return work_dir + "/" + rel

results = {
    "text_io": {},
    "binary_io": {},
    "lines_io": {},
    "format_io": {},
    "existence_checks": {},
    "directory_ops": {},
    "copy_move": {},
    "remove": {},
    "symlinks": {},
    "metadata": {},
    "permissions": {},
    "is_text_file": {},
    "atomic_write": {},
    "touch": {},
}

# ============================================================================
# Text I/O
# ============================================================================

fs_write_text(p("hello.txt"), "Hello, World!")
results["text_io"]["write_and_read"] = fs_read_text(p("hello.txt")) == "Hello, World!"
results["text_io"]["file_exists_after_write"] = fs_exists(p("hello.txt"))

# Overwrite
fs_write_text(p("hello.txt"), "Overwritten")
results["text_io"]["overwrite"] = fs_read_text(p("hello.txt")) == "Overwritten"

# Append
fs_append_text(p("append.txt"), "line1\n")
fs_append_text(p("append.txt"), "line2\n")
appended = fs_read_text(p("append.txt"))
results["text_io"]["append_creates_file"] = fs_exists(p("append.txt"))
results["text_io"]["append_accumulates"] = appended == "line1\nline2\n"

# ============================================================================
# Binary I/O
# ============================================================================

test_bytes = [72, 101, 108, 108, 111]  # "Hello"
fs_write_bytes(p("bytes.bin"), test_bytes)
read_back = fs_read_bytes(p("bytes.bin"))
results["binary_io"]["write_and_read"] = read_back == test_bytes
results["binary_io"]["size_matches"] = fs_size(p("bytes.bin")) == 5

# ============================================================================
# Lines I/O
# ============================================================================

lines_in = ["alpha", "beta", "gamma"]
fs_write_lines(p("lines.txt"), lines_in)
lines_out = fs_read_lines(p("lines.txt"))
results["lines_io"]["write_and_read"] = lines_out == lines_in
results["lines_io"]["trailing_newline"] = fs_read_text(p("lines.txt")).endswith("\n")

# Empty lines
fs_write_lines(p("empty_lines.txt"), [])
results["lines_io"]["empty_write"] = fs_read_text(p("empty_lines.txt")) == ""

# ============================================================================
# Format I/O: JSON, YAML, TOML
# ============================================================================

json_data = {"name": "spaces", "version": 1, "enabled": True}
fs_write_json(p("data.json"), json_data)
json_back = fs_read_json(p("data.json"))
results["format_io"]["json_roundtrip_name"] = json_back.get("name") == "spaces"
results["format_io"]["json_roundtrip_version"] = json_back.get("version") == 1
results["format_io"]["json_roundtrip_enabled"] = json_back.get("enabled") == True

yaml_data = {"project": "test", "count": 42}
fs_write_yaml(p("data.yaml"), yaml_data)
yaml_back = fs_read_yaml(p("data.yaml"))
results["format_io"]["yaml_roundtrip_project"] = yaml_back.get("project") == "test"
results["format_io"]["yaml_roundtrip_count"] = yaml_back.get("count") == 42

toml_data = {"package": {"name": "myapp", "version": "0.1.0"}}
fs_write_toml(p("data.toml"), toml_data)
toml_back = fs_read_toml(p("data.toml"))
results["format_io"]["toml_roundtrip_name"] = toml_back.get("package", {}).get("name") == "myapp"
results["format_io"]["toml_roundtrip_version"] = toml_back.get("package", {}).get("version") == "0.1.0"

# ============================================================================
# Existence / type checks
# ============================================================================

results["existence_checks"]["exists_file"] = fs_exists(p("hello.txt"))
results["existence_checks"]["exists_missing"] = not fs_exists(p("no_such_file.txt"))
results["existence_checks"]["is_file_file"] = fs_is_file(p("hello.txt"))
results["existence_checks"]["is_file_missing"] = not fs_is_file(p("no_such_file.txt"))

fs_mkdir(p("subdir"))
results["existence_checks"]["is_directory"] = fs_is_directory(p("subdir"))
results["existence_checks"]["is_file_on_dir"] = not fs_is_file(p("subdir"))

# ============================================================================
# Directory operations
# ============================================================================

# mkdir: basic
fs_mkdir(p("dir1"))
results["directory_ops"]["mkdir_creates_dir"] = fs_is_directory(p("dir1"))

# mkdir: parents
fs_mkdir(p("deep/nested/path"), parents = True)
results["directory_ops"]["mkdir_parents"] = fs_is_directory(p("deep/nested/path"))

# mkdir: exist_ok
fs_mkdir(p("dir1"), exist_ok = True)
results["directory_ops"]["mkdir_exist_ok"] = fs_is_directory(p("dir1"))

# read_directory
fs_write_text(p("dir1/file_a.txt"), "a")
fs_write_text(p("dir1/file_b.txt"), "b")
dir_contents = fs_read_directory(p("dir1"))
results["directory_ops"]["read_directory_count"] = len(dir_contents) == 2

# ============================================================================
# Copy
# ============================================================================

# File copy
fs_write_text(p("src.txt"), "source content")
fs_copy(p("src.txt"), p("dst.txt"))
results["copy_move"]["copy_file_creates_dst"] = fs_exists(p("dst.txt"))
results["copy_move"]["copy_file_content"] = fs_read_text(p("dst.txt")) == "source content"
results["copy_move"]["copy_preserves_src"] = fs_exists(p("src.txt"))

# Copy with overwrite
fs_write_text(p("orig.txt"), "original")
fs_write_text(p("new_version.txt"), "new")
fs_copy(p("new_version.txt"), p("orig.txt"), overwrite = True)
results["copy_move"]["copy_overwrite"] = fs_read_text(p("orig.txt")) == "new"

# Recursive directory copy
fs_mkdir(p("srcdir"))
fs_write_text(p("srcdir/a.txt"), "aaa")
fs_write_text(p("srcdir/b.txt"), "bbb")
fs_copy(p("srcdir"), p("dstdir"), recursive = True)
results["copy_move"]["copy_recursive_dst_exists"] = fs_is_directory(p("dstdir"))
results["copy_move"]["copy_recursive_file_a"] = fs_read_text(p("dstdir/a.txt")) == "aaa"
results["copy_move"]["copy_recursive_file_b"] = fs_read_text(p("dstdir/b.txt")) == "bbb"

# ============================================================================
# Move
# ============================================================================

fs_write_text(p("move_src.txt"), "move me")
fs_move(p("move_src.txt"), p("move_dst.txt"))
results["copy_move"]["move_dst_exists"] = fs_exists(p("move_dst.txt"))
results["copy_move"]["move_src_gone"] = not fs_exists(p("move_src.txt"))
results["copy_move"]["move_content"] = fs_read_text(p("move_dst.txt")) == "move me"

# Move into subdirectory (creates parent)
fs_write_text(p("to_move.txt"), "nested move")
fs_move(p("to_move.txt"), p("new_nested_dir/to_move.txt"))
results["copy_move"]["move_creates_parent"] = fs_exists(p("new_nested_dir/to_move.txt"))

# Move with overwrite
fs_write_text(p("mv_a.txt"), "AAA")
fs_write_text(p("mv_b.txt"), "BBB")
fs_move(p("mv_a.txt"), p("mv_b.txt"), overwrite = True)
results["copy_move"]["move_overwrite"] = fs_read_text(p("mv_b.txt")) == "AAA"

# ============================================================================
# Remove
# ============================================================================

fs_write_text(p("to_delete.txt"), "bye")
fs_remove(p("to_delete.txt"))
results["remove"]["remove_file"] = not fs_exists(p("to_delete.txt"))

# remove missing_ok=True should not error
fs_remove(p("no_such_file.xyz"), missing_ok = True)
results["remove"]["remove_missing_ok"] = True  # would error if not ok

# Remove directory recursively
fs_mkdir(p("rm_dir"))
fs_write_text(p("rm_dir/file.txt"), "x")
fs_remove(p("rm_dir"), recursive = True)
results["remove"]["remove_recursive"] = not fs_exists(p("rm_dir"))

# ============================================================================
# Symlinks
# ============================================================================

fs_write_text(p("link_target.txt"), "target content")
fs_symlink(p("link_target.txt"), p("my_link.txt"))
results["symlinks"]["is_symlink"] = fs_is_symlink(p("my_link.txt"))
results["symlinks"]["read_link"] = fs_read_link(p("my_link.txt")) == p("link_target.txt")

# Reading through the symlink should return the target's content
results["symlinks"]["read_through_link"] = fs_read_text(p("my_link.txt")) == "target content"

# is_file on a symlink-to-file should be true (follows symlink)
results["symlinks"]["is_file_through_symlink"] = fs_is_file(p("my_link.txt"))

# ============================================================================
# Metadata
# ============================================================================

fs_write_text(p("meta.txt"), "metadata test")
meta = fs_metadata(p("meta.txt"))
results["metadata"]["is_file"] = meta.get("is_file") == True
results["metadata"]["is_dir"] = meta.get("is_dir") == False
results["metadata"]["is_symlink"] = meta.get("is_symlink") == False
results["metadata"]["size_positive"] = meta.get("size") > 0
results["metadata"]["modified_set"] = meta.get("modified") != None
results["metadata"]["permissions_string"] = type(meta.get("permissions")) == "string" and len(meta.get("permissions")) == 9

# metadata on a symlink-to-file: is_file should be True (follows symlink)
meta_link = fs_metadata(p("my_link.txt"))
results["metadata"]["symlink_is_file_true"] = meta_link.get("is_file") == True
results["metadata"]["symlink_is_symlink_true"] = meta_link.get("is_symlink") == True

# size and modified
sz = fs_size(p("meta.txt"))
results["metadata"]["size_matches_meta"] = sz == meta.get("size")
mtime = fs_modified(p("meta.txt"))
results["metadata"]["modified_is_float"] = type(mtime) == "float" or type(mtime) == "int"

# ============================================================================
# is_text_file
# ============================================================================

fs_write_text(p("text_check.txt"), "plain UTF-8 text")
results["is_text_file"]["text_file_true"] = fs_is_text_file(p("text_check.txt"))

# Write binary content (NUL byte) — is_text_file should return False
fs_write_bytes(p("binary_check.bin"), [0x89, 0x50, 0x4E, 0x47, 0x00, 0x0D])  # PNG header with NUL
results["is_text_file"]["binary_file_false"] = not fs_is_text_file(p("binary_check.bin"))

# Non-existent path returns False, not error
results["is_text_file"]["missing_path_false"] = not fs_is_text_file(p("no_such_file_xyz.txt"))

# ============================================================================
# Atomic write
# ============================================================================

fs_write_string_atomic(p("atomic.conf"), "key=value\n")
results["atomic_write"]["content_correct"] = fs_read_text(p("atomic.conf")) == "key=value\n"
results["atomic_write"]["file_exists"] = fs_exists(p("atomic.conf"))

# No leftover .tmp file
dir_after = fs_read_directory(work_dir)
tmp_files = [f for f in dir_after if f.endswith(".tmp")]
results["atomic_write"]["no_leftover_tmp"] = len(tmp_files) == 0

# ============================================================================
# Touch
# ============================================================================

# touch creates a new file
fs_touch(p("touched.txt"))
results["touch"]["creates_file"] = fs_exists(p("touched.txt"))

# touch updates mtime (mtime after touch >= mtime before)
mtime_before = fs_modified(p("touched.txt"))
fs_touch(p("touched.txt"), update_mtime = True)
mtime_after = fs_modified(p("touched.txt"))
results["touch"]["mtime_updated"] = mtime_after >= mtime_before

# touch create=False on missing file should not create it
fs_touch(p("no_create.txt"), create = False, update_mtime = False)
results["touch"]["no_create_respects_flag"] = not fs_exists(p("no_create.txt"))

# ============================================================================
# Permissions (chmod / set_permissions)
# ============================================================================

fs_write_text(p("perms.txt"), "permissions test")
fs_set_permissions(p("perms.txt"), 0o600)
meta_p = fs_metadata(p("perms.txt"))
results["permissions"]["set_permissions_mode"] = (meta_p.get("mode") & 0o777) == 0o600

fs_chmod(p("perms.txt"), "u+x")
meta_p2 = fs_metadata(p("perms.txt"))
results["permissions"]["chmod_add_exec"] = (meta_p2.get("mode") & 0o100) != 0

# Multi-perm chmod (u+rx)
fs_set_permissions(p("perms.txt"), 0o600)
fs_chmod(p("perms.txt"), "u+rx")
meta_p3 = fs_metadata(p("perms.txt"))
results["permissions"]["chmod_multi_perm"] = (meta_p3.get("mode") & 0o500) == 0o500

fs_chmod(p("perms.txt"), "u-x")
meta_p4 = fs_metadata(p("perms.txt"))
results["permissions"]["chmod_remove_exec"] = (meta_p4.get("mode") & 0o100) == 0

# ============================================================================
# Cleanup and output
# ============================================================================

tmp_cleanup_all()

print("Filesystem (fs) Module Test Results:")
print("=====================================")
print("")
print(json_dumps(results, is_pretty = True))
print("")

# Check for any False values
all_ok = True
for category in results:
    for test_name in results[category]:
        val = results[category][test_name]
        if val == False:
            print("FAIL: " + category + "." + test_name)
            all_ok = False

if all_ok:
    print("All fs tests passed!")
else:
    print("Some tests FAILED - see above")
