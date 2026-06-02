"""
Spaces String Module

This module provides ergonomic wrappers around string manipulation operations.
It supports trimming, splitting, case conversion, pattern matching, validation,
padding, and table formatting.

The string module makes it easy to process text data with common patterns like
case transformations (camelCase, snake_case, kebab-case), regex matching,
whitespace handling, and structured text output.

Examples:
    # Case conversion
    name = string_camel_case("hello-world")  # "helloWorld"

    # Trimming whitespace
    text = string_trim("  hello  ")  # "hello"

    # Pattern matching with regex
    match = string_regex_match(r"\\d+", "version 42")
    if match:
        print(f"Found: {match['match']}")

    # Split and process
    words = string_split_whitespace("one two three")  # ["one", "two", "three"]

    # Format as table
    rows = [
        {"name": "Alice", "age": "30"},
        {"name": "Bob", "age": "25"},
    ]
    table = string_format_table(rows)
    print(table)
"""

# ============================================================================
# Trimming Functions
# ============================================================================

def string_trim(s: str) -> str:
    """
    Remove leading and trailing whitespace from a string.

    Strips whitespace characters (spaces, tabs, newlines) from both the
    beginning and end of the string. The string contents are otherwise
    unchanged.

    Args:
        s: The string to trim

    Returns:
        str: The trimmed string

    Examples:
        # Remove surrounding whitespace
        result = string_trim("  hello world  ")
        print(result)  # Output: hello world

        # Works with various whitespace characters
        result = string_trim("\\n\\t  text  \\r\\n")
        print(result)  # Output: text

        # No-op if string has no leading/trailing whitespace
        result = string_trim("hello")
        print(result)  # Output: hello
    """
    return string.trim(s)

def string_trim_start(s: str) -> str:
    """
    Remove leading whitespace from a string.

    Strips whitespace characters from the beginning of the string only.
    Trailing whitespace is preserved.

    Args:
        s: The string to trim

    Returns:
        str: The string with leading whitespace removed

    Examples:
        # Remove leading whitespace
        result = string_trim_start("  hello  ")
        print(result)  # Output: hello

        # Preserve trailing whitespace
        result = string_trim_start("\\n  text  \\n")
        print(result)  # Output: text  \\n
    """
    return string.trim_start(s)

def string_trim_end(s: str) -> str:
    """
    Remove trailing whitespace from a string.

    Strips whitespace characters from the end of the string only.
    Leading whitespace is preserved.

    Args:
        s: The string to trim

    Returns:
        str: The string with trailing whitespace removed

    Examples:
        # Remove trailing whitespace
        result = string_trim_end("  hello  ")
        print(result)  # Output:   hello

        # Preserve leading whitespace
        result = string_trim_end("  \\ntext  \\n")
        print(result)  # Output:   \\ntext
    """
    return string.trim_end(s)

# ============================================================================
# Splitting Functions
# ============================================================================

def string_split_whitespace(s: str) -> list:
    """
    Split a string by whitespace, returning a list of non-empty words.

    Splits the string on any whitespace character (space, tab, newline, etc.)
    and returns only the non-empty parts. Multiple consecutive whitespace
    characters are treated as a single separator.

    Args:
        s: The string to split

    Returns:
        list: A list of non-empty words

    Examples:
        # Split by spaces
        words = string_split_whitespace("hello world foo")
        print(words)  # Output: ["hello", "world", "foo"]

        # Handle multiple spaces
        words = string_split_whitespace("a    b    c")
        print(words)  # Output: ["a", "b", "c"]

        # Handle mixed whitespace
        words = string_split_whitespace("one\\ttwo\\n three")
        print(words)  # Output: ["one", "two", "three"]

        # Empty string returns empty list
        words = string_split_whitespace("")
        print(words)  # Output: []
    """
    return string.split_whitespace(s)

def string_split_lines(s: str) -> list:
    """
    Split a string into lines, handling both Unix and Windows line endings.

    Splits the string on newline characters, properly handling both \\n (Unix)
    and \\r\\n (Windows/DOS) line endings. Always returns a list, even if
    the input contains no newlines.

    Args:
        s: The string to split

    Returns:
        list: A list of lines without line ending characters

    Examples:
        # Split Unix-style line endings
        lines = string_split_lines("line1\\nline2\\nline3")
        print(lines)  # Output: ["line1", "line2", "line3"]

        # Handle Windows-style line endings
        lines = string_split_lines("line1\\r\\nline2\\r\\nline3")
        print(lines)  # Output: ["line1", "line2", "line3"]

        # Mixed line endings
        lines = string_split_lines("unix\\nwindows\\r\\nmac\\r")
        # Output includes proper line boundaries

        # String without newlines returns single-element list
        lines = string_split_lines("single line")
        print(lines)  # Output: ["single line"]
    """
    return string.split_lines(s)

