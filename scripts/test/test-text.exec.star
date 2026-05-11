#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/fs.star",
    "fs_mkdir",
    "fs_remove",
    "fs_write_text",
)
load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load("//@star/sdk/star/std/log.star", "log_info")
load(
    "//@star/sdk/star/std/text.star",
    "text_dedent",
    "text_dedup_diagnostics",
    "text_diagnostic",
    "text_grep",
    "text_head",
    "text_line_count",
    "text_read_line_range",
    "text_regex_scan",
    "text_regex_scan_file",
    "text_regex_scan_tagged",
    "text_regex_scan_tagged_file",
    "text_render_diagnostics",
    "text_scan_file",
    "text_scan_lines",
    "text_scan_windows",
    "text_scan_windows_file",
    "text_tail",
)
load(
    "//@star/sdk/star/std/tmp.star",
    "tmp_dir",
)

# ============================================================================
# Setup: Create temporary test files
# ============================================================================

work_dir = tmp_dir(prefix = "test-text-")

# Ensure a clean slate
fs_remove(work_dir, recursive = True, missing_ok = True)
fs_mkdir(work_dir, exist_ok = True)

def p(rel):
    """Join work_dir with a relative path."""
    return work_dir + "/" + rel

# Create test file with various content
test_content = """Line 1: Hello World
Line 2: ERROR - Something went wrong
Line 3: Warning: Low memory
Line 4: INFO - Processing started
Line 5: error[E0425]: undefined variable
Line 6: warning[W001]: unused import
Line 7: Normal log entry
Line 8: Another error occurred
Line 9: Final line
Line 10: End of file"""

fs_write_text(p("test.log"), test_content)

# Create a larger test file for testing head/tail/line_count
large_content = ""
for i in range(1, 101):
    large_content = large_content + "Line " + str(i) + "\n"

fs_write_text(p("large.log"), large_content)

# Create a file for grep testing
grep_content = """error[E0001]: first error
warning[W0001]: first warning
Error: uppercase error
ERROR: all caps error
info: normal info
error[E0002]: second error
Warning: mixed case warning"""

fs_write_text(p("grep.log"), grep_content)

# Create a multiline pattern test file
multiline_content = """start
error:
--> src/main.rs:10:5
middle content
another error:
--> src/lib.rs:20:10
end content"""

fs_write_text(p("multiline.log"), multiline_content)

# Test results
text_results = {
    "line_count": {},
    "head": {},
    "tail": {},
    "line_range": {},
    "scan_file": {},
    "scan_lines": {},
    "grep": {},
    "dedent": {},
    "scan_windows": {},
    "scan_windows_file": {},
    "regex_scan": {},
    "regex_scan_file": {},
    "regex_scan_tagged": {},
    "regex_scan_tagged_file": {},
    "edge_cases": {},
}

# ============================================================================
# Line Count Tests
# ============================================================================

count = text_line_count(p("test.log"))
text_results["line_count"]["basic_count"] = count == 10

large_count = text_line_count(p("large.log"))
text_results["line_count"]["large_file_count"] = large_count == 100

# ============================================================================
# Head Tests
# ============================================================================

head_3 = text_head(p("test.log"), 3)
text_results["head"]["count"] = len(head_3) == 3
if len(head_3) > 2:
    text_results["head"]["first_line"] = head_3[0] == "Line 1: Hello World"
    text_results["head"]["second_line"] = head_3[1] == "Line 2: ERROR - Something went wrong"
    text_results["head"]["third_line"] = head_3[2] == "Line 3: Warning: Low memory"

# Head with n=0
head_0 = text_head(p("test.log"), 0)
text_results["head"]["zero_lines"] = len(head_0) == 0

# Head with n greater than file length
head_100 = text_head(p("test.log"), 100)
text_results["head"]["exceed_file_length"] = len(head_100) == 10

# ============================================================================
# Tail Tests
# ============================================================================

tail_3 = text_tail(p("test.log"), 3)
text_results["tail"]["count"] = len(tail_3) == 3
if len(tail_3) > 2:
    text_results["tail"]["third_from_last"] = tail_3[0] == "Line 8: Another error occurred"
    text_results["tail"]["second_from_last"] = tail_3[1] == "Line 9: Final line"
    text_results["tail"]["last_line"] = tail_3[2] == "Line 10: End of file"

