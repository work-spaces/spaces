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
    string_results["regex"]["regex_match_value"] = match["match"] == "42"
    string_results["regex"]["regex_match_has_start"] = "start" in match
    string_results["regex"]["regex_match_has_end"] = "end" in match

matches = string_regex_find_all(r"\d+", "I have 2 apples and 3 oranges")
string_results["regex"]["regex_find_all_count"] = len(matches) == 2
if len(matches) >= 2:
    string_results["regex"]["regex_find_all_first"] = matches[0]["match"] == "2"
    string_results["regex"]["regex_find_all_second"] = matches[1]["match"] == "3"

captures = string_regex_captures(r"(?P<year>\d{4})-(?P<month>\d{2})", "2024-03-15")
string_results["regex"]["regex_captures_found"] = captures != None
if captures:
    string_results["regex"]["regex_captures_year"] = captures["year"] == "2024"
    string_results["regex"]["regex_captures_month"] = captures["month"] == "03"

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
# Output Results
# ============================================================================

print("String Module Test Results:")
print("===========================")
print("")
print(json_dumps(string_results, is_pretty = True))
print("")
print("All string functions executed successfully!")
