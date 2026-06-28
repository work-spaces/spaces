use crate::changes::glob;
use std::sync::Arc;

fn insert_filter_glob(globs: &mut glob::Globs, pattern: String) {
    globs.includes.insert(pattern.into());
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

        let expanded: Vec<String> =
            if let Some(pattern) = expr.strip_prefix('+').or_else(|| expr.strip_prefix('-')) {
                vec![pattern.to_string()]
            } else if expr.contains('*') {
                vec![expr.to_string()]
            } else {
                vec![
                    expr.to_string(),
                    format!("**/*:*{expr}*"),
                    format!("**/*{expr}*:*"),
                    format!("**/{expr}*:*"),
                    format!("**/*{expr}*/*:*"),
                ]
            };

        for e in expanded {
            let normalized = e.strip_prefix("//").unwrap_or(e.as_str()).to_string();

            // Label ergonomics: `//pkg/**` is commonly expected to include
            // both subtree labels (`//pkg/sub:target`) and package-local labels
            // (`//pkg:target`). Add `pkg:*` alongside `pkg/**`.
            if let Some(pkg) = normalized.strip_suffix("/**")
                && !pkg.is_empty()
            {
                insert_filter_glob(&mut globs, format!("{pkg}:*"));
            }

            insert_filter_glob(&mut globs, normalized);
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

/// Build a char-level highlight mask for `value` based on case-insensitive
/// substring matches for each keyword.
pub fn keyword_highlight_mask(value: &str, keywords: &[Arc<str>]) -> Vec<bool> {
    // ASCII-only folding preserves char count, so value_lower.len() == value.chars().count().
    let value_lower: Vec<char> = value.to_ascii_lowercase().chars().collect();
    let mut highlights = vec![false; value_lower.len()];

    for term in keywords {
        let term_lower: Vec<char> = term.as_ref().to_ascii_lowercase().chars().collect();
        if term_lower.is_empty() || term_lower.len() > value_lower.len() {
            continue;
        }

        for start in 0..=value_lower.len() - term_lower.len() {
            if value_lower[start..start + term_lower.len()] == term_lower[..] {
                for h in &mut highlights[start..start + term_lower.len()] {
                    *h = true;
                }
            }
        }
    }

    highlights
}

/// Split text into `(chunk, is_highlighted)` segments for rendering.
pub fn highlight_chunks(value: &str, highlight_terms: Option<&[Arc<str>]>) -> Vec<(String, bool)> {
    let Some(highlight_terms) = highlight_terms.filter(|terms| !terms.is_empty()) else {
        return vec![(value.to_owned(), false)];
    };

    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return vec![(String::new(), false)];
    }

    let highlights = keyword_highlight_mask(value, highlight_terms);
    if !highlights.iter().any(|highlighted| *highlighted) {
        return vec![(value.to_owned(), false)];
    }

    let mut chunks = Vec::new();
    let mut current_highlighted = highlights[0];
    let mut chunk = String::new();

    for (ch, highlighted) in chars.into_iter().zip(highlights) {
        if highlighted != current_highlighted {
            chunks.push((std::mem::take(&mut chunk), current_highlighted));
            current_highlighted = highlighted;
        }
        chunk.push(ch);
    }

    chunks.push((chunk, current_highlighted));
    chunks
}

#[cfg(test)]
mod tests {
    use super::{
        build_filter_globs, highlight_chunks, keyword_highlight_mask, matches_filter_in_any_field,
        score_match,
    };
    use std::sync::Arc;

    fn arc_terms(terms: &[&str]) -> Vec<Arc<str>> {
        terms.iter().map(|term| Arc::<str>::from(*term)).collect()
    }

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
        assert!(globs.includes.contains("spaces/**:*test*"));
        assert!(!globs.includes.contains("//spaces/**"));
        assert!(!globs.includes.contains("//spaces/**:*test*"));
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

    #[test]
    fn keyword_highlight_only_matches_whole_search_terms() {
        let mask = keyword_highlight_mask("some search thing", &arc_terms(&["something"]));
        let highlighted: Vec<usize> = mask
            .into_iter()
            .enumerate()
            .filter_map(|(idx, highlighted)| highlighted.then_some(idx))
            .collect();

        assert!(highlighted.is_empty());
    }

    #[test]
    fn keyword_highlight_marks_substrings_and_multiple_occurrences() {
        let mask = keyword_highlight_mask("tested tests test", &arc_terms(&["test"]));
        let highlighted: Vec<usize> = mask
            .into_iter()
            .enumerate()
            .filter_map(|(idx, highlighted)| highlighted.then_some(idx))
            .collect();

        assert_eq!(highlighted, vec![0, 1, 2, 3, 7, 8, 9, 10, 13, 14, 15, 16]);
    }

    #[test]
    fn keyword_highlight_no_panic_on_length_changing_lowercase() {
        // 'ß'.to_lowercase() == "ss" (1 char expands to 2), which previously caused an
        // out-of-bounds panic because highlights was sized from the original char count
        // while value_lower used the expanded length.
        let mask = keyword_highlight_mask("Straße", &arc_terms(&["straße"]));
        assert_eq!(mask.len(), "Straße".chars().count());
        // "Straße".to_ascii_lowercase() == "straße", so the full string should match.
        assert!(mask.iter().all(|&h| h));
    }

    #[test]
    fn keyword_highlight_merges_matches_from_multiple_terms() {
        let mask = keyword_highlight_mask("build and test", &arc_terms(&["build", "test"]));
        let highlighted: Vec<usize> = mask
            .into_iter()
            .enumerate()
            .filter_map(|(idx, highlighted)| highlighted.then_some(idx))
            .collect();

        assert_eq!(highlighted, vec![0, 1, 2, 3, 4, 10, 11, 12, 13]);
    }

    #[test]
    fn highlight_chunks_splits_highlighted_and_plain_segments() {
        let chunks = highlight_chunks("build and test", Some(&arc_terms(&["build", "test"])));
        assert_eq!(
            chunks,
            vec![
                ("build".to_string(), true),
                (" and ".to_string(), false),
                ("test".to_string(), true),
            ]
        );
    }
}
