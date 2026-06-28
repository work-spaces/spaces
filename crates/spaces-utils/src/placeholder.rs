pub fn format_placeholder(marker: &str, name: &str) -> String {
    format!("{marker}{{{name}}}")
}

pub fn expand_placeholders<T, Resolve, UnclosedError>(
    value: &str,
    marker: &str,
    mut resolve: Resolve,
    mut unclosed_error: UnclosedError,
) -> anyhow::Result<String>
where
    T: AsRef<str>,
    Resolve: FnMut(&str) -> anyhow::Result<T>,
    UnclosedError: FnMut() -> anyhow::Error,
{
    let mut result = String::new();
    let mut remaining = value;
    let marker_open = format!("{marker}{{");

    while let Some(start) = remaining.find(&marker_open) {
        result.push_str(&remaining[..start]);
        let after = &remaining[start + marker_open.len()..];
        let end = after.find('}').ok_or_else(&mut unclosed_error)?;
        let name = &after[..end];
        let replacement = resolve(name)?;
        result.push_str(replacement.as_ref());
        remaining = &after[end + 1..];
    }

    result.push_str(remaining);
    Ok(result)
}

pub fn replace_values_with_placeholders<'a, I>(value: &str, marker: &str, values: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut result = value.to_string();
    for (name, raw_value) in values {
        if !raw_value.is_empty() {
            let placeholder = format_placeholder(marker, name);
            result = result.replace(raw_value, &placeholder);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_placeholder_builds_marker_token() {
        assert_eq!(format_placeholder("$FOO", "BAR"), "$FOO{BAR}");
    }

    #[test]
    fn expand_placeholders_replaces_multiple_tokens() {
        let expanded = expand_placeholders(
            "x $FOO{a} y $FOO{b}",
            "$FOO",
            |name| -> anyhow::Result<String> { Ok(format!("<{name}>")) },
            || anyhow::anyhow!("unclosed token"),
        )
        .unwrap();

        assert_eq!(expanded, "x <a> y <b>");
    }

    #[test]
    fn expand_placeholders_errors_on_unclosed_token() {
        let err = expand_placeholders::<String, _, _>(
            "value $FOO{oops",
            "$FOO",
            |_| Ok("x".to_string()),
            || anyhow::anyhow!("unclosed token"),
        )
        .unwrap_err();

        assert!(err.to_string().contains("unclosed token"));
    }

    #[test]
    fn replace_values_with_placeholders_skips_empty_values() {
        let replaced = replace_values_with_placeholders(
            "abc/path/def",
            "$AUTO",
            [("PATH", "/path"), ("EMPTY", "")],
        );

        assert_eq!(replaced, "abc$AUTO{PATH}/def");
    }
}
