use std::collections::HashSet;
use std::sync::Arc;

pub fn is_glob_include(glob: &str) -> Option<Arc<str>> {
    let mut result = glob.to_owned();
    if let Some(first_char) = result.chars().next() {
        if first_char == '-' {
            return None;
        }
        let char_len = first_char.len_utf8();
        result.drain(..char_len);
    }
    Some(result.into())
}

pub fn match_globs(globs: &HashSet<Arc<str>>, input: &str) -> bool {
    let includes = globs.iter().filter(|g| g.starts_with('+'));
    let excludes = globs.iter().filter(|g| g.starts_with('-'));

    for include in includes {
        let include_pattern = include.strip_prefix('+').unwrap_or(include.as_ref());
        if glob_match::glob_match(include_pattern, input) {
            for exclude in excludes {
                let exclude_pattern = exclude.strip_prefix('-').unwrap_or(exclude.as_ref());
                if glob_match::glob_match(exclude_pattern, input) {
                    return false;
                }
            }
            return true;
        }
    }
    
    false
}