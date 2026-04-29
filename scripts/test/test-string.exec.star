#!/usr/bin/env spaces

load(
    "//@star/sdk/star/std/json.star",
    "json_dumps",
)
load(
    "//@star/sdk/star/std/string.star",
    "string_camel_case",
    "string_contains",
    "string_ends_with",
    "string_format_table",
    "string_kebab_case",
    "string_lower",
    "string_pad_left",
    "string_pad_right",
    "string_regex_captures",
    "string_regex_find_all",
    "string_regex_match",
    "string_replace",
    "string_snake_case",
    "string_split_lines",
    "string_split_whitespace",
    "string_starts_with",
    "string_title_case",
    "string_trim",
    "string_trim_end",
    "string_trim_start",
    "string_upper",
)

# String module test results
string_results = {
    "trimming": {},
    "splitting": {},
    "validation": {},
    "replacement": {},
    "regex": {},
    "case_conversion": {},
    "padding": {},
    "table_formatting": {},
    "unicode": {},
    "edge_cases": {},
}

# ============================================================================
# String Module Tests
# ============================================================================

# Test trimming functions
string_results["trimming"]["trim_both"] = string_trim("  hello  ") == "hello"
string_results["trimming"]["trim_start"] = string_trim_start("  hello  ") == "hello  "
string_results["trimming"]["trim_end"] = string_trim_end("  hello  ") == "  hello"

# Test splitting functions
whitespace_split = string_split_whitespace("one  two\tthree\nfour")
string_results["splitting"]["split_whitespace_count"] = len(whitespace_split) == 4
string_results["splitting"]["split_whitespace_first"] = whitespace_split[0] == "one"
string_results["splitting"]["split_whitespace_second"] = whitespace_split[1] == "two"

lines_split = string_split_lines("line1\nline2\r\nline3")
string_results["splitting"]["split_lines_count"] = len(lines_split) == 3
string_results["splitting"]["split_lines_first"] = lines_split[0] == "line1"

# Test validation functions
string_results["validation"]["contains_found"] = string_contains("hello world", "world")
string_results["validation"]["contains_not_found"] = not string_contains("hello world", "xyz")
string_results["validation"]["contains_case_insensitive"] = string_contains("Hello", "hello", ignore_case = True)
string_results["validation"]["contains_case_sensitive_fail"] = not string_contains("Hello", "hello", ignore_case = False)

string_results["validation"]["starts_with_true"] = string_starts_with("hello-world", "hello")
string_results["validation"]["starts_with_false"] = not string_starts_with("hello-world", "world")

string_results["validation"]["ends_with_true"] = string_ends_with("file.txt", ".txt")
string_results["validation"]["ends_with_false"] = not string_ends_with("file.txt", ".pdf")

# Test replacement functions
string_results["replacement"]["simple_replace"] = string_replace("hello world", "world", "universe") == "hello universe"
string_results["replacement"]["replace_count"] = string_replace("aaa", "a", "b", count = 2) == "bba"
string_results["replacement"]["replace_case_insensitive"] = string_replace("Hello HELLO hello", "hello", "hi", ignore_case = True) == "hi hi hi"
string_results["replacement"]["replace_regex"] = string_replace("abc123def456", r"\d+", "X", regex = True) == "abcXdefX"

# Test regex functions
match = string_regex_match(r"\d+", "version 42")
string_results["regex"]["regex_match_found"] = match != None
if match:
    string_results["regex"]["regex_match_value"] = match.get("match") == "42"
    string_results["regex"]["regex_match_has_start"] = "start" in match
    string_results["regex"]["regex_match_has_end"] = "end" in match

matches = string_regex_find_all(r"\d+", "I have 2 apples and 3 oranges")
string_results["regex"]["regex_find_all_count"] = len(matches) == 2
if len(matches) >= 2:
    string_results["regex"]["regex_find_all_first"] = matches[0].get("match") == "2"
    string_results["regex"]["regex_find_all_second"] = matches[1].get("match") == "3"

captures = string_regex_captures(r"(?P<year>\d{4})-(?P<month>\d{2})", "2024-03-15")
string_results["regex"]["regex_captures_found"] = captures != None
if captures:
    string_results["regex"]["regex_captures_year"] = captures.get("year") == "2024"
    string_results["regex"]["regex_captures_month"] = captures.get("month") == "03"

