use anyhow::Context;
use anyhow::anyhow;
use anyhow_source_location::format_context;
use regex::Regex;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneOr;
use starlark::values::{Heap, Value};
use std::cmp::min;
use std::collections::{BTreeMap, BTreeSet};

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn trim(s: &str) -> anyhow::Result<String> {
        Ok(s.trim().to_string())
    }

    fn trim_start(s: &str) -> anyhow::Result<String> {
        Ok(s.trim_start().to_string())
    }

    fn trim_end(s: &str) -> anyhow::Result<String> {
        Ok(s.trim_end().to_string())
    }

    fn split_whitespace(s: &str) -> anyhow::Result<Vec<String>> {
        Ok(s.split_whitespace().map(ToString::to_string).collect())
    }

    fn split_lines(s: &str) -> anyhow::Result<Vec<String>> {
        let mut out = Vec::new();
        let bytes = s.as_bytes();
        let mut start = 0usize;
        let mut i = 0usize;

        while i < bytes.len() {
            if bytes[i] == b'\n' {
                let end = if i > start && bytes[i - 1] == b'\r' {
                    i - 1
                } else {
                    i
                };
                out.push(s[start..end].to_string());
                i += 1;
                start = i;
            } else {
                i += 1;
            }
        }

        if start < s.len() {
            out.push(s[start..].to_string());
        }

        Ok(out)
    }

    fn contains(s: &str, needle: &str, ignore_case: Option<bool>) -> anyhow::Result<bool> {
        if ignore_case.unwrap_or(false) {
            Ok(s.to_lowercase().contains(&needle.to_lowercase()))
        } else {
            Ok(s.contains(needle))
        }
    }

    fn starts_with(s: &str, prefix: &str) -> anyhow::Result<bool> {
        Ok(s.starts_with(prefix))
    }

    fn ends_with(s: &str, suffix: &str) -> anyhow::Result<bool> {
        Ok(s.ends_with(suffix))
    }

    fn replace(
        s: &str,
        from: &str,
        to: &str,
        count: Option<i32>,
        regex: Option<bool>,
        ignore_case: Option<bool>,
    ) -> anyhow::Result<String> {
        let use_regex = regex.unwrap_or(false);
        let ic = ignore_case.unwrap_or(false);
        let n = count.unwrap_or(-1);

        if n == 0 {
            return Ok(s.to_string());
        }

        if use_regex {
            let pattern = if ic {
                format!("(?i){from}")
            } else {
                from.to_string()
            };
            let re = Regex::new(&pattern).context(format_context!("invalid regex pattern"))?;
            if n < 0 {
                Ok(re.replace_all(s, to).to_string())
            } else {
                Ok(re.replacen(s, n as usize, to).to_string())
            }
        } else if ic {
            replace_case_insensitive(s, from, to, n)
        } else if n < 0 {
            Ok(s.replace(from, to))
        } else {
            Ok(s.replacen(from, to, n as usize))
        }
    }

    fn regex_match<'v>(
        pattern: &str,
        s: &str,
        eval: &mut starlark::eval::Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneOr<Value<'v>>> {
        let re = Regex::new(pattern).context(format_context!("invalid regex pattern"))?;
        if let Some(caps) = re.captures(s) {
            let heap = eval.heap();
            let map = build_match_map(heap, &re, s, &caps)?;
            Ok(NoneOr::Other(heap.alloc(map)))
        } else {
            Ok(NoneOr::None)
        }
    }

    fn regex_find_all<'v>(
        pattern: &str,
        s: &str,
        eval: &mut starlark::eval::Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        let re = Regex::new(pattern).context(format_context!("invalid regex pattern"))?;
        let mut out = Vec::new();
        for caps in re.captures_iter(s) {
            let heap = eval.heap();
            let map = build_match_map(heap, &re, s, &caps)?;
            out.push(heap.alloc(map));
        }
        Ok(eval.heap().alloc(out))
    }

    fn regex_captures<'v>(
        pattern: &str,
        s: &str,
        eval: &mut starlark::eval::Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneOr<Value<'v>>> {
        let re = Regex::new(pattern).context(format_context!("invalid regex pattern"))?;
        if let Some(caps) = re.captures(s) {
            let mut named = BTreeMap::<String, String>::new();
            for name in re.capture_names().flatten() {
                if let Some(m) = caps.name(name) {
                    named.insert(name.to_string(), m.as_str().to_string());
                }
            }
            Ok(NoneOr::Other(eval.heap().alloc(named)))
        } else {
            Ok(NoneOr::None)
        }
    }

    fn to_upper(s: &str) -> anyhow::Result<String> {
        Ok(s.to_uppercase())
    }

    fn to_lower(s: &str) -> anyhow::Result<String> {
        Ok(s.to_lowercase())
    }

    fn title_case(s: &str) -> anyhow::Result<String> {
        let words = split_words(s);
        let out = words
            .into_iter()
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    Some(first) => {
                        let mut p = first.to_uppercase().collect::<String>();
                        p.push_str(&chars.flat_map(char::to_lowercase).collect::<String>());
                        p
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        Ok(out)
    }

    fn snake_case(s: &str) -> anyhow::Result<String> {
        Ok(split_words(s).join("_").to_lowercase())
    }

    fn kebab_case(s: &str) -> anyhow::Result<String> {
        Ok(split_words(s).join("-").to_lowercase())
    }

    fn camel_case(s: &str) -> anyhow::Result<String> {
        let words = split_words(s);
        if words.is_empty() {
            return Ok(String::new());
        }
        let mut out = String::new();
        out.push_str(&words[0].to_lowercase());
        for w in words.iter().skip(1) {
            let mut chars = w.chars();
            if let Some(first) = chars.next() {
                out.push_str(&first.to_uppercase().collect::<String>());
                out.push_str(&chars.flat_map(char::to_lowercase).collect::<String>());
            }
        }
        Ok(out)
    }

    fn pad_left(s: &str, n: i32, fill: Option<&str>) -> anyhow::Result<String> {
        pad_impl(s, n, fill.unwrap_or(" "), true)
    }

    fn pad_right(s: &str, n: i32, fill: Option<&str>) -> anyhow::Result<String> {
        pad_impl(s, n, fill.unwrap_or(" "), false)
    }

    fn format_table<'v>(
        rows: starlark::values::list::UnpackList<Value<'v>>,
    ) -> anyhow::Result<String> {
        if rows.items.is_empty() {
            return Ok(String::new());
        }

        let mut columns = BTreeSet::<String>::new();
        let mut materialized = Vec::<BTreeMap<String, String>>::new();

        for row in rows.items {
            let json = row
                .to_json_value()
                .context(format_context!("failed to convert row"))?;
            let obj = json
                .as_object()
                .ok_or_else(|| anyhow!(format_context!("each row must be a dict/object")))?;

            let mut map = BTreeMap::<String, String>::new();
            for (k, v) in obj {
                columns.insert(k.clone());
                map.insert(k.clone(), json_cell_to_string(v));
            }
            materialized.push(map);
        }

        let headers = columns.into_iter().collect::<Vec<_>>();
        let mut widths = headers
            .iter()
            .map(|h| h.chars().count())
            .collect::<Vec<_>>();

        for row in &materialized {
            for (i, h) in headers.iter().enumerate() {
                let val = row.get(h).cloned().unwrap_or_default();
                widths[i] = widths[i].max(val.chars().count());
            }
        }

        let sep = format!(
            "+{}+",
            widths
                .iter()
                .map(|w| "-".repeat(*w + 2))
                .collect::<Vec<_>>()
                .join("+")
        );

        let mut out = String::new();
        out.push_str(&sep);
        out.push('\n');

        out.push('|');
        for (i, h) in headers.iter().enumerate() {
            out.push(' ');
            out.push_str(&pad_display(h, widths[i], false));
            out.push(' ');
            out.push('|');
        }
        out.push('\n');
        out.push_str(&sep);
        out.push('\n');

        for row in &materialized {
            out.push('|');
            for (i, h) in headers.iter().enumerate() {
                let v = row.get(h).cloned().unwrap_or_default();
                out.push(' ');
                out.push_str(&pad_display(&v, widths[i], false));
                out.push(' ');
                out.push('|');
            }
            out.push('\n');
        }

        out.push_str(&sep);
        Ok(out)
    }
}