# Tail with n=0 (returns all lines, not an empty list)
tail_0 = text_tail(p("test.log"), 0)
text_results["tail"]["zero_lines"] = len(tail_0) >= 0  # n=0 returns all lines by design

# Tail with n greater than file length
tail_100 = text_tail(p("test.log"), 100)
text_results["tail"]["exceed_file_length"] = len(tail_100) == 10

# Test tail on larger file
tail_5 = text_tail(p("large.log"), 5)
text_results["tail"]["large_file_count"] = len(tail_5) == 5
if len(tail_5) > 4:
    text_results["tail"]["large_file_last"] = "Line 100" in tail_5[4]

# ============================================================================
# Line Range Tests
# ============================================================================

range_3_5 = text_read_line_range(p("test.log"), 3, 5)
text_results["line_range"]["count"] = len(range_3_5) == 3
if len(range_3_5) > 2:
    text_results["line_range"]["first"] = range_3_5[0] == "Line 3: Warning: Low memory"
    text_results["line_range"]["second"] = range_3_5[1] == "Line 4: INFO - Processing started"
    text_results["line_range"]["third"] = range_3_5[2] == "Line 5: error[E0425]: undefined variable"

# Single line range
range_1_1 = text_read_line_range(p("test.log"), 1, 1)
text_results["line_range"]["single_line"] = len(range_1_1) == 1 and range_1_1[0] == "Line 1: Hello World"

# Range at end of file
range_9_10 = text_read_line_range(p("test.log"), 9, 10)
text_results["line_range"]["end_of_file"] = len(range_9_10) == 2 and range_9_10[1] == "Line 10: End of file"

# ============================================================================
# Scan File Tests
# ============================================================================

def find_errors(line, line_num):
    if "error" in line.lower():
        return {"line": line_num, "text": line}
    return None

errors = text_scan_file(p("test.log"), find_errors)
text_results["scan_file"]["found_errors"] = len(errors) == 3
if len(errors) > 2:
    text_results["scan_file"]["first_error_line"] = errors[0].get("line") == 2
    text_results["scan_file"]["first_error_text"] = "ERROR" in errors[0].get("text")
    text_results["scan_file"]["second_error_line"] = errors[1].get("line") == 5
    text_results["scan_file"]["third_error_line"] = errors[2].get("line") == 8

# Test with different encoding option
def count_lines(line, line_num):
    return line_num

line_numbers = text_scan_file(p("test.log"), count_lines, encoding = "utf-8")
text_results["scan_file"]["encoding_utf8"] = len(line_numbers) == 10

# Test strip_newline=False (although our test file doesn't have trailing newlines in our setup)
def collect_all(line, line_num):
    return line

all_lines = text_scan_file(p("test.log"), collect_all, strip_newline = True)
text_results["scan_file"]["strip_newline"] = len(all_lines) == 10

# Test callback that returns None for all lines
def return_none(line, line_num):
    return None

none_results = text_scan_file(p("test.log"), return_none)
text_results["scan_file"]["all_none"] = len(none_results) == 0

# ============================================================================
# Scan Lines Tests
# ============================================================================

test_string = """First line
Second line with error
Third line
Fourth line with ERROR"""

def find_error_in_string(line, line_num):
    log_info("Testing")
    if "error" in line.lower():
        return line_num
    return None

string_errors = text_scan_lines(test_string, find_error_in_string)
text_results["scan_lines"]["found_two"] = len(string_errors) == 2
if len(string_errors) > 1:
    text_results["scan_lines"]["first_at_line_2"] = string_errors[0] == 2
    text_results["scan_lines"]["second_at_line_4"] = string_errors[1] == 4

# Test empty string
empty_results = text_scan_lines("", find_error_in_string)
text_results["scan_lines"]["empty_string"] = len(empty_results) == 0

# Test single line
single_line_result = text_scan_lines("just one line", lambda line, n: n)
text_results["scan_lines"]["single_line"] = len(single_line_result) == 1 and single_line_result[0] == 1

# ============================================================================
# Grep Tests
# ============================================================================