# ============================================================================
# String Validation Functions
# ============================================================================

def string_contains(s: str, needle: str, ignore_case: bool = False) -> bool:
    """
    Check if a string contains a substring.

    Returns True if the string contains the needle substring, optionally
    with case-insensitive matching.

    Args:
        s: The string to search in
        needle: The substring to search for
        ignore_case: If True, performs case-insensitive search. Default is False.

    Returns:
        bool: True if the substring is found, False otherwise

    Examples:
        # Basic contains check
        if string_contains("hello world", "world"):
            print("Found!")  # This will print

        # Case-sensitive by default
        if string_contains("Hello", "hello"):
            print("Found")  # Will NOT print
        else:
            print("Not found")  # This will print

        # Case-insensitive search
        if string_contains("Hello", "hello", ignore_case=True):
            print("Found!")  # This will print

        # Check for substring in longer text
        message = "Welcome to our service"
        if string_contains(message, "welcome", ignore_case=True):
            print("Welcome message detected")
    """
    return string.contains(s, needle, ignore_case)

def string_starts_with(s: str, prefix: str) -> bool:
    """
    Check if a string starts with a given prefix.

    Returns True if the string begins with the specified prefix, False otherwise.
    This is a case-sensitive check.

    Args:
        s: The string to check
        prefix: The prefix to look for

    Returns:
        bool: True if the string starts with the prefix, False otherwise

    Examples:
        # Check for prefix
        if string_starts_with("hello-world", "hello"):
            print("Starts with hello")  # This will print

        # Prefix not found
        if string_starts_with("hello-world", "world"):
            print("Found")  # Will NOT print
        else:
            print("Not found")  # This will print

        # Use case-sensitive matching
        if string_starts_with("Hello", "hello"):
            print("Found")  # Will NOT print
        elif string_starts_with("Hello", "Hello"):
            print("Found with correct case")  # This will print

        # URL path checking
        if string_starts_with(path, "/api/"):
            print("This is an API endpoint")
    """
    return string.starts_with(s, prefix)

def string_ends_with(s: str, suffix: str) -> bool:
    """
    Check if a string ends with a given suffix.

    Returns True if the string ends with the specified suffix, False otherwise.
    This is a case-sensitive check.

    Args:
        s: The string to check
        suffix: The suffix to look for

    Returns:
        bool: True if the string ends with the suffix, False otherwise

    Examples:
        # Check for suffix
        if string_ends_with("hello.txt", ".txt"):
            print("Is a text file")  # This will print

        # File extension check
        if string_ends_with("document.pdf", ".pdf"):
            print("PDF file detected")  # This will print
        else:
            print("Not a PDF")

        # Domain validation
        if string_ends_with(email, "@example.com"):
            print("Valid company email")

        # Case-sensitive suffix matching
        if string_ends_with("Hello", "lo"):
            print("Ends with 'lo'")  # This will print
        elif string_ends_with("Hello", "LO"):
            print("Found")  # Will NOT print
    """
    return string.ends_with(s, suffix)

# ============================================================================
# String Replacement Functions
# ============================================================================

def string_replace(s: str, from_str: str, to_str: str, count: int = -1, regex: bool = False, ignore_case: bool = False) -> str:
    """
    Replace occurrences of a substring with another string.

    Performs string replacement, optionally supporting regex patterns and
    case-insensitive matching. By default, replaces all occurrences. Use
    'count' to limit the number of replacements.

    Args:
        s: The string to perform replacements in
        from_str: The substring (or regex pattern) to replace
        to_str: The replacement string
        count: Maximum number of replacements (-1 for unlimited). Default is -1.
        regex: If True, treats from_str as a regex pattern. Default is False.
        ignore_case: If True, performs case-insensitive matching. Default is False.

    Returns:
        str: The string with replacements applied

    Examples:
        # Simple replacement
        result = string_replace("hello world", "world", "universe")
        print(result)  # Output: hello universe

        # Replace with limit
        result = string_replace("aaa", "a", "b", count=2)
        print(result)  # Output: bba

        # Case-insensitive replacement
        result = string_replace("Hello HELLO hello", "hello", "hi", ignore_case=True)
        print(result)  # Output: hi hi hi

        # Regex pattern replacement
        result = string_replace("abc123def456", r"\\d+", "X", regex=True)
        print(result)  # Output: abcXdefX

        # Regex with capture groups
        result = string_replace("John Doe", r"(\\w+) (\\w+)", "$2, $1", regex=True)
        # Note: Capture group replacement depends on rust-regex behavior
    """
    return string.replace(s, from_str, to_str, count, regex, ignore_case)

