use crate::changes::glob;

fn insert_filter_glob(globs: &mut glob::Globs, is_include: bool, pattern: String) {
    if is_include {
        globs.includes.insert(pattern.into());
    } else {
        globs.excludes.insert(pattern.into());
    }
}

/// Build filter glob expressions from a `--filter` value.
/// Mirrors the expansion logic in `arguments.rs`.
pub fn build_filter_globs(filter: &str) -> glob::Globs {
    let mut globs = glob::Globs::default();
    for expr in filter.split(',') {
        let expr = expr.trim();
        if expr.is_empty() {
            continue;
        }

        let expr = expr.strip_prefix("//").unwrap_or(expr);

        let expanded: Vec<(bool, String)> = if let Some(pattern) = expr.strip_prefix('+') {
            vec![(true, pattern.to_string())]
        } else if let Some(pattern) = expr.strip_prefix('-') {
            vec![(false, pattern.to_string())]
        } else if expr.contains('*') {
            vec![(true, expr.to_string())]
        } else {
            vec![
                (true, expr.to_string()),
                (true, format!("**/*:*{expr}*")),
                (true, format!("**/*{expr}*:*")),
                (true, format!("**/{expr}*:*")),
                (true, format!("**/*{expr}*/*:*")),
            ]
        };

        for (is_include, e) in expanded {
            let normalized = e.strip_prefix("//").unwrap_or(e.as_str()).to_string();

            // Label ergonomics: `//pkg/**` is commonly expected to include
            // both subtree labels (`//pkg/sub:target`) and package-local labels
            // (`//pkg:target`). Add `pkg:*` alongside `pkg/**`.
            if let Some(pkg) = normalized.strip_suffix("/**")
                && !pkg.is_empty()
            {
                insert_filter_glob(&mut globs, is_include, format!("{pkg}:*"));
            }

            insert_filter_glob(&mut globs, is_include, normalized);
        }
    }
    globs
}

/// Score match quality with exact/prefix/substring bonuses, then fuzzy fallback.
pub fn score_match(query_term: &str, target: &str) -> isize {
    let query_lower = query_term.to_lowercase();
    let target_lower = target.to_lowercase();

    // Check for exact case-sensitive match (best)
    if target == query_term {
        return 15000;
    }

    // Check for exact case-insensitive match
    if target_lower == query_lower {
        return 10000;
    }

    // Check for prefix match
    if target_lower.starts_with(&query_lower) {
        return 5000;
    }

    // Check for word boundary match (e.g., "test" matches "build-test", "pkg:test")
    if target_lower.contains(&format!("-{}", query_lower))
        || target_lower.contains(&format!("_{}", query_lower))
        || target_lower.contains(&format!(":{}", query_lower))
    {
        return 3000;
    }

    // Check for substring match
    if target_lower.contains(&query_lower) {
        return 2000;
    }

    // Fall back to fuzzy match
    sublime_fuzzy::best_match(query_term, target)
        .map(|m| m.score())
        .unwrap_or(0)
}

/// Returns true if any provided field matches the filter globs.
///
/// Empty globs are treated as match-all.
pub fn matches_filter_in_any_field<'a>(
    globs: &glob::Globs,
    fields: impl IntoIterator<Item = &'a str>,
) -> bool {
    if globs.is_empty() {
        return true;
    }

    fields.into_iter().any(|field| {
        let normalized = field.strip_prefix("//").unwrap_or(field);
        globs.is_match(normalized)
    })
}

#[cfg(test)]
mod tests {
    use super::{build_filter_globs, matches_filter_in_any_field, score_match};

    #[test]
    fn filter_glob_normalizes_label_style_include_prefix() {
        let globs = build_filter_globs("//spaces/**");
        assert!(globs.includes.contains("spaces/**"));
        assert!(globs.includes.contains("spaces:*"));
        assert!(!globs.includes.contains("//spaces/**"));
    }

    #[test]
    fn filter_glob_fuzzy_expansion_strips_label_prefix() {
        let globs = build_filter_globs("//pkg:target");
        assert!(globs.includes.contains("pkg:target"));
        assert!(globs.includes.contains("**/*:*pkg:target*"));
        assert!(globs.includes.contains("**/*pkg:target*:*"));
        assert!(globs.includes.contains("**/pkg:target*:*"));
        assert!(globs.includes.contains("**/*pkg:target*/*:*"));
        assert!(!globs.includes.contains("**/*:*//pkg:target*"));
    }

    #[test]
    fn filter_glob_exact_label_term_is_included() {
        let globs = build_filter_globs("spaces:check");
        assert!(globs.includes.contains("spaces:check"));
    }

    #[test]
    fn filter_glob_exact_label_term_with_leading_slashes_is_included() {
        let globs = build_filter_globs("//spaces:check");
        assert!(globs.includes.contains("spaces:check"));
        assert!(!globs.includes.contains("//spaces:check"));
    }

    #[test]
    fn filter_glob_normalizes_label_style_annotated_prefixes() {
        let globs = build_filter_globs("+//spaces/**,-//spaces/**:*test*");
        assert!(globs.includes.contains("spaces/**"));
        assert!(globs.includes.contains("spaces:*"));
        assert!(globs.excludes.contains("spaces/**:*test*"));
        assert!(!globs.includes.contains("//spaces/**"));
        assert!(!globs.excludes.contains("//spaces/**:*test*"));
    }

    #[test]
    fn matches_filter_checks_any_field() {
        let globs = build_filter_globs("pkg:build");
        assert!(matches_filter_in_any_field(
            &globs,
            ["//foo:noop", "//pkg:build", "other"]
        ));
        assert!(!matches_filter_in_any_field(
            &globs,
            ["//foo:noop", "//bar:test"]
        ));
    }

    #[test]
    fn score_match_prefers_exact_over_substring() {
        let exact = score_match("build", "build");
        let substring = score_match("build", "foo-build");
        assert!(exact > substring);
    }
}