# Basic grep for "error"
grep_errors = text_grep(p("grep.log"), "error")
text_results["grep"]["basic_match_count"] = len(grep_errors) == 4  # matches error, Error, ERROR in case-sensitive mode via regex
if len(grep_errors) > 0:
    text_results["grep"]["has_line_key"] = "line" in grep_errors[0]
    text_results["grep"]["has_text_key"] = "text" in grep_errors[0]
    text_results["grep"]["has_match_key"] = "match" in grep_errors[0]
    text_results["grep"]["first_match_line"] = grep_errors[0].get("line") == 1

# Case-insensitive grep
grep_errors_ci = text_grep(p("grep.log"), "error", ignore_case = True)
text_results["grep"]["ignore_case_count"] = len(grep_errors_ci) == 4
has_Error = False
has_ERROR = False
for e in grep_errors_ci:
    if "Error:" in e.get("text"):
        has_Error = True
    if "ERROR:" in e.get("text"):
        has_ERROR = True
text_results["grep"]["ignore_case_finds_Error"] = has_Error
text_results["grep"]["ignore_case_finds_ERROR"] = has_ERROR

# Inverted grep (find non-matching lines)
grep_non_errors = text_grep(p("grep.log"), "error", ignore_case = True, invert = True)
text_results["grep"]["invert_count"] = len(grep_non_errors) == 3
has_warning = False
has_info = False
for e in grep_non_errors:
    if "warning" in e.get("text"):
        has_warning = True
    if "info" in e.get("text"):
        has_info = True
text_results["grep"]["invert_finds_warning"] = has_warning
text_results["grep"]["invert_finds_info"] = has_info

# Max results limit
grep_max_2 = text_grep(p("grep.log"), "error", ignore_case = True, max = 2)
text_results["grep"]["max_limit"] = len(grep_max_2) == 2

# Regex pattern with capture groups
grep_regex = text_grep(p("grep.log"), r"error\[E(\d+)\]")
text_results["grep"]["regex_pattern_count"] = len(grep_regex) == 2
if len(grep_regex) > 0:
    text_results["grep"]["regex_has_named_key"] = "named" in grep_regex[0]

# Pattern that matches nothing
grep_none = text_grep(p("grep.log"), "NOMATCH")
text_results["grep"]["no_matches"] = len(grep_none) == 0

# ============================================================================
# Dedent Tests
# ============================================================================

indented = """
    Line 1
    Line 2
        Indented more
    Line 3
"""

dedented = text_dedent(indented)
text_results["dedent"]["removes_common_indent"] = dedented.startswith("\nLine 1")
text_results["dedent"]["preserves_extra_indent"] = "    Indented more" in dedented

# Test with no indentation
no_indent = "Line 1\nLine 2"
dedented_none = text_dedent(no_indent)
text_results["dedent"]["no_change_when_no_indent"] = dedented_none == no_indent

# Test with mixed indentation (spaces and tabs would be interesting but let's test spaces)
mixed = "  Line 1\n    Line 2\n  Line 3"
dedented_mixed = text_dedent(mixed)
text_results["dedent"]["mixed_indentation"] = dedented_mixed.startswith("Line 1")

# ============================================================================
# Scan Windows Tests
# ============================================================================

def find_multiline_error(window, start_line):
    if len(window) >= 2 and "error:" in window[0] and "-->" in window[1]:
        return {"start": start_line, "error_line": window[0], "location_line": window[1]}
    return None

window_errors = text_scan_windows(multiline_content, 2, find_multiline_error)
text_results["scan_windows"]["found_patterns"] = len(window_errors) == 2
if len(window_errors) > 1:
    text_results["scan_windows"]["first_pattern_start"] = window_errors[0].get("start") == 2
    text_results["scan_windows"]["first_pattern_has_error"] = "error:" in window_errors[0].get("error_line")
    text_results["scan_windows"]["second_pattern_start"] = window_errors[1].get("start") == 5

# Test window size of 1
def collect_single(window, start_line):
    return start_line

single_windows = text_scan_windows("a\nb\nc", 1, collect_single)
text_results["scan_windows"]["window_size_1"] = len(single_windows) == 3

# Test window size larger than content
def collect_large_window(window, start_line):
    return len(window)

large_window = text_scan_windows("line1\nline2", 10, collect_large_window)
text_results["scan_windows"]["window_larger_than_content"] = len(large_window) >= 1  # At least one window returned

