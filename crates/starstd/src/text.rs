use crate::is_lsp_mode;
use anyhow::{Context, anyhow};
use anyhow_source_location::format_context;
use regex::{Regex, RegexSet};
use serde::{Deserialize, Serialize};
use serde_json;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;
use starlark::values::list::UnpackList;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanFileOptions {
    pub path: String,
    pub encoding: Option<String>,
    pub strip_newline: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrepOptions {
    pub path: String,
    pub pattern: String,
    pub ignore_case: Option<bool>,
    pub invert: Option<bool>,
    pub max: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticOptions {
    pub file: String,
    pub severity: String,
    pub message: String,
    pub line: Option<i32>,
    pub column: Option<i32>,
    pub end_line: Option<i32>,
    pub end_column: Option<i32>,
    pub code: Option<String>,
    pub source: Option<String>,
}

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Stream a file line-by-line and invoke a callback for each line.
    ///
    /// This function reads a file from disk line-by-line without loading the entire file into memory,
    /// making it efficient for processing large files. For each line, it invokes a callback function
    /// and collects non-None results.
    ///
    /// # Arguments
    ///
    /// * `options` - A dictionary with the following keys:
    ///   * `path` (string, required): Path to the file to scan
    ///   * `encoding` (string, optional): Encoding to use. Either "utf-8" (default, strict) or "lossy" (replaces invalid UTF-8)
    ///   * `strip_newline` (bool, optional): Whether to strip newline characters from each line (default: true)
    /// * `callback` - A function that takes `(line: string, line_number: int)` and returns any value.
    ///   Return `None` to exclude the result from the output list.
    ///
    /// # Returns
    ///
    /// A list containing all non-None values returned by the callback function.
    ///
    /// # Example
    ///
    /// ```python
    /// # Find lines containing "error"
    /// errors = text.scan_file(
    ///     {"path": "app.log"},
    ///     lambda line, num: {"line": num, "text": line} if "error" in line.lower() else None
    /// )
    /// ```
    fn scan_file<'v>(
        options: Value<'v>,
        callback: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let opts: ScanFileOptions = serde_json::from_value(options.to_json_value()?)
            .context(format_context!("bad options for scan_file"))?;

        let encoding = opts.encoding.as_deref().unwrap_or("utf-8");
        let strip = opts.strip_newline.unwrap_or(true);

        let file =
            File::open(&opts.path).context(format_context!("failed to open file {}", opts.path))?;
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let mut results = Vec::new();
        let mut line_number = 1i32;

        loop {
            buffer.clear();
            let n = reader
                .read_until(b'\n', &mut buffer)
                .context(format_context!("failed to read line from {}", opts.path))?;

            if n == 0 {
                break;
            }

            let line_str = if encoding == "lossy" {
                String::from_utf8_lossy(&buffer).to_string()
            } else if encoding == "utf-8" {
                std::str::from_utf8(&buffer)
                    .context(format_context!(
                        "invalid UTF-8 at line {} in {}",
                        line_number,
                        opts.path
                    ))?
                    .to_string()
            } else {
                return Err(anyhow!(format_context!(
                    "unsupported encoding '{}'; use 'utf-8' or 'lossy'",
                    encoding
                )));
            };

            let line_str = if strip {
                strip_newline_str(&line_str)
            } else {
                line_str
            };

            let heap = eval.heap();
            let result = eval
                .eval_function(
                    callback,
                    &[heap.alloc(line_str), heap.alloc(line_number)],
                    &[],
                )
                .map_err(|e| anyhow!(format_context!("callback failed: {}", e)))?;

            if !result.is_none() {
                results.push(result);
            }

            line_number += 1;
        }

        Ok(results)
    }

    /// Split a string into lines and invoke a callback for each line.
    ///
    /// This function splits a string by newline characters (handling both `\n` and `\r\n`) and
    /// invokes a callback for each line. This is useful for processing text content already in memory.
    ///
    /// # Arguments
    ///
    /// * `content` - The string to split into lines
    /// * `callback` - A function that takes `(line: string, line_number: int)` and returns any value.
    ///   Return `None` to exclude the result from the output list.
    ///
    /// # Returns
    ///
    /// A list containing all non-None values returned by the callback function.
    ///
    /// # Example
    ///
    /// ```python
    /// # Count non-empty lines
    /// content = fs.read_file("data.txt")
    /// non_empty = text.scan_lines(
    ///     content,
    ///     lambda line, num: 1 if line.strip() else None
    /// )
    /// count = len(non_empty)
    /// ```
    fn scan_lines<'v>(
        content: &str,
        callback: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        let mut line_number = 1i32;

        let bytes = content.as_bytes();
        let mut start = 0usize;
        let mut i = 0usize;

        while i <= bytes.len() {
            if i == bytes.len() || bytes[i] == b'\n' {
                let end = if i > start && i > 0 && bytes[i - 1] == b'\r' {
                    i - 1
                } else {
                    i
                };

                let line_str = &content[start..end];
                let heap = eval.heap();
                let result = eval
                    .eval_function(
                        callback,
                        &[heap.alloc(line_str.to_string()), heap.alloc(line_number)],
                        &[],
                    )
                    .map_err(|e| anyhow!(format_context!("callback failed: {}", e)))?;

                if !result.is_none() {
                    results.push(result);
                }

                line_number += 1;
                start = i + 1;
            }
            i += 1;
        }

        Ok(results)
    }

    /// Return the number of lines in a file.
    ///
    /// Counts the number of lines by counting newline characters. If the file is non-empty and
    /// doesn't end with a newline, the last line is still counted. This function reads the file
    /// in chunks for efficiency.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file
    ///
    /// # Returns
    ///
    /// The number of lines in the file as an integer.
    ///
    /// # Example
    ///
    /// ```python
    /// count = text.line_count("large_file.txt")
    /// print("File has {} lines".format(count))
    /// ```
    fn line_count(path: &str) -> anyhow::Result<i32> {
        if is_lsp_mode() {
            return Ok(0);
        }

        let mut file = File::open(path).context(format_context!("failed to open file {}", path))?;
        let mut buffer = [0u8; 64 * 1024];
        let mut count = 0i32;
        let mut last_byte: Option<u8> = None;

        loop {
            let n = file
                .read(&mut buffer)
                .context(format_context!("failed to read from {}", path))?;
            if n == 0 {
                break;
            }

            for &byte in &buffer[..n] {
                if byte == b'\n' {
                    count += 1;
                }
                last_byte = Some(byte);
            }
        }

        // If file is non-empty and doesn't end with newline, add one
        if let Some(b) = last_byte {
            if b != b'\n' {
                count += 1;
            }
        }

        Ok(count)
    }

    /// Read a range of lines from a file.
    ///
    /// Reads lines from `start` to `end` (inclusive, 1-based indexing). This is efficient for
    /// extracting a specific section of a file without reading the entire file into memory.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file
    /// * `start` - First line to read (1-based, must be >= 1)
    /// * `end` - Last line to read (1-based, inclusive, must be >= start)
    ///
    /// # Returns
    ///
    /// A list of strings, one per line (newlines are stripped).
    ///
    /// # Example
    ///
    /// ```python
    /// # Read lines 10-20 from a file
    /// lines = text.read_line_range("data.txt", 10, 20)
    /// for line in lines:
    ///     print(line)
    /// ```
    fn read_line_range(path: &str, start: i32, end: i32) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        if start < 1 {
            return Err(anyhow!(format_context!("start must be >= 1")));
        }
        if end < start {
            return Err(anyhow!(format_context!("end must be >= start")));
        }

        let file = File::open(path).context(format_context!("failed to open file {}", path))?;
        let reader = BufReader::new(file);
        let mut result = Vec::new();
        let mut line_number = 1i32;

        for line in reader.lines() {
            if line_number > end {
                break;
            }

            let line_str = line.context(format_context!(
                "failed to read line {} from {}",
                line_number,
                path
            ))?;

            if line_number >= start {
                result.push(line_str);
            }

            line_number += 1;
        }

        Ok(result)
    }

    /// Return the first n lines of a file.
    ///
    /// Reads and returns the first `n` lines from a file, similar to the Unix `head` command.
    /// Stops reading after `n` lines for efficiency.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file
    /// * `n` - Number of lines to read (must be >= 0)
    ///
    /// # Returns
    ///
    /// A list of strings containing the first n lines (newlines are stripped).
    ///
    /// # Example
    ///
    /// ```python
    /// # Get the first 5 lines of a log file
    /// first_lines = text.head("app.log", 5)
    /// ```
    fn head(path: &str, n: i32) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        if n < 0 {
            return Err(anyhow!(format_context!("n must be non-negative")));
        }

        let file = File::open(path).context(format_context!("failed to open file {}", path))?;
        let reader = BufReader::new(file);
        let mut result = Vec::new();

        for line in reader.lines().take(n as usize) {
            let line_str = line.context(format_context!("failed to read line from {}", path))?;
            result.push(line_str);
        }

        Ok(result)
    }

    /// Return the last n lines of a file.
    ///
    /// Reads and returns the last `n` lines from a file, similar to the Unix `tail` command.
    /// The entire file is scanned, but only the last `n` lines are kept in memory.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file
    /// * `n` - Number of lines to read (must be >= 0)
    ///
    /// # Returns
    ///
    /// A list of strings containing the last n lines (newlines are stripped).
    ///
    /// # Example
    ///
    /// ```python
    /// # Get the last 10 lines of a log file
    /// last_lines = text.tail("app.log", 10)
    /// ```
    fn tail(path: &str, n: i32) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        if n < 0 {
            return Err(anyhow!(format_context!("n must be non-negative")));
        }

        let file = File::open(path).context(format_context!("failed to open file {}", path))?;
        let reader = BufReader::new(file);
        let mut ring = VecDeque::with_capacity(n as usize);

        for line in reader.lines() {
            let line_str = line.context(format_context!("failed to read line from {}", path))?;

            if ring.len() == n as usize {
                ring.pop_front();
            }
            ring.push_back(line_str);
        }

        Ok(ring.into_iter().collect())
    }

    /// Search for lines in a file matching a regex pattern.
    ///
    /// Searches through a file line-by-line and returns information about lines matching (or not matching)
    /// a regular expression pattern. Similar to the Unix `grep` command but returns structured data.
    ///
    /// # Arguments
    ///
    /// * `options` - A dictionary with the following keys:
    ///   * `path` (string, required): Path to the file to search
    ///   * `pattern` (string, required): Regular expression pattern to match
    ///   * `ignore_case` (bool, optional): If true, perform case-insensitive matching (default: false)
    ///   * `invert` (bool, optional): If true, return lines that don't match (default: false)
    ///   * `max` (int, optional): Maximum number of matches to return (default: unlimited)
    ///
    /// # Returns
    ///
    /// A list of dictionaries, one per matching line, with the following keys:
    /// * `line` (int): Line number (1-based)
    /// * `text` (string): The full text of the matching line
    /// * `match` (string): The portion of the line that matched the pattern
    /// * `named` (dict): Dictionary of named capture groups from the regex
    ///
    /// # Example
    ///
    /// ```python
    /// # Find all error lines with a severity level
    /// matches = text.grep({
    ///     "path": "app.log",
    ///     "pattern": r"ERROR \[(?P<severity>\w+)\]",
    ///     "max": 100
    /// })
    /// for m in matches:
    ///     print("Line {}: severity={}".format(m["line"], m["named"]["severity"]))
    /// ```
    fn grep<'v>(
        options: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let opts: GrepOptions = serde_json::from_value(options.to_json_value()?)
            .context(format_context!("bad options for grep"))?;

        let ignore_case = opts.ignore_case.unwrap_or(false);
        let invert = opts.invert.unwrap_or(false);
        let max_hits = opts.max;

        let pattern_str = if ignore_case {
            format!("(?i){}", opts.pattern)
        } else {
            opts.pattern.clone()
        };

        let re = Regex::new(&pattern_str)
            .context(format_context!("invalid regex pattern '{}'", opts.pattern))?;

        let file =
            File::open(&opts.path).context(format_context!("failed to open file {}", opts.path))?;
        let reader = BufReader::new(file);
        let mut results = Vec::new();
        let mut line_number = 1i32;
        let mut hit_count = 0i32;

        for line in reader.lines() {
            let line_str = line.context(format_context!(
                "failed to read line {} from {}",
                line_number,
                opts.path
            ))?;

            let matches = re.is_match(&line_str);
            let include = if invert { !matches } else { matches };

            if include {
                let heap = eval.heap();
                let mut map = BTreeMap::new();

                map.insert("line".to_string(), heap.alloc(line_number));
                map.insert("text".to_string(), heap.alloc(line_str.clone()));

                if let Some(caps) = re.captures(&line_str) {
                    if let Some(m) = caps.get(0) {
                        map.insert("match".to_string(), heap.alloc(m.as_str().to_string()));
                    } else {
                        map.insert("match".to_string(), heap.alloc("".to_string()));
                    }

                    let mut named = BTreeMap::new();
                    for name in re.capture_names().flatten() {
                        if let Some(m) = caps.name(name) {
                            named.insert(name.to_string(), m.as_str().to_string());
                        }
                    }
                    map.insert("named".to_string(), heap.alloc(named));
                } else {
                    map.insert("match".to_string(), heap.alloc("".to_string()));
                    map.insert(
                        "named".to_string(),
                        heap.alloc(BTreeMap::<String, String>::new()),
                    );
                }

                results.push(heap.alloc(map));
                hit_count += 1;

                if let Some(max_val) = max_hits {
                    if hit_count >= max_val {
                        break;
                    }
                }
            }

            line_number += 1;
        }

        Ok(results)
    }

    /// Remove common leading whitespace from all non-empty lines.
    ///
    /// This function analyzes all non-empty lines to find the minimum indentation level,
    /// then removes that amount of leading whitespace from each line. This is useful for
    /// processing indented text blocks. Equivalent to Python's `textwrap.dedent()`.
    ///
    /// # Arguments
    ///
    /// * `content` - The string to dedent
    ///
    /// # Returns
    ///
    /// A new string with common leading whitespace removed.
    ///
    /// # Example
    ///
    /// ```python
    /// code = '''
    ///     def hello():
    ///         print("world")
    /// '''
    /// dedented = text.dedent(code)
    /// # Result:
    /// # def hello():
    /// #     print("world")
    /// ```
    fn dedent(content: &str) -> anyhow::Result<String> {
        let lines: Vec<&str> = content.lines().collect();

        // Find minimum indentation of non-empty lines
        let mut min_indent: Option<usize> = None;

        for line in &lines {
            if line.trim().is_empty() {
                continue;
            }

            let indent = line.chars().take_while(|c| c.is_whitespace()).count();
            min_indent = Some(min_indent.map_or(indent, |m| m.min(indent)));
        }

        let min_indent = min_indent.unwrap_or(0);

        // Strip that many characters from each line
        let mut result = String::new();
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                result.push('\n');
            }

            if line.trim().is_empty() {
                // Keep empty lines as-is (might have trailing whitespace)
                result.push_str(line);
            } else {
                let char_indices: Vec<(usize, char)> = line.char_indices().collect();
                if min_indent < char_indices.len() {
                    let byte_start = char_indices[min_indent].0;
                    result.push_str(&line[byte_start..]);
                } else {
                    // Line has fewer characters than min_indent, push as-is or empty
                    result.push_str(line.trim_start());
                }
            }
        }

        Ok(result)
    }

    /// Slide a window of consecutive lines over a string.
    ///
    /// This function processes a string by sliding a window of `n` consecutive lines over it,
    /// invoking a callback for each window position. This is useful for analyzing patterns that
    /// span multiple lines (e.g., function definitions, error blocks).
    ///
    /// # Arguments
    ///
    /// * `content` - The string to process
    /// * `n` - Window size in lines (must be >= 1)
    /// * `callback` - A function that takes `(window: list[string], start_line: int)` and returns any value.
    ///   The window contains up to `n` lines, and `start_line` is the 1-based line number of the first line.
    ///   Return `None` to exclude the result from the output list.
    ///
    /// # Returns
    ///
    /// A list containing all non-None values returned by the callback function.
    ///
    /// # Example
    ///
    /// ```python
    /// # Find function definitions (assume they span 3 lines)
    /// content = fs.read_file("code.py")
    /// functions = text.scan_windows(
    ///     content,
    ///     3,
    ///     lambda window, line: line if "def " in window[0] else None
    /// )
    /// ```
    fn scan_windows<'v>(
        content: &str,
        n: i32,
        callback: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        if n < 1 {
            return Err(anyhow!(format_context!("n must be >= 1")));
        }

        let n_usize = n as usize;
        let mut results = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            return Ok(results);
        }

        for start_idx in 0..lines.len() {
            let end_idx = (start_idx + n_usize).min(lines.len());
            let window: Vec<String> = lines[start_idx..end_idx]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let start_line_number = (start_idx + 1) as i32;

            let heap = eval.heap();
            let result = eval
                .eval_function(
                    callback,
                    &[heap.alloc(window), heap.alloc(start_line_number)],
                    &[],
                )
                .map_err(|e| anyhow!(format_context!("callback failed: {}", e)))?;

            if !result.is_none() {
                results.push(result);
            }
        }

        Ok(results)
    }

    /// Slide a window of consecutive lines over a file.
    ///
    /// Similar to `scan_windows()` but reads from a file on disk, processing it in a streaming fashion
    /// without loading the entire file into memory. This is efficient for large files.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to scan
    /// * `n` - Window size in lines (must be >= 1)
    /// * `callback` - A function that takes `(window: list[string], start_line: int)` and returns any value.
    ///   The window contains up to `n` lines, and `start_line` is the 1-based line number of the first line.
    ///   Return `None` to exclude the result from the output list.
    ///
    /// # Returns
    ///
    /// A list containing all non-None values returned by the callback function.
    ///
    /// # Example
    ///
    /// ```python
    /// # Find error blocks that span 5 lines
    /// errors = text.scan_windows_file(
    ///     "large.log",
    ///     5,
    ///     lambda window, line: {"start": line, "text": window} if "ERROR" in window[0] else None
    /// )
    /// ```
    fn scan_windows_file<'v>(
        path: &str,
        n: i32,
        callback: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        if n < 1 {
            return Err(anyhow!(format_context!("n must be >= 1")));
        }

        let n_usize = n as usize;
        let file = File::open(path).context(format_context!("failed to open file {}", path))?;
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let mut window = VecDeque::with_capacity(n_usize);
        let mut results = Vec::new();
        let mut line_number = 1i32;

        // Read lines and build full windows
        loop {
            buffer.clear();
            let bytes_read = reader
                .read_until(b'\n', &mut buffer)
                .context(format_context!("failed to read line from {}", path))?;

            if bytes_read == 0 {
                break;
            }

            let line_str = std::str::from_utf8(&buffer).context(format_context!(
                "invalid UTF-8 at line {} in {}",
                line_number,
                path
            ))?;
            let line_str = strip_newline_str(line_str);

            window.push_back(line_str);

            // Emit window if it's full
            if window.len() == n_usize {
                let start_line = line_number - (n_usize as i32) + 1;
                let window_vec: Vec<String> = window.iter().cloned().collect();

                let heap = eval.heap();
                let result = eval
                    .eval_function(
                        callback,
                        &[heap.alloc(window_vec), heap.alloc(start_line)],
                        &[],
                    )
                    .map_err(|e| anyhow!(format_context!("callback failed: {}", e)))?;

                if !result.is_none() {
                    results.push(result);
                }

                window.pop_front();
            }

            line_number += 1;
        }

        // Drain remaining windows (shorter than n)
        while window.len() > 1 {
            let start_line = line_number - (window.len() as i32);
            let window_vec: Vec<String> = window.iter().cloned().collect();

            let heap = eval.heap();
            let result = eval
                .eval_function(
                    callback,
                    &[heap.alloc(window_vec), heap.alloc(start_line)],
                    &[],
                )
                .map_err(|e| anyhow!(format_context!("callback failed: {}", e)))?;

            if !result.is_none() {
                results.push(result);
            }

            window.pop_front();
        }

        Ok(results)
    }

    /// Scan content for multiple regex patterns simultaneously.
    ///
    /// This function efficiently searches through content for multiple regex patterns at once,
    /// returning detailed information about all matches found. It uses a RegexSet internally
    /// for efficient multi-pattern matching.
    ///
    /// # Arguments
    ///
    /// * `content` - The string to search
    /// * `patterns` - A list of regex pattern strings to search for
    ///
    /// # Returns
    ///
    /// A list of dictionaries, one per match, with the following keys:
    /// * `pattern_index` (int): Index of the pattern that matched (0-based)
    /// * `line` (int): Line number where the match occurred (1-based)
    /// * `column` (int): Column number where the match starts (1-based, character offset)
    /// * `match` (string): The text that matched
    /// * `named` (dict): Dictionary of named capture groups from the regex
    ///
    /// # Example
    ///
    /// ```python
    /// log_content = fs.read_file("app.log")
    /// matches = text.regex_scan(log_content, [
    ///     r"ERROR: (?P<msg>.*)",
    ///     r"WARN: (?P<msg>.*)",
    ///     r"FATAL: (?P<msg>.*)"
    /// ])
    /// for m in matches:
    ///     print("Pattern {}: {}".format(m["pattern_index"], m["named"]["msg"]))
    /// ```
    fn regex_scan<'v>(
        content: &str,
        patterns: UnpackList<String>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let patterns_vec: Vec<String> = patterns.items;
        if patterns_vec.is_empty() {
            return Ok(Vec::new());
        }

        // Compile individual regexes
        let regexes: Vec<Regex> = patterns_vec
            .iter()
            .enumerate()
            .map(|(i, p)| {
                Regex::new(p).context(format_context!(
                    "invalid regex pattern {} at index {}",
                    p,
                    i
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Build a RegexSet for fast matching
        let regex_set =
            RegexSet::new(&patterns_vec).context(format_context!("failed to create RegexSet"))?;

        let heap = eval.heap();
        let mut results = Vec::new();
        let mut line_number = 1i32;

        for line in content.lines() {
            // Check if any pattern matches
            let matches = regex_set.matches(line);

            for pattern_idx in matches.iter() {
                let re = &regexes[pattern_idx];
                if let Some(caps) = re.captures(line) {
                    if let Some(m) = caps.get(0) {
                        let byte_start = m.start();
                        let column = line[..byte_start].chars().count() as i32 + 1;

                        let mut named = BTreeMap::new();
                        for name in re.capture_names().flatten() {
                            if let Some(capture) = caps.name(name) {
                                named.insert(name.to_string(), capture.as_str().to_string());
                            }
                        }

                        let mut result_map = BTreeMap::new();
                        result_map
                            .insert("pattern_index".to_string(), heap.alloc(pattern_idx as i32));
                        result_map.insert("line".to_string(), heap.alloc(line_number));
                        result_map.insert("column".to_string(), heap.alloc(column));
                        result_map.insert("match".to_string(), heap.alloc(m.as_str().to_string()));
                        result_map.insert("named".to_string(), heap.alloc(named));

                        results.push(heap.alloc(result_map));
                    }
                }
            }

            line_number += 1;
        }

        Ok(results)
    }

    /// Scan a file for multiple regex patterns simultaneously.
    ///
    /// Similar to `regex_scan()` but reads from a file on disk in a streaming fashion,
    /// making it efficient for large files.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to scan
    /// * `patterns` - A list of regex pattern strings to search for
    ///
    /// # Returns
    ///
    /// A list of dictionaries, one per match, with the following keys:
    /// * `pattern_index` (int): Index of the pattern that matched (0-based)
    /// * `line` (int): Line number where the match occurred (1-based)
    /// * `column` (int): Column number where the match starts (1-based, character offset)
    /// * `match` (string): The text that matched
    /// * `named` (dict): Dictionary of named capture groups from the regex
    ///
    /// # Example
    ///
    /// ```python
    /// matches = text.regex_scan_file("large.log", [
    ///     r"ERROR: (?P<msg>.*)",
    ///     r"FATAL: (?P<msg>.*)"
    /// ])
    /// ```
    fn regex_scan_file<'v>(
        path: &str,
        patterns: UnpackList<String>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let patterns_vec: Vec<String> = patterns.items;
        if patterns_vec.is_empty() {
            return Ok(Vec::new());
        }

        // Compile individual regexes
        let regexes: Vec<Regex> = patterns_vec
            .iter()
            .enumerate()
            .map(|(i, p)| {
                Regex::new(p).context(format_context!(
                    "invalid regex pattern {} at index {}",
                    p,
                    i
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Build a RegexSet for fast matching
        let regex_set =
            RegexSet::new(&patterns_vec).context(format_context!("failed to create RegexSet"))?;

        let file = File::open(path).context(format_context!("failed to open file {}", path))?;
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let heap = eval.heap();
        let mut results = Vec::new();
        let mut line_number = 1i32;

        loop {
            buffer.clear();
            let bytes_read = reader
                .read_until(b'\n', &mut buffer)
                .context(format_context!("failed to read line from {}", path))?;

            if bytes_read == 0 {
                break;
            }

            let line = std::str::from_utf8(&buffer).context(format_context!(
                "invalid UTF-8 at line {} in {}",
                line_number,
                path
            ))?;
            let line = strip_newline_str(line);

            // Check if any pattern matches
            let matches = regex_set.matches(&line);

            for pattern_idx in matches.iter() {
                let re = &regexes[pattern_idx];
                if let Some(caps) = re.captures(&line) {
                    if let Some(m) = caps.get(0) {
                        let byte_start = m.start();
                        let column = line[..byte_start].chars().count() as i32 + 1;

                        let mut named = BTreeMap::new();
                        for name in re.capture_names().flatten() {
                            if let Some(capture) = caps.name(name) {
                                named.insert(name.to_string(), capture.as_str().to_string());
                            }
                        }

                        let mut result_map = BTreeMap::new();
                        result_map
                            .insert("pattern_index".to_string(), heap.alloc(pattern_idx as i32));
                        result_map.insert("line".to_string(), heap.alloc(line_number));
                        result_map.insert("column".to_string(), heap.alloc(column));
                        result_map.insert("match".to_string(), heap.alloc(m.as_str().to_string()));
                        result_map.insert("named".to_string(), heap.alloc(named));

                        results.push(heap.alloc(result_map));
                    }
                }
            }

            line_number += 1;
        }

        Ok(results)
    }

    /// Scan content for multiple tagged regex patterns.
    ///
    /// Similar to `regex_scan()` but allows associating a custom tag with each pattern.
    /// This makes it easier to identify which type of pattern matched without tracking indices.
    ///
    /// # Arguments
    ///
    /// * `content` - The string to search
    /// * `patterns` - A list of dictionaries, each with:
    ///   * `tag` (string): A custom identifier for this pattern
    ///   * `pattern` (string): The regex pattern to match
    ///
    /// # Returns
    ///
    /// A list of dictionaries, one per match, with the following keys:
    /// * `tag` (string): The tag associated with the matched pattern
    /// * `line` (int): Line number where the match occurred (1-based)
    /// * `column` (int): Column number where the match starts (1-based, character offset)
    /// * `match` (string): The text that matched
    /// * `named` (dict): Dictionary of named capture groups from the regex
    ///
    /// # Example
    ///
    /// ```python
    /// log_content = fs.read_file("app.log")
    /// matches = text.regex_scan_tagged(log_content, [
    ///     {"tag": "error", "pattern": r"ERROR: (?P<msg>.*)"},
    ///     {"tag": "warning", "pattern": r"WARN: (?P<msg>.*)"},
    ///     {"tag": "fatal", "pattern": r"FATAL: (?P<msg>.*)"}
    /// ])
    /// for m in matches:
    ///     print("{}: {}".format(m["tag"], m["named"]["msg"]))
    /// ```
    fn regex_scan_tagged<'v>(
        content: &str,
        patterns: UnpackList<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let patterns_vec = patterns.items;
        if patterns_vec.is_empty() {
            return Ok(Vec::new());
        }

        // Extract tags and pattern strings
        let mut tags = Vec::new();
        let mut pattern_strings = Vec::new();

        for (i, val) in patterns_vec.iter().enumerate() {
            let json = val
                .to_json_value()
                .context(format_context!("failed to convert pattern at index {}", i))?;
            let obj = json.as_object().ok_or_else(|| {
                anyhow!(format_context!(
                    "pattern at index {} must be a dict with 'tag' and 'pattern' keys",
                    i
                ))
            })?;

            let tag = obj.get("tag").and_then(|v| v.as_str()).ok_or_else(|| {
                anyhow!(format_context!(
                    "pattern at index {} must have 'tag' string key",
                    i
                ))
            })?;
            let pattern = obj.get("pattern").and_then(|v| v.as_str()).ok_or_else(|| {
                anyhow!(format_context!(
                    "pattern at index {} must have 'pattern' string key",
                    i
                ))
            })?;

            tags.push(tag.to_string());
            pattern_strings.push(pattern.to_string());
        }

        // Compile individual regexes
        let regexes: Vec<Regex> = pattern_strings
            .iter()
            .enumerate()
            .map(|(i, p)| {
                Regex::new(p).context(format_context!(
                    "invalid regex pattern {} at index {}",
                    p,
                    i
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Build a RegexSet for fast matching
        let regex_set = RegexSet::new(&pattern_strings)
            .context(format_context!("failed to create RegexSet"))?;

        let heap = eval.heap();
        let mut results = Vec::new();
        let mut line_number = 1i32;

        for line in content.lines() {
            // Check if any pattern matches
            let matches = regex_set.matches(line);

            for pattern_idx in matches.iter() {
                let re = &regexes[pattern_idx];
                let tag = &tags[pattern_idx];

                if let Some(caps) = re.captures(line) {
                    if let Some(m) = caps.get(0) {
                        let byte_start = m.start();
                        let column = line[..byte_start].chars().count() as i32 + 1;

                        let mut named = BTreeMap::new();
                        for name in re.capture_names().flatten() {
                            if let Some(capture) = caps.name(name) {
                                named.insert(name.to_string(), capture.as_str().to_string());
                            }
                        }

                        let mut result_map = BTreeMap::new();
                        result_map.insert("tag".to_string(), heap.alloc(tag.clone()));
                        result_map.insert("line".to_string(), heap.alloc(line_number));
                        result_map.insert("column".to_string(), heap.alloc(column));
                        result_map.insert("match".to_string(), heap.alloc(m.as_str().to_string()));
                        result_map.insert("named".to_string(), heap.alloc(named));

                        results.push(heap.alloc(result_map));
                    }
                }
            }

            line_number += 1;
        }

        Ok(results)
    }

    /// Scan a file for multiple tagged regex patterns.
    ///
    /// Similar to `regex_scan_tagged()` but reads from a file on disk in a streaming fashion,
    /// making it efficient for large files.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to scan
    /// * `patterns` - A list of dictionaries, each with:
    ///   * `tag` (string): A custom identifier for this pattern
    ///   * `pattern` (string): The regex pattern to match
    ///
    /// # Returns
    ///
    /// A list of dictionaries, one per match, with the following keys:
    /// * `tag` (string): The tag associated with the matched pattern
    /// * `line` (int): Line number where the match occurred (1-based)
    /// * `column` (int): Column number where the match starts (1-based, character offset)
    /// * `match` (string): The text that matched
    /// * `named` (dict): Dictionary of named capture groups from the regex
    ///
    /// # Example
    ///
    /// ```python
    /// matches = text.regex_scan_tagged_file("large.log", [
    ///     {"tag": "error", "pattern": r"ERROR: (?P<msg>.*)"},
    ///     {"tag": "fatal", "pattern": r"FATAL: (?P<msg>.*)"}
    /// ])
    /// ```
    fn regex_scan_tagged_file<'v>(
        path: &str,
        patterns: UnpackList<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let patterns_vec = patterns.items;
        if patterns_vec.is_empty() {
            return Ok(Vec::new());
        }

        // Extract tags and pattern strings
        let mut tags = Vec::new();
        let mut pattern_strings = Vec::new();

        for (i, val) in patterns_vec.iter().enumerate() {
            let json = val
                .to_json_value()
                .context(format_context!("failed to convert pattern at index {}", i))?;
            let obj = json.as_object().ok_or_else(|| {
                anyhow!(format_context!(
                    "pattern at index {} must be a dict with 'tag' and 'pattern' keys",
                    i
                ))
            })?;

            let tag = obj.get("tag").and_then(|v| v.as_str()).ok_or_else(|| {
                anyhow!(format_context!(
                    "pattern at index {} must have 'tag' string key",
                    i
                ))
            })?;
            let pattern = obj.get("pattern").and_then(|v| v.as_str()).ok_or_else(|| {
                anyhow!(format_context!(
                    "pattern at index {} must have 'pattern' string key",
                    i
                ))
            })?;

            tags.push(tag.to_string());
            pattern_strings.push(pattern.to_string());
        }

        // Compile individual regexes
        let regexes: Vec<Regex> = pattern_strings
            .iter()
            .enumerate()
            .map(|(i, p)| {
                Regex::new(p).context(format_context!(
                    "invalid regex pattern {} at index {}",
                    p,
                    i
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Build a RegexSet for fast matching
        let regex_set = RegexSet::new(&pattern_strings)
            .context(format_context!("failed to create RegexSet"))?;

        let file = File::open(path).context(format_context!("failed to open file {}", path))?;
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let heap = eval.heap();
        let mut results = Vec::new();
        let mut line_number = 1i32;

        loop {
            buffer.clear();
            let bytes_read = reader
                .read_until(b'\n', &mut buffer)
                .context(format_context!("failed to read line from {}", path))?;

            if bytes_read == 0 {
                break;
            }

            let line = std::str::from_utf8(&buffer).context(format_context!(
                "invalid UTF-8 at line {} in {}",
                line_number,
                path
            ))?;
            let line = strip_newline_str(line);

            // Check if any pattern matches
            let matches = regex_set.matches(&line);

            for pattern_idx in matches.iter() {
                let re = &regexes[pattern_idx];
                let tag = &tags[pattern_idx];

                if let Some(caps) = re.captures(&line) {
                    if let Some(m) = caps.get(0) {
                        let byte_start = m.start();
                        let column = line[..byte_start].chars().count() as i32 + 1;

                        let mut named = BTreeMap::new();
                        for name in re.capture_names().flatten() {
                            if let Some(capture) = caps.name(name) {
                                named.insert(name.to_string(), capture.as_str().to_string());
                            }
                        }

                        let mut result_map = BTreeMap::new();
                        result_map.insert("tag".to_string(), heap.alloc(tag.clone()));
                        result_map.insert("line".to_string(), heap.alloc(line_number));
                        result_map.insert("column".to_string(), heap.alloc(column));
                        result_map.insert("match".to_string(), heap.alloc(m.as_str().to_string()));
                        result_map.insert("named".to_string(), heap.alloc(named));

                        results.push(heap.alloc(result_map));
                    }
                }
            }

            line_number += 1;
        }

        Ok(results)
    }

    /// Create a standardized diagnostic dictionary.
    ///
    /// This function creates a properly formatted diagnostic that can be rendered in various formats
    /// (human-readable, GitHub Actions, JSON, SARIF) using `render_diagnostics()`. Diagnostics are
    /// used to report errors, warnings, and other issues found during linting, building, or testing.
    ///
    /// # Arguments
    ///
    /// * `options` - A dictionary with the following keys:
    ///   * `file` (string, required): Path to the file where the issue was found
    ///   * `severity` (string, required): One of: "error", "warning", "info", "hint", "note"
    ///   * `message` (string, required): Description of the issue
    ///   * `line` (int, optional): Line number where the issue occurs (1-based, must be >= 1)
    ///   * `column` (int, optional): Column number where the issue starts (1-based, must be >= 1)
    ///   * `end_line` (int, optional): Line number where the issue ends (1-based, must be >= 1)
    ///   * `end_column` (int, optional): Column number where the issue ends (1-based, must be >= 1)
    ///   * `code` (string, optional): Error code or rule identifier (e.g., "E501", "no-unused-vars")
    ///   * `source` (string, optional): Name of the tool that generated this diagnostic (e.g., "pylint", "eslint")
    /// * `related` - Optional list of related diagnostics (for additional context)
    ///
    /// # Returns
    ///
    /// A dictionary representing the diagnostic with all provided fields.
    ///
    /// # Example
    ///
    /// ```python
    /// diag = text.diagnostic({
    ///     "file": "src/main.py",
    ///     "severity": "error",
    ///     "message": "Undefined variable 'x'",
    ///     "line": 42,
    ///     "column": 10,
    ///     "code": "E0602",
    ///     "source": "pylint"
    /// })
    /// ```
    fn diagnostic<'v>(
        options: Value<'v>,
        #[starlark(require = named)] related: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<BTreeMap<String, Value<'v>>> {
        if is_lsp_mode() {
            return Ok(BTreeMap::new());
        }

        let opts: DiagnosticOptions = serde_json::from_value(options.to_json_value()?)
            .context(format_context!("bad options for diagnostic"))?;

        // Validate severity
        let severity_lower = opts.severity.to_lowercase();
        if !["error", "warning", "info", "hint", "note"].contains(&severity_lower.as_str()) {
            return Err(anyhow!(
                "severity must be one of: error, warning, info, hint, note; got: {}",
                opts.severity
            )
            .context(format_context!("invalid severity")));
        }

        // Validate numeric fields
        if let Some(l) = opts.line {
            if l < 1 {
                return Err(anyhow!("line must be >= 1, got {}", l)
                    .context(format_context!("invalid line")));
            }
        }
        if let Some(c) = opts.column {
            if c < 1 {
                return Err(anyhow!("column must be >= 1, got {}", c)
                    .context(format_context!("invalid column")));
            }
        }
        if let Some(el) = opts.end_line {
            if el < 1 {
                return Err(anyhow!("end_line must be >= 1, got {}", el)
                    .context(format_context!("invalid end_line")));
            }
        }
        if let Some(ec) = opts.end_column {
            if ec < 1 {
                return Err(anyhow!("end_column must be >= 1, got {}", ec)
                    .context(format_context!("invalid end_column")));
            }
        }

        let heap = eval.heap();
        let mut result = BTreeMap::new();
        result.insert("file".to_string(), heap.alloc(opts.file.as_str()));
        result.insert("severity".to_string(), heap.alloc(severity_lower.as_str()));
        result.insert("message".to_string(), heap.alloc(opts.message.as_str()));

        if let Some(l) = opts.line {
            result.insert("line".to_string(), heap.alloc(l));
        }
        if let Some(c) = opts.column {
            result.insert("column".to_string(), heap.alloc(c));
        }
        if let Some(el) = opts.end_line {
            result.insert("end_line".to_string(), heap.alloc(el));
        }
        if let Some(ec) = opts.end_column {
            result.insert("end_column".to_string(), heap.alloc(ec));
        }
        if let Some(cd) = &opts.code {
            result.insert("code".to_string(), heap.alloc(cd.as_str()));
        }
        if let Some(src) = &opts.source {
            result.insert("source".to_string(), heap.alloc(src.as_str()));
        }
        if let Some(rel) = related {
            result.insert("related".to_string(), rel);
        }

        Ok(result)
    }

    /// Remove duplicate diagnostics from a list.
    ///
    /// Compares diagnostics based on their complete JSON representation and removes duplicates,
    /// preserving only the first occurrence of each unique diagnostic. This is useful when
    /// combining diagnostics from multiple sources that may report the same issue.
    ///
    /// # Arguments
    ///
    /// * `diagnostics` - A list of diagnostic dictionaries
    ///
    /// # Returns
    ///
    /// A new list containing only unique diagnostics, in the order they first appeared.
    ///
    /// # Example
    ///
    /// ```python
    /// all_diags = pylint_diags + mypy_diags + flake8_diags
    /// unique_diags = text.dedup_diagnostics(all_diags)
    /// ```
    fn dedup_diagnostics<'v>(diagnostics: UnpackList<Value<'v>>) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let mut seen = HashSet::new();
        let mut result = Vec::new();

        for diag in diagnostics.items {
            let json_value = diag
                .to_json_value()
                .context(format_context!("failed to convert diagnostic to JSON"))?;
            let json_string = serde_json::to_string(&json_value)
                .context(format_context!("failed to serialize diagnostic"))?;

            if seen.insert(json_string) {
                result.push(diag);
            }
        }

        Ok(result)
    }

    /// Render a list of diagnostics in various formats.
    ///
    /// Converts a list of diagnostic dictionaries into a formatted string suitable for display
    /// or consumption by CI/CD tools.
    ///
    /// # Arguments
    ///
    /// * `diagnostics` - A list of diagnostic dictionaries (created with `diagnostic()`)
    /// * `format` - Output format (default: "human"):
    ///   * `"human"`: Human-readable format like "file.py:10:5: error: message"
    ///   * `"github"`: GitHub Actions workflow commands format (creates annotations)
    ///   * `"json"`: Pretty-printed JSON array
    ///   * `"sarif"`: SARIF 2.1.0 format (Static Analysis Results Interchange Format)
    ///
    /// # Returns
    ///
    /// A formatted string representation of the diagnostics.
    ///
    /// # Example
    ///
    /// ```python
    /// diagnostics = []
    /// diagnostics.append(text.diagnostic({
    ///     "file": "src/main.py",
    ///     "severity": "error",
    ///     "message": "Syntax error",
    ///     "line": 10,
    ///     "column": 5
    /// }))
    ///
    /// # For console output
    /// print(text.render_diagnostics(diagnostics, format="human"))
    ///
    /// # For GitHub Actions
    /// print(text.render_diagnostics(diagnostics, format="github"))
    ///
    /// # For tools that consume SARIF
    /// fs.write_file("results.sarif", text.render_diagnostics(diagnostics, format="sarif"))
    /// ```
    fn render_diagnostics<'v>(
        diagnostics: UnpackList<Value<'v>>,
        #[starlark(require = named, default = "human")] format: &str,
    ) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }

        match format {
            "human" => render_human(diagnostics.items),
            "github" => render_github(diagnostics.items),
            "json" => render_json(diagnostics.items),
            "sarif" => render_sarif(diagnostics.items),
            _ => Err(anyhow!(
                "format must be one of: human, github, json, sarif; got: {}",
                format
            )
            .context(format_context!("invalid format"))),
        }
    }
}

/// Strip a trailing `\n` and any preceding `\r` from a string.
fn strip_newline_str(s: &str) -> String {
    let bytes = s.as_bytes();
    let len = bytes.len();

    if len > 0 && bytes[len - 1] == b'\n' {
        let end = if len > 1 && bytes[len - 2] == b'\r' {
            len - 2
        } else {
            len - 1
        };
        s[..end].to_string()
    } else {
        s.to_string()
    }
}

/// Render diagnostics in human-readable format
fn render_human(diagnostics: Vec<Value>) -> anyhow::Result<String> {
    let mut lines = Vec::new();

    for diag in diagnostics {
        let json_value = diag
            .to_json_value()
            .context(format_context!("failed to convert diagnostic to JSON"))?;
        let obj = json_value.as_object().ok_or_else(|| {
            anyhow!("diagnostic must be a dict").context(format_context!("invalid diagnostic type"))
        })?;

        let file = obj.get("file").and_then(|v| v.as_str()).unwrap_or("");
        let severity = obj
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("error");
        let message = obj.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let line = obj.get("line").and_then(|v| v.as_i64());
        let column = obj.get("column").and_then(|v| v.as_i64());

        let mut output = file.to_string();
        if let Some(l) = line {
            output.push_str(&format!(":{}", l));
            if let Some(c) = column {
                output.push_str(&format!(":{}", c));
            }
        }
        output.push_str(&format!(": {}: {}", severity, message));
        lines.push(output);
    }

    Ok(lines.join("\n"))
}

/// Render diagnostics in GitHub Actions format
fn render_github(diagnostics: Vec<Value>) -> anyhow::Result<String> {
    let mut lines = Vec::new();

    for diag in diagnostics {
        let json_value = diag
            .to_json_value()
            .context(format_context!("failed to convert diagnostic to JSON"))?;
        let obj = json_value.as_object().ok_or_else(|| {
            anyhow!("diagnostic must be a dict").context(format_context!("invalid diagnostic type"))
        })?;

        let file = obj.get("file").and_then(|v| v.as_str()).unwrap_or("");
        let severity = obj
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("error");
        let message = obj.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let line = obj.get("line").and_then(|v| v.as_i64());
        let column = obj.get("column").and_then(|v| v.as_i64());

        // Map severity to GitHub Actions level
        let level = match severity {
            "error" | "hint" => "error",
            "warning" => "warning",
            "info" | "note" => "notice",
            _ => "error",
        };

        let mut output = format!("::{}", level);
        let mut params = vec![format!("file={}", file)];
        if let Some(l) = line {
            params.push(format!("line={}", l));
        }
        if let Some(c) = column {
            params.push(format!("col={}", c));
        }
        output.push(' ');
        output.push_str(&params.join(","));
        output.push_str("::");
        output.push_str(message);
        lines.push(output);
    }

    Ok(lines.join("\n"))
}

/// Render diagnostics in JSON format
fn render_json(diagnostics: Vec<Value>) -> anyhow::Result<String> {
    let mut json_diagnostics = Vec::new();

    for diag in diagnostics {
        let json_value = diag
            .to_json_value()
            .context(format_context!("failed to convert diagnostic to JSON"))?;
        json_diagnostics.push(json_value);
    }

    serde_json::to_string_pretty(&json_diagnostics)
        .context(format_context!("failed to serialize diagnostics as JSON"))
}

/// Render diagnostics in SARIF 2.1.0 format
fn render_sarif(diagnostics: Vec<Value>) -> anyhow::Result<String> {
    let mut results = Vec::new();
    let mut tool_name = "starstd".to_string();
    let mut found_source = false;

    for diag in &diagnostics {
        let json_value = diag
            .to_json_value()
            .context(format_context!("failed to convert diagnostic to JSON"))?;
        let obj = json_value.as_object().ok_or_else(|| {
            anyhow!("diagnostic must be a dict").context(format_context!("invalid diagnostic type"))
        })?;

        // Get tool name from first diagnostic with a source
        if !found_source {
            if let Some(source) = obj.get("source").and_then(|v| v.as_str()) {
                tool_name = source.to_string();
                found_source = true;
            }
        }

        let file = obj.get("file").and_then(|v| v.as_str()).unwrap_or("");
        let severity = obj
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("error");
        let message = obj.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let line = obj.get("line").and_then(|v| v.as_i64());
        let column = obj.get("column").and_then(|v| v.as_i64());

        // Map severity to SARIF level
        let level = match severity {
            "error" => "error",
            "warning" => "warning",
            "info" | "hint" | "note" => "note",
            _ => "warning",
        };

        let mut result = serde_json::json!({
            "level": level,
            "message": {
                "text": message
            }
        });

        // Add location if we have file info
        if !file.is_empty() || line.is_some() {
            let mut location = serde_json::json!({
                "physicalLocation": {
                    "artifactLocation": {
                        "uri": file
                    }
                }
            });

            if line.is_some() || column.is_some() {
                let mut region = serde_json::Map::new();
                if let Some(l) = line {
                    region.insert("startLine".to_string(), serde_json::json!(l));
                }
                if let Some(c) = column {
                    region.insert("startColumn".to_string(), serde_json::json!(c));
                }
                location["physicalLocation"]["region"] = serde_json::json!(region);
            }

            result["locations"] = serde_json::json!([location]);
        }

        results.push(result);
    }

    let sarif = serde_json::json!({
        "version": "2.1.0",
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
        "runs": [{
            "tool": {
                "driver": {
                    "name": tool_name
                }
            },
            "results": results
        }]
    });

    serde_json::to_string_pretty(&sarif).context(format_context!("failed to serialize SARIF"))
}