# ============================================================================
# Regex Functions
# ============================================================================

def string_regex_match(pattern: str, s: str):
    """
    Match a regex pattern against a string.

    Searches for the first match of the regex pattern anywhere in the string.
    Returns a dictionary with match details for the first match found, or None if there is no match.

    The returned dictionary contains:
    - 'match': The matched text
    - 'start': Starting character (Unicode codepoint) index of the match
    - 'end': Ending character (Unicode codepoint) index of the match
    - 'groups': List of captured groups
    - 'named': Dictionary of named capture groups
    - 'source': The original source string

    Args:
        pattern: The regex pattern to match
        s: The string to search in

    Returns:
        dict or None: Match information if found, None otherwise

    Raises:
        Error: If the regex pattern is invalid

    Examples:
        # Simple pattern match
        match = string_regex_match(r"\\d+", "version 42")
        if match:
            print(f"Found number: {match['match']}")  # Output: Found number: 42

        # Match with capture groups
        match = string_regex_match(r"(\\w+)@(\\w+\\.\\w+)", "user@example.com")
        if match:
            print(f"Email: {match['match']}")  # Output: Email: user@example.com
            print(f"Groups: {match['groups']}")  # Output: Groups: ["user", "example.com"]

        # Match with named groups
        pattern = r"(?P<year>\\d{4})-(?P<month>\\d{2})"
        match = string_regex_match(pattern, "2024-03-15")
        if match:
            print(f"Year: {match['named']['year']}")  # Output: Year: 2024

        # No match returns None
        match = string_regex_match(r"\\d+", "no numbers here")
        if match is None:
            print("No match found")
    """
    return string.regex_match(pattern, s)

def string_regex_find_all(pattern: str, s: str) -> list:
    """
    Find all regex matches in a string.

    Finds all non-overlapping matches of the regex pattern in the string.
    Returns a list of match dictionaries, each containing match details.

    Each element in the returned list contains:
    - 'match': The matched text
    - 'start': Starting character (Unicode codepoint) index
    - 'end': Ending character (Unicode codepoint) index
    - 'groups': List of captured groups
    - 'named': Dictionary of named captures
    - 'source': Original source string

    Args:
        pattern: The regex pattern to find
        s: The string to search in

    Returns:
        list: List of match dictionaries, empty list if no matches

    Raises:
        Error: If the regex pattern is invalid

    Examples:
        # Find all numbers in string
        matches = string_regex_find_all(r"\\d+", "I have 2 apples and 3 oranges")
        print(len(matches))  # Output: 2
        for match in matches:
            print(match['match'])  # Output: 2, then: 3

        # Find all email addresses
        pattern = r"[\\w.-]+@[\\w.-]+"
        text = "Contact: alice@example.com or bob@test.org"
        matches = string_regex_find_all(pattern, text)
        for match in matches:
            print(match['match'])  # Output: alice@example.com, then: bob@test.org

        # Extract all quoted strings
        pattern = r'\\"([^\\"]*)\\"'
        text = '"first" and "second" and "third"'
        matches = string_regex_find_all(pattern, text)
        print(len(matches))  # Output: 3

        # No matches returns empty list
        matches = string_regex_find_all(r"\\d+", "no digits here")
        print(len(matches))  # Output: 0
    """
    return string.regex_find_all(pattern, s)

def string_regex_captures(pattern: str, s: str):
    """
    Extract named capture groups from the first regex match.

    Matches the regex pattern and returns a dictionary of named capture groups
    from the first match. Unnamed groups are ignored.

    Args:
        pattern: The regex pattern with named capture groups
        s: The string to search in

    Returns:
        dict or None: Dictionary of named captures if match found, None otherwise

    Raises:
        Error: If the regex pattern is invalid

    Examples:
        # Extract named groups
        pattern = r"(?P<first>\\w+) (?P<last>\\w+)"
        match = string_regex_captures(pattern, "John Doe")
        if match:
            print(f"First: {match['first']}, Last: {match['last']}")
            # Output: First: John, Last: Doe

        # Parse date components
        pattern = r"(?P<year>\\d{4})-(?P<month>\\d{2})-(?P<day>\\d{2})"
        match = string_regex_captures(pattern, "2024-03-15")
        if match:
            print(f"Date: {match['day']}/{match['month']}/{match['year']}")
            # Output: Date: 15/03/2024

        # No match returns None
        pattern = r"(?P<digit>\\d+)"
        match = string_regex_captures(pattern, "no digits")
        if match is None:
            print("No match found")
    """
    return string.regex_captures(pattern, s)

