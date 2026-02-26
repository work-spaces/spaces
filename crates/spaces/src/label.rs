use crate::{singleton, workspace};
use anyhow_source_location::format_error;
use std::sync::Arc;

pub fn get_source_from_label(label: &str) -> String {
    let (source, _rule_name) = label.split_once(":").unwrap_or(("", label));
    let source = source.strip_prefix("//").unwrap_or(source);
    let source_dot = format!("{source}.spaces.star");
    let source_slash = format!("{source}/spaces.star");
    if std::path::Path::new(&source_dot).exists() {
        source_dot
    } else if std::path::Path::new(&source_slash).exists() {
        source_slash
    } else {
        source.to_string()
    }
}

pub fn get_rule_name_from_label(label: &str) -> &str {
    let (_path, rule_name) = label.split_once(":").unwrap_or(("", label));
    rule_name
}

pub fn get_path_from_label(label: &str) -> &str {
    let (path, _rule_name) = label.split_once(":").unwrap_or(("", label));
    path
}

pub fn sanitize_rule_for_display(rule_name: Arc<str>) -> Arc<str> {
    // if length > MAX_RULE_NAME_LENGTH show firtst INTRO_LENGTH chars, then ... then the rest
    const MAX_RULE_NAME_LENGTH: usize = 64;
    const INTRO_LENGTH: usize = 16;
    const RULE_NAME_REST: usize = MAX_RULE_NAME_LENGTH - INTRO_LENGTH;
    if rule_name.len() < MAX_RULE_NAME_LENGTH {
        return rule_name;
    }

    let mut result = String::new();
    for (i, c) in rule_name.chars().enumerate() {
        if i < INTRO_LENGTH {
            result.push(c);
        } else if i == INTRO_LENGTH {
            result.push_str("...");
        } else if i > rule_name.len() - RULE_NAME_REST {
            result.push(c);
        }
    }
    result.into()
}