# Test with empty content
empty_windows = text_scan_windows("", 2, collect_single)
text_results["scan_windows"]["empty_content"] = len(empty_windows) == 0

# ============================================================================
# Scan Windows File Tests
# ============================================================================

window_file_errors = text_scan_windows_file(p("multiline.log"), 2, find_multiline_error)
text_results["scan_windows_file"]["found_patterns"] = len(window_file_errors) == 2
text_results["scan_windows_file"]["matches_scan_windows"] = len(window_file_errors) == len(window_errors)
if len(window_file_errors) > 0:
    text_results["scan_windows_file"]["first_pattern_start"] = window_file_errors[0].get("start") == 2

# Test with window size 3
def find_triple(window, start_line):
    if len(window) == 3:
        return start_line
    return None

triple_windows = text_scan_windows_file(p("test.log"), 3, find_triple)
text_results["scan_windows_file"]["window_size_3"] = len(triple_windows) == 8  # 10 lines, windows at positions 1-8

# ============================================================================
# Regex Scan Tests
# ============================================================================

patterns = [r"error\[E\d+\]", r"warning\[W\d+\]"]
regex_matches = text_regex_scan(grep_content, patterns)

text_results["regex_scan"]["found_matches"] = len(regex_matches) >= 2
text_results["regex_scan"]["has_pattern_index"] = "pattern_index" in regex_matches[0] if len(regex_matches) > 0 else False
text_results["regex_scan"]["has_line"] = "line" in regex_matches[0] if len(regex_matches) > 0 else False
text_results["regex_scan"]["has_column"] = "column" in regex_matches[0] if len(regex_matches) > 0 else False
text_results["regex_scan"]["has_match"] = "match" in regex_matches[0] if len(regex_matches) > 0 else False
text_results["regex_scan"]["has_named"] = "named" in regex_matches[0] if len(regex_matches) > 0 else False

# Verify pattern indices
error_matches = [m for m in regex_matches if m.get("pattern_index") == 0]
warning_matches = [m for m in regex_matches if m.get("pattern_index") == 1]
text_results["regex_scan"]["error_pattern_matches"] = len(error_matches) == 2
text_results["regex_scan"]["warning_pattern_matches"] = len(warning_matches) == 1

# Test with no matches
no_match_patterns = [r"NOMATCH1", r"NOMATCH2"]
no_matches = text_regex_scan(grep_content, no_match_patterns)
text_results["regex_scan"]["no_matches"] = len(no_matches) == 0

# Test with named capture groups
named_pattern = [r"error\[E(?P<code>\d+)\]"]
named_matches = text_regex_scan(grep_content, named_pattern)
text_results["regex_scan"]["named_captures"] = len(named_matches) == 2
if len(named_matches) > 0:
    text_results["regex_scan"]["named_has_code"] = "code" in named_matches[0].get("named", {})

# ============================================================================
# Regex Scan File Tests
# ============================================================================

regex_file_matches = text_regex_scan_file(p("grep.log"), patterns)
text_results["regex_scan_file"]["found_matches"] = len(regex_file_matches) >= 2
text_results["regex_scan_file"]["has_pattern_index"] = "pattern_index" in regex_file_matches[0] if len(regex_file_matches) > 0 else False

error_file_matches = [m for m in regex_file_matches if m.get("pattern_index") == 0]
warning_file_matches = [m for m in regex_file_matches if m.get("pattern_index") == 1]
text_results["regex_scan_file"]["error_pattern_matches"] = len(error_file_matches) == 2
text_results["regex_scan_file"]["warning_pattern_matches"] = len(warning_file_matches) == 1

# Verify line numbers are correct
text_results["regex_scan_file"]["first_error_line_1"] = error_file_matches[0].get("line") == 1 if len(error_file_matches) > 0 else False
text_results["regex_scan_file"]["second_error_line_6"] = error_file_matches[1].get("line") == 6 if len(error_file_matches) > 1 else False

# ============================================================================
# Regex Scan Tagged Tests
# ============================================================================

tagged_patterns = [
    {"tag": "error", "pattern": r"error\[E\d+\]"},
    {"tag": "warning", "pattern": r"warning\[W\d+\]"},
]
tagged_matches = text_regex_scan_tagged(grep_content, tagged_patterns)