# Test case conversion functions
string_results["case_conversion"]["upper"] = string_upper("hello") == "HELLO"
string_results["case_conversion"]["lower"] = string_lower("HELLO") == "hello"
string_results["case_conversion"]["title_case"] = string_title_case("hello world") == "Hello World"
string_results["case_conversion"]["snake_case"] = string_snake_case("helloWorld") == "hello_world"
string_results["case_conversion"]["kebab_case"] = string_kebab_case("helloWorld") == "hello-world"
string_results["case_conversion"]["camel_case"] = string_camel_case("hello_world") == "helloWorld"

# Test various case conversion patterns
string_results["case_conversion"]["title_case_multiple"] = string_title_case("hello-world_foo bar") == "Hello World Foo Bar"
string_results["case_conversion"]["snake_case_spaces"] = string_snake_case("hello world") == "hello_world"
string_results["case_conversion"]["kebab_case_snake"] = string_kebab_case("hello_world") == "hello-world"
string_results["case_conversion"]["camel_case_kebab"] = string_camel_case("hello-world-bar") == "helloWorldBar"

# Test padding functions
string_results["padding"]["pad_left_default"] = string_pad_left("42", 5) == "   42"
string_results["padding"]["pad_left_custom"] = string_pad_left("42", 5, "0") == "00042"
string_results["padding"]["pad_left_no_padding"] = string_pad_left("hello", 3) == "hello"

string_results["padding"]["pad_right_default"] = string_pad_right("42", 5) == "42   "
string_results["padding"]["pad_right_custom"] = string_pad_right("hi", 5, ".") == "hi..."
string_results["padding"]["pad_right_no_padding"] = string_pad_right("hello", 3) == "hello"

# Test table formatting
table_data = [
    {"name": "Alice", "age": "30"},
    {"name": "Bob", "age": "25"},
]
table_output = string_format_table(table_data)
string_results["table_formatting"]["table_generated"] = table_output != None and len(table_output) > 0
string_results["table_formatting"]["table_contains_separator"] = "+" in table_output
string_results["table_formatting"]["table_contains_pipe"] = "|" in table_output
string_results["table_formatting"]["table_contains_name"] = "name" in table_output and "Alice" in table_output

# ============================================================================
# Edge-case tests
# ============================================================================

# split_lines: trailing newline must NOT produce a trailing empty string
trailing_nl = string_split_lines("a\nb\n")
string_results["edge_cases"]["split_lines_trailing_newline_count"] = len(trailing_nl) == 2
string_results["edge_cases"]["split_lines_trailing_newline_content"] = trailing_nl[0] == "a" and trailing_nl[1] == "b"

# split_lines: empty string must return empty list
string_results["edge_cases"]["split_lines_empty"] = len(string_split_lines("")) == 0

# split_lines: single newline returns one empty line
single_nl = string_split_lines("\n")
string_results["edge_cases"]["split_lines_single_newline_count"] = len(single_nl) == 1
string_results["edge_cases"]["split_lines_single_newline_content"] = single_nl[0] == ""

# split_lines: no newline returns one-element list
string_results["edge_cases"]["split_lines_no_newline"] = string_split_lines("hello") == ["hello"]

# replace: count=0 is a no-op
string_results["edge_cases"]["replace_count_zero"] = string_replace("aaa", "a", "b", count = 0) == "aaa"

# replace: empty haystack
string_results["edge_cases"]["replace_empty_haystack"] = string_replace("", "a", "b") == ""

# pad: width smaller than string length is a no-op
string_results["edge_cases"]["pad_left_noop"] = string_pad_left("hello", 2) == "hello"
string_results["edge_cases"]["pad_right_noop"] = string_pad_right("hello", 2) == "hello"

# pad: zero width is a no-op
string_results["edge_cases"]["pad_left_zero_width"] = string_pad_left("x", 0) == "x"
string_results["edge_cases"]["pad_right_zero_width"] = string_pad_right("x", 0) == "x"

# regex_match: no match returns None
string_results["edge_cases"]["regex_match_no_match"] = string_regex_match(r"\d+", "no digits here") == None

# regex_find_all: no matches returns empty list
string_results["edge_cases"]["regex_find_all_empty"] = len(string_regex_find_all(r"\d+", "no digits")) == 0

