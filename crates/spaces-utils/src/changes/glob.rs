use anyhow_source_location::format_error;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct Globs {
    pub includes: HashSet<Arc<str>>,
    pub excludes: HashSet<Arc<str>>,
}

impl Globs {
    pub fn new_with_includes(includes: &HashSet<Arc<str>>) -> Self {
        Self {
            includes: includes.clone(),
            excludes: HashSet::new(),
        }
    }

    pub fn new_with_annotated_set(annotated_set: &HashSet<Arc<str>>) -> Self {
        let includes = annotated_set
            .iter()
            .filter_map(|g| g.strip_prefix('+').map(|e| e.into()))
            .collect();
        let excludes = annotated_set
            .iter()
            .filter_map(|g| g.strip_prefix('-').map(|e| e.into()))
            .collect();
        Self { includes, excludes }
    }

    pub fn merge(&mut self, other: &Self) {
        self.includes.extend(other.includes.clone());
        self.excludes.extend(other.excludes.clone());
    }

    pub fn is_empty(&self) -> bool {
        self.includes.is_empty()
    }

    pub fn is_match(&self, input: &str) -> bool {
        let input = input.strip_prefix("./").unwrap_or(input);
        for include in self.includes.iter() {
            let include_pattern = include.strip_prefix('+').unwrap_or(include.as_ref());
            if glob_match::glob_match(include_pattern, input) {
                for exclude_pattern in self.excludes.iter() {
                    if glob_match::glob_match(exclude_pattern, input) {
                        return false;
                    }
                }
                return true;
            }
        }
        false
    }

    fn walk_glob_dir(&self, glob_include_path: Arc<str>) -> Vec<walkdir::DirEntry> {
        walkdir::WalkDir::new(glob_include_path.as_ref())
            .into_iter()
            .filter_map(|entry| entry.ok())
            .collect()
    }

    pub fn collect_matches(&self) -> Vec<Arc<std::path::Path>> {
        let mut set = HashSet::new();
        for input in self.includes.iter() {
            let glob_include_path = get_glob_path(input.clone());
            let walk_dir = self.walk_glob_dir(glob_include_path);
            for entry in walk_dir.into_iter() {
                if entry.file_type().is_dir() {
                    continue;
                }
                set.insert(entry.into_path());
            }
        }

        let mut result = Vec::new();
        result.extend(set.into_iter().map(|e| e.into()));
        result.sort();
        result
    }
}

// This is used to limit globbing to a subset of the workspace
pub fn get_glob_path(input: Arc<str>) -> Arc<str> {
    if let Some(asterisk_position) = input.find('*') {
        let mut path = input.to_string();
        path.truncate(asterisk_position);
        if path.is_empty() {
            ".".into()
        } else {
            path.into()
        }
    } else {
        // no asterisk found, return input as-is
        input.clone()
    }
}

pub fn is_glob_include(glob: &str) -> Option<Arc<str>> {
    let mut result = glob.to_owned();
    if let Some(first_char) = result.chars().next() {
        if first_char == '-' {
            return None;
        }
        let char_len = first_char.len_utf8();
        result.drain(..char_len);
    }
    if result.is_empty() {
        result.push('.');
    }
    Some(result.into())
}

