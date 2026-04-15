/// Extracts the inner values from all occurrences of `{marker}{value}` in `input`.
///
/// For example, given marker `"$FOO"` and input `"hello $FOO{bar} and $FOO{baz}"`,
/// this returns `["bar", "baz"]`.
pub fn extract_marker_values(input: &str, marker: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut remaining = input;
    let open = format!("{marker}{{");
    while let Some(start) = remaining.find(&open) {
        let after = &remaining[start + open.len()..];
        if let Some(end) = after.find('}') {
            values.push(after[..end].to_string());
            remaining = &after[end + 1..];
        } else {
            break;
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_markers() {
        assert!(extract_marker_values("hello world", "$M").is_empty());
    }

    #[test]
    fn test_single_marker() {
        assert_eq!(
            extract_marker_values("$M{foo}", "$M"),
            vec!["foo".to_string()]
        );
    }

    #[test]
    fn test_multiple_markers() {
        assert_eq!(
            extract_marker_values("$M{foo} text $M{bar}", "$M"),
            vec!["foo".to_string(), "bar".to_string()]
        );
    }

    #[test]
    fn test_unclosed_marker_stops_early() {
        assert_eq!(
            extract_marker_values("$M{foo} $M{unclosed", "$M"),
            vec!["foo".to_string()]
        );
    }
}