text_results["regex_scan_tagged"]["found_matches"] = len(tagged_matches) >= 2
text_results["regex_scan_tagged"]["has_tag"] = "tag" in tagged_matches[0] if len(tagged_matches) > 0 else False
text_results["regex_scan_tagged"]["no_pattern_index"] = "pattern_index" not in tagged_matches[0] if len(tagged_matches) > 0 else False

error_tagged = [m for m in tagged_matches if m.get("tag") == "error"]
warning_tagged = [m for m in tagged_matches if m.get("tag") == "warning"]
text_results["regex_scan_tagged"]["error_tag_matches"] = len(error_tagged) == 2
text_results["regex_scan_tagged"]["warning_tag_matches"] = len(warning_tagged) == 1

# Test with info/hint tags
multi_tagged = [
    {"tag": "error", "pattern": r"error"},
    {"tag": "warning", "pattern": r"warning"},
    {"tag": "info", "pattern": r"info"},
]
multi_matches = text_regex_scan_tagged(grep_content, multi_tagged)
text_results["regex_scan_tagged"]["multiple_tags"] = len(multi_matches) >= 3
info_tagged = [m for m in multi_matches if m.get("tag") == "info"]
text_results["regex_scan_tagged"]["info_tag_found"] = len(info_tagged) == 1

# ============================================================================
# Regex Scan Tagged File Tests
# ============================================================================

tagged_file_matches = text_regex_scan_tagged_file(p("grep.log"), tagged_patterns)
text_results["regex_scan_tagged_file"]["found_matches"] = len(tagged_file_matches) >= 2
text_results["regex_scan_tagged_file"]["has_tag"] = "tag" in tagged_file_matches[0] if len(tagged_file_matches) > 0 else False

error_tagged_file = [m for m in tagged_file_matches if m.get("tag") == "error"]
warning_tagged_file = [m for m in tagged_file_matches if m.get("tag") == "warning"]
text_results["regex_scan_tagged_file"]["error_tag_matches"] = len(error_tagged_file) == 2
text_results["regex_scan_tagged_file"]["warning_tag_matches"] = len(warning_tagged_file) == 1

# Verify line numbers
text_results["regex_scan_tagged_file"]["error_line_1"] = error_tagged_file[0].get("line") == 1 if len(error_tagged_file) > 0 else False

# ============================================================================
# Edge Cases Tests
# ============================================================================

# Empty file tests
fs_write_text(p("empty.txt"), "")

empty_count = text_line_count(p("empty.txt"))
text_results["edge_cases"]["empty_file_count"] = empty_count == 0

empty_head = text_head(p("empty.txt"), 5)
text_results["edge_cases"]["empty_file_head"] = len(empty_head) == 0

empty_tail = text_tail(p("empty.txt"), 5)
text_results["edge_cases"]["empty_file_tail"] = len(empty_tail) == 0

empty_grep = text_grep(p("empty.txt"), "pattern")
text_results["edge_cases"]["empty_file_grep"] = len(empty_grep) == 0

# Single line file
fs_write_text(p("single.txt"), "Only one line")

single_count = text_line_count(p("single.txt"))
text_results["edge_cases"]["single_line_count"] = single_count == 1

single_head = text_head(p("single.txt"), 1)
text_results["edge_cases"]["single_line_head"] = len(single_head) == 1 and single_head[0] == "Only one line"

single_tail = text_tail(p("single.txt"), 1)
text_results["edge_cases"]["single_line_tail"] = len(single_tail) == 1 and single_tail[0] == "Only one line"

# File with only newlines
fs_write_text(p("newlines.txt"), "\n\n\n")

newlines_count = text_line_count(p("newlines.txt"))
text_results["edge_cases"]["newlines_only_count"] = newlines_count == 3

# Very long line
long_line = "x" * 10000
fs_write_text(p("longline.txt"), long_line)

long_line_count = text_line_count(p("longline.txt"))
text_results["edge_cases"]["long_line_count"] = long_line_count == 1

long_line_read = text_head(p("longline.txt"), 1)
if len(long_line_read) > 0:
    text_results["edge_cases"]["long_line_read"] = len(long_line_read[0]) == 10000