fn replace_case_insensitive(s: &str, from: &str, to: &str, n: i32) -> anyhow::Result<String> {
    if from.is_empty() {
        return Ok(s.to_string());
    }
    // Escape `from` so it is treated as a literal string, then prepend (?i)
    // for Unicode-aware case-insensitive matching via the regex engine.
    // This avoids the byte-offset mismatch that arises when to_lowercase()
    // changes the byte length of non-ASCII characters (e.g. Turkish İ → i + ◌̇).
    let escaped = regex::escape(from);
    let pattern = format!("(?i){escaped}");
    let re = Regex::new(&pattern).context(format_context!(
        "invalid pattern for case-insensitive replace"
    ))?;
    if n < 0 {
        Ok(re.replace_all(s, to).to_string())
    } else {
        Ok(re.replacen(s, n as usize, to).to_string())
    }
}

fn split_words(s: &str) -> Vec<String> {
    let mut words = Vec::<String>::new();
    let mut current = String::new();
    let mut prev_is_lower_or_digit = false;

    for ch in s.chars() {
        if ch == '_' || ch == '-' || ch.is_whitespace() {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
            prev_is_lower_or_digit = false;
            continue;
        }

        if ch.is_uppercase() && prev_is_lower_or_digit && !current.is_empty() {
            words.push(current.clone());
            current.clear();
        }

        current.push(ch);
        prev_is_lower_or_digit = ch.is_lowercase() || ch.is_ascii_digit();
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

fn pad_impl(s: &str, n: i32, fill: &str, left: bool) -> anyhow::Result<String> {
    if n <= 0 {
        return Ok(s.to_string());
    }
    if fill.is_empty() {
        return Err(anyhow!(format_context!("fill must not be empty")));
    }

    let width = n as usize;
    let len = s.chars().count();
    if len >= width {
        return Ok(s.to_string());
    }

    let needed = width - len;
    let pad = repeat_to_len(fill, needed);

    if left {
        Ok(format!("{pad}{s}"))
    } else {
        Ok(format!("{s}{pad}"))
    }
}

fn repeat_to_len(fill: &str, target_chars: usize) -> String {
    let mut out = String::new();
    let fill_chars = fill.chars().collect::<Vec<_>>();
    if fill_chars.is_empty() || target_chars == 0 {
        return out;
    }

    let mut i = 0usize;
    while out.chars().count() < target_chars {
        out.push(fill_chars[i % fill_chars.len()]);
        i += 1;
    }

    if out.chars().count() > target_chars {
        let mut trimmed = String::new();
        for ch in out.chars().take(target_chars) {
            trimmed.push(ch);
        }
        trimmed
    } else {
        out
    }
}

fn build_match_map<'v>(
    heap: Heap<'v>,
    re: &Regex,
    source: &str,
    caps: &regex::Captures<'_>,
) -> anyhow::Result<BTreeMap<String, Value<'v>>> {
    let mut out = BTreeMap::<String, Value<'v>>::new();

    if let Some(m) = caps.get(0) {
        out.insert("match".to_string(), heap.alloc(m.as_str().to_string()));
        // Convert byte offsets (from the regex crate) to Unicode scalar (char) offsets
        // so callers can use start/end directly as character-level indices.
        let char_start = source[..m.start()].chars().count();
        let char_end = source[..m.end()].chars().count();
        out.insert("start".to_string(), heap.alloc(char_start as i32));
        out.insert("end".to_string(), heap.alloc(char_end as i32));
    }

    let groups = (1..caps.len())
        .map(|i| {
            caps.get(i)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    out.insert("groups".to_string(), heap.alloc(groups));

    let mut named = BTreeMap::<String, String>::new();
    for name in re.capture_names().flatten() {
        if let Some(m) = caps.name(name) {
            named.insert(name.to_string(), m.as_str().to_string());
        }
    }
    out.insert("named".to_string(), heap.alloc(named));
    out.insert("source".to_string(), heap.alloc(source.to_string()));

    Ok(out)
}

fn json_cell_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}

fn pad_display(s: &str, width: usize, left: bool) -> String {
    let len = s.chars().count();
    if len >= width {
        return s.to_string();
    }
    let pad = " ".repeat(min(width - len, width));
    if left {
        format!("{pad}{s}")
    } else {
        format!("{s}{pad}")
    }
}