# ============================================================================
# Case Conversion Functions
# ============================================================================

def string_upper(s: str) -> str:
    """
    Convert a string to uppercase.

    Converts all lowercase letters to uppercase. Non-letter characters
    are unchanged.

    Args:
        s: The string to convert

    Returns:
        str: The uppercase string

    Examples:
        # Convert to uppercase
        result = string_upper("hello")
        print(result)  # Output: HELLO

        # Mixed case
        result = string_upper("Hello World 123")
        print(result)  # Output: HELLO WORLD 123

        # Already uppercase
        result = string_upper("HELLO")
        print(result)  # Output: HELLO
    """
    return string.to_upper(s)

def string_lower(s: str) -> str:
    """
    Convert a string to lowercase.

    Converts all uppercase letters to lowercase. Non-letter characters
    are unchanged.

    Args:
        s: The string to convert

    Returns:
        str: The lowercase string

    Examples:
        # Convert to lowercase
        result = string_lower("HELLO")
        print(result)  # Output: hello

        # Mixed case
        result = string_lower("Hello World 123")
        print(result)  # Output: hello world 123

        # Already lowercase
        result = string_lower("hello")
        print(result)  # Output: hello
    """
    return string.to_lower(s)

def string_title_case(s: str) -> str:
    """
    Convert a string to Title Case.

    Converts the first letter of each word to uppercase and remaining
    letters to lowercase. Words are separated by spaces, hyphens, or underscores.

    Args:
        s: The string to convert

    Returns:
        str: The title-cased string

    Examples:
        # Simple title case
        result = string_title_case("hello world")
        print(result)  # Output: Hello World

        # Multiple separators
        result = string_title_case("hello-world_foo bar")
        print(result)  # Output: Hello World Foo Bar

        # With numbers
        result = string_title_case("chapter 1 introduction")
        print(result)  # Output: Chapter 1 Introduction

        # Already mixed case
        result = string_title_case("heLLo WoRLd")
        print(result)  # Output: Hello World
    """
    return string.title_case(s)

def string_snake_case(s: str) -> str:
    """
    Convert a string to snake_case.

    Converts a string to lowercase with words separated by underscores.
    Handles camelCase, PascalCase, and space-separated words.

    Args:
        s: The string to convert

    Returns:
        str: The snake_case string

    Examples:
        # Space-separated to snake_case
        result = string_snake_case("hello world")
        print(result)  # Output: hello_world

        # CamelCase to snake_case
        result = string_snake_case("helloWorld")
        print(result)  # Output: hello_world

        # PascalCase to snake_case
        result = string_snake_case("HelloWorld")
        print(result)  # Output: hello_world

        # Mixed separators
        result = string_snake_case("hello-world foo bar")
        print(result)  # Output: hello_world_foo_bar

        # With numbers
        result = string_snake_case("version2Release3Beta")
        print(result)  # Output: version2_release3_beta
    """
    return string.snake_case(s)

def string_kebab_case(s: str) -> str:
    """
    Convert a string to kebab-case.

    Converts a string to lowercase with words separated by hyphens.
    Handles camelCase, PascalCase, and space-separated words.

    Args:
        s: The string to convert

    Returns:
        str: The kebab-case string

    Examples:
        # Space-separated to kebab-case
        result = string_kebab_case("hello world")
        print(result)  # Output: hello-world

        # CamelCase to kebab-case
        result = string_kebab_case("helloWorld")
        print(result)  # Output: hello-world

        # PascalCase to kebab-case
        result = string_kebab_case("HelloWorld")
        print(result)  # Output: hello-world

        # Mixed separators
        result = string_kebab_case("hello_world foo bar")
        print(result)  # Output: hello-world-foo-bar

        # URL-style usage
        result = string_kebab_case("myProductName")
        url_path = f"/api/{string_kebab_case(result)}"
        print(url_path)  # Output: /api/my-product-name
    """
    return string.kebab_case(s)