pub fn sanitize_rule(rule_name: Arc<str>, starlark_module: Option<Arc<str>>) -> Arc<str> {
    if is_rule_sanitized(rule_name.as_ref()) {
        return rule_name;
    }

    if let Some(latest_module) = starlark_module {
        let rule_name = rule_name.strip_prefix(':').unwrap_or(rule_name.as_ref());
        let slash_suffix = format!("/{}", workspace::SPACES_MODULE_NAME);
        let dot_suffix = format!(".{}", workspace::SPACES_MODULE_NAME);

        let rule_prefix = latest_module
            .strip_suffix(slash_suffix.as_str())
            .or_else(|| latest_module.strip_suffix(dot_suffix.as_str()))
            .unwrap_or("");

        let separator = if rule_name.contains(':') { '/' } else { ':' };
        format!("//{rule_prefix}{separator}{rule_name}").into()
    } else {
        rule_name
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum IsAnnotated {
    No,
    Yes,
}

pub fn sanitize_glob_value(
    value: &str,
    is_annotated: IsAnnotated,
    rule_name: &str,
    starlark_module: Option<Arc<str>>,
) -> anyhow::Result<Arc<str>> {
    let module = starlark_module.unwrap_or("unknown".into());
    if is_annotated == IsAnnotated::Yes {
        if value.starts_with("+//**") {
            singleton::push_glob_warning(
                format!(
                    "{module}:{rule_name} inputs -> {value} globbing the workspace root is bad for performance"
                )
                .into()
            );
        }

        if value.starts_with("+//") || value.starts_with("-//") {
            return Ok(value.replace("+//", "+").replace("-//", "-").into());
        }

        return Err(format_error!(
            "{}:{} inputs -> {} must begin with +// or -// and be a workspace root path",
            module,
            rule_name,
            value
        ));
    } else {
        if value.starts_with("//**") {
            singleton::push_glob_warning(
            format!(
                "{module}:{rule_name} inputs -> {value} globbing the workspace root is bad for performance"
            )
            .into()
        );
        }

        if value.starts_with("//") {
            return Ok(value.replace("//", "").into());
        }
    }

    Err(format_error!(
        "{}:{} inputs Includes/Excludes -> {} must begin with // and be a workspace root path",
        module,
        rule_name,
        value
    ))
}

pub fn sanitize_working_directory(
    working_directory: Arc<str>,
    starlark_module: Option<Arc<str>>,
) -> Arc<str> {
    if working_directory.starts_with("//") {
        return working_directory;
    }
    if let Some(latest_module) = starlark_module {
        let module_path = std::path::Path::new(latest_module.as_ref());
        let path_prefix = module_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let separator = if path_prefix.is_empty() { "" } else { "/" };
        format!("//{path_prefix}{separator}{working_directory}").into()
    } else {
        // The latest module is always available, this path is unreachable
        // unless there is a bug
        working_directory
    }
}

pub fn is_rule_sanitized(rule_name: &str) -> bool {
    rule_name.starts_with("//")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_is_rule_sanitized() {
        // Starts with "//" → sanitized
        assert!(is_rule_sanitized("//some/path:rule"));
        assert!(is_rule_sanitized("//"));
        assert!(!is_rule_sanitized("/some/path:rule"));
        assert!(!is_rule_sanitized("some/path:rule"));
        assert!(!is_rule_sanitized(""));
    }

    #[test]
    fn test_get_rule_name_from_label() {
        assert_eq!(get_rule_name_from_label("//path/to/pkg:my_rule"), "my_rule");
        assert_eq!(get_rule_name_from_label("my_rule"), "my_rule");
        assert_eq!(get_rule_name_from_label(""), "");
        assert_eq!(get_rule_name_from_label(":"), "");
        // this label is malformed but allowed by this function
        assert_eq!(
            get_rule_name_from_label("//path:nested:rule"),
            "nested:rule"
        );
    }

    #[test]
    fn test_get_path_from_label() {
        assert_eq!(
            get_path_from_label("//path/to/pkg:my_rule"),
            "//path/to/pkg"
        );
        // these are malformed but allowed by this function
        assert_eq!(get_path_from_label("my_rule"), "");
        assert_eq!(get_path_from_label(""), "");
        assert_eq!(get_path_from_label(":"), "");
        assert_eq!(get_path_from_label("//path:nested:rule"), "//path");
    }

    #[test]
    fn test_get_source_from_label() {
        // Strips "//" prefix and returns source when no file exists on disk
        assert_eq!(
            get_source_from_label("//nonexistent/path:rule"),
            "nonexistent/path"
        );

        // No colon → source portion is empty
        assert_eq!(get_source_from_label("my_rule"), "");

        // No "//" prefix → source returned as-is (no file on disk)
        assert_eq!(get_source_from_label("plain/path:rule"), "plain/path");

        // Falls through to raw source when neither file variant exists
        assert_eq!(
            get_source_from_label("//does/not/exist:rule"),
            "does/not/exist"
        );

        // Prefers {source}.spaces.star when both file variants exist
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("mymod");
        let dot_file = format!("{}.spaces.star", base.display());
        std::fs::write(&dot_file, "").unwrap();
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("spaces.star"), "").unwrap();
        let label = format!("//{}:rule", base.display());
        assert_eq!(get_source_from_label(&label), dot_file);

        // Falls back to {source}/spaces.star when only that variant exists
        let dir2 = tempfile::tempdir().unwrap();
        let base2 = dir2.path().join("mymod2");
        std::fs::create_dir_all(&base2).unwrap();
        std::fs::write(base2.join("spaces.star"), "").unwrap();
        let label2 = format!("//{}:rule", base2.display());
        assert_eq!(
            get_source_from_label(&label2),
            format!("{}/spaces.star", base2.display())
        );
    }

    #[test]
    fn test_sanitize_rule_for_display() {
        // Empty name returned as-is (same Arc)
        let empty: Arc<str> = "".into();
        assert!(Arc::ptr_eq(
            &empty,
            &sanitize_rule_for_display(empty.clone())
        ));

        // Short name (< 64 chars) returned as-is (same Arc, no allocation)
        let short: Arc<str> = "short_rule".into();
        let short_result = sanitize_rule_for_display(short.clone());
        assert_eq!(short_result.as_ref(), "short_rule");
        assert!(Arc::ptr_eq(&short, &short_result));

        // Exactly 63 chars → still below threshold, returned as-is
        let name_63: Arc<str> = "a".repeat(63).into();
        let result_63 = sanitize_rule_for_display(name_63.clone());
        assert_eq!(result_63.len(), 63);
        assert!(Arc::ptr_eq(&name_63, &result_63));

        // 64 chars → triggers truncation with "..."
        let name_64: Arc<str> = "a".repeat(64).into();
        let result_64 = sanitize_rule_for_display(name_64);
        assert!(result_64.contains("..."));
        assert!(result_64.starts_with(&"a".repeat(16)));
        assert!(result_64.len() < 64 + 3);

        // Long name with distinct intro/tail chars preserves structure
        // 80 chars: 16 'A' then 64 'B'
        let long: Arc<str> = format!("{}{}", "A".repeat(16), "B".repeat(64)).into();
        let long_result = sanitize_rule_for_display(long);
        assert!(long_result.starts_with(&"A".repeat(16)));
        assert!(long_result.contains("..."));
        assert!(long_result.ends_with('B'));
    }

    #[test]
    fn test_sanitize_rule() {
        // Already sanitized (starts with "//") → returned as same Arc
        let sanitized: Arc<str> = "//already/sanitized:rule".into();
        assert!(Arc::ptr_eq(
            &sanitized,
            &sanitize_rule(sanitized.clone(), Some("module/spaces.star".into()))
        ));

        // None module → returned as-is
        let unsanitized: Arc<str> = "unsanitized_rule".into();
        assert!(Arc::ptr_eq(
            &unsanitized,
            &sanitize_rule(unsanitized.clone(), None)
        ));

        // Module with /spaces.star suffix → prefix extracted, colon separator
        assert_eq!(
            sanitize_rule("my_rule".into(), Some("path/to/pkg/spaces.star".into())).as_ref(),
            "//path/to/pkg:my_rule"
        );

        // Module with .spaces.star suffix → prefix extracted, colon separator
        assert_eq!(
            sanitize_rule("my_rule".into(), Some("path/to/pkg.spaces.star".into())).as_ref(),
            "//path/to/pkg:my_rule"
        );

        // Leading colon on rule name is stripped before formatting
        assert_eq!(
            sanitize_rule(":my_rule".into(), Some("path/to/pkg/spaces.star".into())).as_ref(),
            "//path/to/pkg:my_rule"
        );

        // Rule containing colon → uses '/' separator instead of ':'
        assert_eq!(
            sanitize_rule("nested:rule".into(), Some("path/to/pkg/spaces.star".into())).as_ref(),
            "//path/to/pkg/nested:rule"
        );

        // Module without a recognized suffix → empty prefix
        assert_eq!(
            sanitize_rule("my_rule".into(), Some("no_suffix_match.star".into())).as_ref(),
            "//:my_rule"
        );

        // Module that is just "spaces.star" → neither suffix matches, empty prefix
        assert_eq!(
            sanitize_rule("my_rule".into(), Some("spaces.star".into())).as_ref(),
            "//:my_rule"
        );
    }

    #[test]
    fn test_sanitize_glob_value() {
        // "+//" prefix is replaced with "+"
        assert_eq!(
            sanitize_glob_value(
                "+//some/path/*.rs",
                IsAnnotated::Yes,
                "rule",
                Some("module.star".into())
            )
            .unwrap()
            .as_ref(),
            "+some/path/*.rs"
        );

        assert_eq!(
            sanitize_glob_value(
                "//some/path/*.rs",
                IsAnnotated::No,
                "rule",
                Some("module.star".into())
            )
            .unwrap()
            .as_ref(),
            "some/path/*.rs"
        );

        // "-//" prefix is replaced with "-"
        assert_eq!(
            sanitize_glob_value(
                "-//some/path/*.rs",
                IsAnnotated::Yes,
                "rule",
                Some("module.star".into())
            )
            .unwrap()
            .as_ref(),
            "-some/path/*.rs"
        );

        // "+//**" triggers a performance warning but still succeeds
        assert_eq!(
            sanitize_glob_value(
                "+//**/*.rs",
                IsAnnotated::Yes,
                "rule",
                Some("module.star".into())
            )
            .unwrap()
            .as_ref(),
            "+**/*.rs"
        );

        // Invalid prefixes → error
        assert!(
            sanitize_glob_value(
                "some/path/*.rs",
                IsAnnotated::Yes,
                "rule",
                Some("module.star".into())
            )
            .is_err()
        );
        assert!(
            sanitize_glob_value(
                "+/some/path",
                IsAnnotated::Yes,
                "rule",
                Some("module.star".into())
            )
            .is_err()
        );
        assert!(
            sanitize_glob_value(
                "-/some/path",
                IsAnnotated::Yes,
                "rule",
                Some("module.star".into())
            )
            .is_err()
        );

        // None module → error message contains "unknown"
        let err_msg = sanitize_glob_value("bad_value", IsAnnotated::Yes, "rule", None)
            .unwrap_err()
            .to_string();
        assert!(err_msg.contains("unknown"));
    }

    #[test]
    fn test_sanitize_working_directory() {
        // Already starts with "//" → returned as same Arc
        let absolute: Arc<str> = "//already/absolute".into();
        assert!(Arc::ptr_eq(
            &absolute,
            &sanitize_working_directory(absolute.clone(), Some("module/spaces.star".into()))
        ));

        // Relative dir + module → parent path of module is prepended
        assert_eq!(
            sanitize_working_directory("subdir".into(), Some("path/to/pkg/spaces.star".into()))
                .as_ref(),
            "//path/to/pkg/subdir"
        );

        // Deeply nested module path
        assert_eq!(
            sanitize_working_directory("build".into(), Some("a/b/c/d/spaces.star".into())).as_ref(),
            "//a/b/c/d/build"
        );

        // Module in root (no parent directory) → no extra separator
        assert_eq!(
            sanitize_working_directory("subdir".into(), Some("spaces.star".into())).as_ref(),
            "//subdir"
        );

        // Empty working directory with module → trailing slash from separator
        assert_eq!(
            sanitize_working_directory("".into(), Some("path/to/spaces.star".into())).as_ref(),
            "//path/to/"
        );

        // None module → returned as-is
        let relative: Arc<str> = "relative/dir".into();
        assert!(Arc::ptr_eq(
            &relative,
            &sanitize_working_directory(relative.clone(), None)
        ));
    }
}