pub fn validate(globs: &HashSet<Arc<str>>) -> anyhow::Result<()> {
    let mut has_includes = false;
    for values in globs.iter() {
        let value = values.as_ref();
        if value.starts_with('+') || value.starts_with('-') {
            if value.starts_with('+') {
                has_includes = true;
            }
            continue;
        }
        return Err(format_error!(
            "invalid glob pattern: {value:?}. Must begin with '+' or '-'"
        ));
    }

    if !has_includes {
        return Err(format_error!(
            "if globs are specified, at least one must be an include (start with `+`)"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_names() {
        static INPUT_LIST: &[&str] = &[
            "//capsules:ftp.gnu.org-m4-m4-exp_relocate_bin",
            "//capsules:ftp.gnu.org-libidn-libidn2-exp_publish_archive",
            "//capsules:github.com-lz4-lz4-exp_cmake_configure",
            "//capsules:github.com-madler-zlib-exp_relocate_bin",
            "//capsules:github.com-ngtcp2-nghttp3-exp_relocate_bin",
            "//capsules:relocate",
            "//capsules:bin",
            "//:all",
            "//:setup",
            "//test/capsules/test1:configure",
            "//test/capsules/test1:build",
            "//test/capsules/test1:install",
            "//test/capsules/test1:install_bin",
        ];

        let globs =
            Globs::new_with_annotated_set(&vec!["+//**/*:*bin".into()].into_iter().collect());
        assert!(globs.is_match(INPUT_LIST[0]));
        assert!(!globs.is_match(INPUT_LIST[1]));
        assert!(!globs.is_match(INPUT_LIST[2]));
        assert!(globs.is_match(INPUT_LIST[3]));
        assert!(globs.is_match(INPUT_LIST[4]));
        assert!(!globs.is_match(INPUT_LIST[5]));
        assert!(!globs.is_match(INPUT_LIST[7]));
        assert!(!globs.is_match(INPUT_LIST[8]));
        assert!(!globs.is_match(INPUT_LIST[9]));
        assert!(!globs.is_match(INPUT_LIST[10]));
        assert!(!globs.is_match(INPUT_LIST[11]));
        assert!(globs.is_match(INPUT_LIST[12]));

        let globs = Globs::new_with_annotated_set(
            &vec!["+//**/*:*bin".into(), "-//**/*:*ftp.gnu.org*".into()]
                .into_iter()
                .collect(),
        );
        assert!(!globs.is_match(INPUT_LIST[0]));
        assert!(!globs.is_match(INPUT_LIST[1]));
        assert!(!globs.is_match(INPUT_LIST[2]));
        assert!(globs.is_match(INPUT_LIST[3]));
        assert!(globs.is_match(INPUT_LIST[4]));
        assert!(!globs.is_match(INPUT_LIST[5]));
        assert!(globs.is_match(INPUT_LIST[6]));
        assert!(!globs.is_match(INPUT_LIST[7]));
        assert!(!globs.is_match(INPUT_LIST[8]));
        assert!(!globs.is_match(INPUT_LIST[9]));
        assert!(!globs.is_match(INPUT_LIST[10]));
        assert!(!globs.is_match(INPUT_LIST[11]));
        assert!(globs.is_match(INPUT_LIST[12]));
        let globs = Globs::new_with_annotated_set(
            &vec!["+//**/**".into(), "-//capsules:*".into()]
                .into_iter()
                .collect(),
        );
        assert!(!globs.is_match(INPUT_LIST[0]));
        assert!(!globs.is_match(INPUT_LIST[1]));
        assert!(!globs.is_match(INPUT_LIST[2]));
        assert!(!globs.is_match(INPUT_LIST[3]));
        assert!(!globs.is_match(INPUT_LIST[4]));
        assert!(!globs.is_match(INPUT_LIST[5]));
        assert!(!globs.is_match(INPUT_LIST[6]));
        assert!(globs.is_match(INPUT_LIST[7]));
        assert!(globs.is_match(INPUT_LIST[8]));
        assert!(globs.is_match(INPUT_LIST[9]));
        assert!(globs.is_match(INPUT_LIST[10]));
        assert!(globs.is_match(INPUT_LIST[11]));
        assert!(globs.is_match(INPUT_LIST[12]));
        let globs = Globs::new_with_annotated_set(
            &vec!["+//**/capsules/**:*".into()].into_iter().collect(),
        );
        assert!(!globs.is_match(INPUT_LIST[0]));
        assert!(!globs.is_match(INPUT_LIST[1]));
        assert!(!globs.is_match(INPUT_LIST[2]));
        assert!(!globs.is_match(INPUT_LIST[3]));
        assert!(!globs.is_match(INPUT_LIST[4]));
        assert!(!globs.is_match(INPUT_LIST[5]));
        assert!(!globs.is_match(INPUT_LIST[6]));
        assert!(!globs.is_match(INPUT_LIST[7]));
        assert!(!globs.is_match(INPUT_LIST[8]));
        assert!(globs.is_match(INPUT_LIST[9]));
        assert!(globs.is_match(INPUT_LIST[10]));
        assert!(globs.is_match(INPUT_LIST[11]));
        assert!(globs.is_match(INPUT_LIST[12]));
        let globs = Globs::new_with_annotated_set(
            &vec![
                "+//*/capsules/**:*".into(),
                "-//*/capsules/**:*install*".into(),
            ]
            .into_iter()
            .collect(),
        );
        assert!(!globs.is_match(INPUT_LIST[0]));
        assert!(!globs.is_match(INPUT_LIST[1]));
        assert!(!globs.is_match(INPUT_LIST[2]));
        assert!(!globs.is_match(INPUT_LIST[3]));
        assert!(!globs.is_match(INPUT_LIST[4]));
        assert!(!globs.is_match(INPUT_LIST[5]));
        assert!(!globs.is_match(INPUT_LIST[6]));
        assert!(!globs.is_match(INPUT_LIST[7]));
        assert!(!globs.is_match(INPUT_LIST[8]));
        assert!(globs.is_match(INPUT_LIST[9]));
        assert!(globs.is_match(INPUT_LIST[10]));
        assert!(!globs.is_match(INPUT_LIST[11]));
        assert!(!globs.is_match(INPUT_LIST[12]));
    }

    #[test]
    fn test_is_glob_include() {
        assert_eq!(is_glob_include("+foo"), Some("foo".into()));
        assert_eq!(is_glob_include("-foo"), None);
        assert_eq!(is_glob_include("+"), Some(".".into()));
        assert_eq!(is_glob_include("-"), None);
    }

    #[test]
    fn test_match_globs() {
        let globs = Globs::new_with_annotated_set(
            &vec!["+foo".into(), "-foo".into(), "+bar".into()]
                .into_iter()
                .collect(),
        );

        assert!(!globs.is_match("foo"));
        assert!(globs.is_match("bar"));
        assert!(!globs.is_match("baz"));
    }

    #[test]
    fn test_validate() {
        let globs = vec!["+foo".into(), "-foo".into(), "+bar".into(), "-bar".into()]
            .into_iter()
            .collect();

        assert!(validate(&globs).is_ok());

        let globs = vec!["foo".into()].into_iter().collect();
        assert!(validate(&globs).is_err());

        let globs = vec!["-foo".into()].into_iter().collect();
        assert!(validate(&globs).is_err());

        let globs = vec!["-foo".into(), "-bar".into()].into_iter().collect();
        assert!(validate(&globs).is_err());
    }
}