def string_camel_case(s: str) -> str:
    """
    Convert a string to camelCase.

    Converts a string to camelCase with the first word lowercase and
    subsequent words capitalized. Handles multiple word separators.

    Args:
        s: The string to convert

    Returns:
        str: The camelCase string

    Examples:
        # Space-separated to camelCase
        result = string_camel_case("hello world")
        print(result)  # Output: helloWorld

        # Snake case to camelCase
        result = string_camel_case("hello_world_foo")
        print(result)  # Output: helloWorldFoo

        # Kebab case to camelCase
        result = string_camel_case("hello-world-bar")
        print(result)  # Output: helloWorldBar

        # PascalCase to camelCase
        result = string_camel_case("HelloWorld")
        print(result)  # Output: helloWorld

        # JavaScript-style variable name
        var_name = string_camel_case("user first name")
        print(f"var {var_name} = ...")  # Output: var userFirstName = ...
    """
    return string.camel_case(s)

# ============================================================================
# Padding Functions
# ============================================================================

def string_pad_left(s: str, width: int, fill: str = " ") -> str:
    """
    Pad a string on the left to a target width.

    Adds characters to the left side of the string to reach the target width.
    If the string is already at or longer than the target width, returns
    the original string unchanged.

    Args:
        s: The string to pad
        width: The target width in characters
        fill: The fill character(s) to use. Default is space. Only the first
              character is used for single-character fills.

    Returns:
        str: The left-padded string

    Examples:
        # Pad with spaces
        result = string_pad_left("42", 5)
        print(f"'{result}'")  # Output: '   42'

        # Pad with custom character
        result = string_pad_left("42", 5, "0")
        print(f"'{result}'")  # Output: '00042'

        # Already wide enough
        result = string_pad_left("hello", 3)
        print(result)  # Output: hello

        # Numeric formatting
        nums = ["1", "22", "333"]
        for num in nums:
            print(f"'{string_pad_left(num, 5, '0')}'")
            # Output: '00001', '00022', '00333'
    """
    return string.pad_left(s, width, fill)

def string_pad_right(s: str, width: int, fill: str = " ") -> str:
    """
    Pad a string on the right to a target width.

    Adds characters to the right side of the string to reach the target width.
    If the string is already at or longer than the target width, returns
    the original string unchanged.

    Args:
        s: The string to pad
        width: The target width in characters
        fill: The fill character(s) to use. Default is space.

    Returns:
        str: The right-padded string

    Examples:
        # Pad with spaces
        result = string_pad_right("42", 5)
        print(f"'{result}'")  # Output: '42   '

        # Pad with custom character
        result = string_pad_right("hi", 5, ".")
        print(f"'{result}'")  # Output: 'hi...'

        # Already wide enough
        result = string_pad_right("hello", 3)
        print(result)  # Output: hello

        # Table column alignment
        for value in ["a", "bb", "ccc"]:
            print(f"[{string_pad_right(value, 5)}]")
            # Output: [a    ], [bb   ], [ccc  ]
    """
    return string.pad_right(s, width, fill)

# ============================================================================
# Table Formatting
# ============================================================================

def string_format_table(rows: list) -> str:
    """
    Format a list of dictionaries as an ASCII table.

    Converts a list of dictionaries into a formatted ASCII table with
    column headers and borders. Each dictionary represents a row, with
    keys as column names and values as cell contents.

    Args:
        rows: A list of dictionaries, each representing a table row.
              All rows should have consistent keys (columns).

    Returns:
        str: The formatted ASCII table as a string

    Examples:
        # Simple table
        data = [
            {"name": "Alice", "age": "30"},
            {"name": "Bob", "age": "25"},
            {"name": "Charlie", "age": "35"},
        ]
        print(string_format_table(data))
        # Output:
        # +---------+-----+
        # | name    | age |
        # +---------+-----+
        # | Alice   | 30  |
        # | Bob     | 25  |
        # | Charlie | 35  |
        # +---------+-----+

        # Table with longer content
        results = [
            {"endpoint": "/api/users", "status": "200", "time": "45ms"},
            {"endpoint": "/api/posts", "status": "404", "time": "12ms"},
        ]
        print(string_format_table(results))

        # Dynamic table generation
        files = [
            {"name": "config.json", "size": "1.2 KB"},
            {"name": "data.csv", "size": "256 KB"},
            {"name": "readme.md", "size": "5.3 KB"},
        ]
        print(string_format_table(files))
    """
    return string.format_table(rows)