# regex_captures: no match returns None
string_results["edge_cases"]["regex_captures_no_match"] = string_regex_captures(r"(?P<d>\d+)", "no digits") == None

# ============================================================================
# Unicode tests
# ============================================================================

# Trim with accented characters
string_results["unicode"]["trim_accented"] = string_trim("  héllo  ") == "héllo"
string_results["unicode"]["trim_start_accented"] = string_trim_start("  café") == "café"
string_results["unicode"]["trim_end_accented"] = string_trim_end("thé  ") == "thé"

# Case conversion with accented letters
string_results["unicode"]["upper_accented"] = string_upper("héllo") == "HÉLLO"
string_results["unicode"]["lower_accented"] = string_lower("HÉLLO") == "héllo"
string_results["unicode"]["title_case_accented"] = string_title_case("héllo wörld") == "Héllo Wörld"
string_results["unicode"]["snake_case_accented"] = string_snake_case("héllo wörld") == "héllo_wörld"
string_results["unicode"]["kebab_case_accented"] = string_kebab_case("héllo wörld") == "héllo-wörld"
string_results["unicode"]["camel_case_accented"] = string_camel_case("héllo wörld") == "hélloWörld"

# split_whitespace with accented words
unicode_words = string_split_whitespace("café thé")
string_results["unicode"]["split_whitespace_accented"] = len(unicode_words) == 2 and unicode_words[0] == "café" and unicode_words[1] == "thé"

# split_lines with non-ASCII content
unicode_lines = string_split_lines("café\nthé\nbière")
string_results["unicode"]["split_lines_accented_count"] = len(unicode_lines) == 3
string_results["unicode"]["split_lines_accented_content"] = unicode_lines[0] == "café" and unicode_lines[1] == "thé"

# contains with accented characters
string_results["unicode"]["contains_accented"] = string_contains("héllo wörld", "wörld")
string_results["unicode"]["contains_accented_ci"] = string_contains("HÉLLO", "héllo", ignore_case = True)

# starts_with / ends_with with multi-byte chars
string_results["unicode"]["starts_with_unicode"] = string_starts_with("héllo", "hé")
string_results["unicode"]["ends_with_unicode"] = string_ends_with("café", "fé")

# replace with accented characters (literal replacement)
string_results["unicode"]["replace_accented"] = string_replace("café café", "café", "tea") == "tea tea"
string_results["unicode"]["replace_ci_accented"] = string_replace("Café CAFÉ café", "café", "tea", ignore_case = True) == "tea tea tea"

# Padding uses char count, not byte count (é is 2 bytes, 1 char)
padded_left = string_pad_left("é", 4)
string_results["unicode"]["pad_left_multibyte"] = len(padded_left) == 4 and padded_left == "   é"
padded_right = string_pad_right("é", 4)
string_results["unicode"]["pad_right_multibyte"] = len(padded_right) == 4 and padded_right == "é   "

# regex match: start/end must be char offsets, not byte offsets
# "café 42": é is 2 bytes so byte offset of "42" is 6, but char offset is 5
unicode_match = string_regex_match(r"\d+", "café 42")
string_results["unicode"]["regex_match_unicode_found"] = unicode_match != None
if unicode_match:
    string_results["unicode"]["regex_match_unicode_value"] = unicode_match.get("match") == "42"

    # char offset: c(0) a(1) f(2) é(3) space(4) → "42" starts at char 5
    string_results["unicode"]["regex_match_char_start"] = unicode_match.get("start") == 5
    string_results["unicode"]["regex_match_char_end"] = unicode_match.get("end") == 7

# regex find_all: char offsets for all matches in a non-ASCII string
unicode_all = string_regex_find_all(r"\d+", "à1b2")
string_results["unicode"]["regex_find_all_unicode_count"] = len(unicode_all) == 2
if len(unicode_all) == 2:
    # 'à' is 1 char (2 bytes); "1" starts at char offset 1
    string_results["unicode"]["regex_find_all_first_char_start"] = unicode_all[0].get("start") == 1

    # "2" starts at char offset 3
    string_results["unicode"]["regex_find_all_second_char_start"] = unicode_all[1].get("start") == 3

# ============================================================================
# Output Results
# ============================================================================

print("String Module Test Results:")
print("===========================")
print("")
print(json_dumps(string_results, is_pretty = True))
print("")
print("All string functions executed successfully!")