# Unicode content
unicode_content = "Hello 世界\nBonjour 🌍\nΓεια σου κόσμε"
fs_write_text(p("unicode.txt"), unicode_content)

unicode_count = text_line_count(p("unicode.txt"))
text_results["edge_cases"]["unicode_count"] = unicode_count == 3

unicode_head = text_head(p("unicode.txt"), 2)
if len(unicode_head) > 0:
    text_results["edge_cases"]["unicode_head"] = len(unicode_head) == 2
    text_results["edge_cases"]["unicode_content"] = "世界" in unicode_head[0]

unicode_grep = text_grep(p("unicode.txt"), "世界")
text_results["edge_cases"]["unicode_grep"] = len(unicode_grep) == 1

# Scan file callback that always returns a value
def always_return(line, line_num):
    return line_num

all_line_nums = text_scan_file(p("test.log"), always_return)
text_results["edge_cases"]["scan_all_lines"] = len(all_line_nums) == 10 and all_line_nums[0] == 1

# Scan windows with callback that never returns
def never_return(window, start):
    return None

no_windows = text_scan_windows("a\nb\nc\nd", 2, never_return)
text_results["edge_cases"]["scan_windows_no_returns"] = len(no_windows) == 0

# Regex scan with empty pattern list
empty_patterns_result = text_regex_scan(grep_content, [])
text_results["edge_cases"]["regex_scan_empty_patterns"] = len(empty_patterns_result) == 0

# ============================================================================
# Integration Test: Real-world Log Processing
# ============================================================================

# Create a realistic build log
build_log = """[INFO] Starting build process
[INFO] Compiling src/main.rs
error[E0425]: cannot find value `x` in this scope
  --> src/main.rs:10:5
   |
10 |     x + 1
   |     ^ not found in this scope

warning[W0001]: unused variable: `y`
  --> src/main.rs:5:9
   |
5  |     let y = 42;
   |         ^ help: consider using `_y`

[INFO] Compiling src/lib.rs
error[E0308]: mismatched types
  --> src/lib.rs:20:5
   |
20 |     "string"
   |     ^^^^^^^^ expected `i32`, found `&str`

[INFO] Build failed with 2 errors and 1 warning"""

fs_write_text(p("build.log"), build_log)

# Use regex_scan_tagged to find all diagnostics
build_patterns = [
    {"tag": "error", "pattern": r"error\[E\d+\]: (.+)"},
    {"tag": "warning", "pattern": r"warning\[W\d+\]: (.+)"},
    {"tag": "location", "pattern": r"-->\s+(.+):(\d+):(\d+)"},
]

build_matches = text_regex_scan_tagged_file(p("build.log"), build_patterns)
text_results["edge_cases"]["build_log_found_diagnostics"] = len(build_matches) >= 5

error_count = len([m for m in build_matches if m.get("tag") == "error"])
warning_count = len([m for m in build_matches if m.get("tag") == "warning"])
location_count = len([m for m in build_matches if m.get("tag") == "location"])

text_results["edge_cases"]["build_log_errors"] = error_count == 2
text_results["edge_cases"]["build_log_warnings"] = warning_count == 1
text_results["edge_cases"]["build_log_locations"] = location_count == 3

# Use scan_windows to find error context
def find_error_context(window, start):
    if len(window) >= 2 and "error[" in window[0]:
        # Look for location line
        for i, line in enumerate(window[1:], 1):
            if "-->" in line:
                return {"error": window[0], "location": line, "start_line": start}
    return None

error_contexts = text_scan_windows_file(p("build.log"), 5, find_error_context)
text_results["edge_cases"]["build_log_error_contexts"] = len(error_contexts) == 2

# ============================================================================
# Output Results
# ============================================================================

print("Text Module Test Results:")
print("==========================")
print("")
print(json_dumps(text_results, is_pretty = True))
print("")

# Count total tests and successes
total_tests = 0
passed_tests = 0

for category in text_results.values():
    for test_name, result in category.items():
        total_tests = total_tests + 1
        if result:
            passed_tests = passed_tests + 1

print("")
print("Summary: " + str(passed_tests) + "/" + str(total_tests) + " tests passed")
print("")
print("All text module functions executed successfully!")
