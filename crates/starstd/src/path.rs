use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use crate::is_lsp_mode;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Joins path segments using the platform separator.
    ///
    /// ```python
    /// p = path.join(["a", "b", "c.txt"])  # "a/b/c.txt" (or "a\\b\\c.txt" on Windows)
    /// ```
    fn join(parts: starlark::values::list::UnpackList<String>) -> anyhow::Result<String> {
        let mut buf = PathBuf::new();
        for part in parts.items {
            buf.push(part);
        }
        Ok(path_to_string_lossy(&buf).into_owned())
    }

    /// Splits a path into (dirname, basename).
    ///
    /// ```python
    /// d, b = path.split("a/b/c.txt")  # ("a/b", "c.txt")
    /// ```
    fn split(path: &str) -> anyhow::Result<(String, String)> {
        let p = Path::new(path);
        let dirname = p
            .parent()
            .map(path_to_string_lossy)
            .map(Cow::into_owned)
            .unwrap_or_default();
        let basename = p
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok((dirname, basename))
    }

    /// Returns the directory portion of the path.
    fn dirname(path: &str) -> anyhow::Result<String> {
        let p = Path::new(path);
        Ok(p.parent()
            .map(path_to_string_lossy)
            .map(Cow::into_owned)
            .unwrap_or_default())
    }

    /// Returns the final path component.
    fn basename(path: &str) -> anyhow::Result<String> {
        let p = Path::new(path);
        Ok(p.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default())
    }

    /// Returns the file stem (filename without final extension).
    fn stem(path: &str) -> anyhow::Result<String> {
        let p = Path::new(path);
        Ok(p.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default())
    }

    /// Returns the file extension without dot.
    fn extension(path: &str) -> anyhow::Result<String> {
        let p = Path::new(path);
        Ok(p.extension()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default())
    }

    /// Replaces the file extension.
    ///
    /// ```python
    /// path.with_extension("a.txt", "md")  # "a.md"
    /// ```
    fn with_extension(path: &str, ext: &str) -> anyhow::Result<String> {
        let mut p = PathBuf::from(path);
        p.set_extension(ext);
        Ok(path_to_string_lossy(&p).into_owned())
    }

    /// Returns true if path is absolute.
    fn is_absolute(path: &str) -> anyhow::Result<bool> {
        Ok(Path::new(path).is_absolute())
    }

    /// Returns an absolute path, resolving against current working directory for relative paths.
    fn absolute(path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let p = Path::new(path);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir()
                .context(format_context!("Failed to read current working directory"))?
                .join(p)
        };
        Ok(path_to_string_lossy(&abs).into_owned())
    }

    /// Canonicalizes a path (resolves symlinks, `.` and `..`) on the real filesystem.
    fn canonicalize(path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let p = std::fs::canonicalize(path)
            .context(format_context!("Failed to canonicalize path: {path}"))?;
        Ok(path_to_string_lossy(&p).into_owned())
    }

    /// Computes `target` relative to `base`.
    ///
    /// ```python
    /// path.relative_to("a/b/c", "a/d")  # "../b/c"
    /// ```
    fn relative_to(target: &str, base: &str) -> anyhow::Result<String> {
        let target_abs = make_absolute(Path::new(target))?;
        let base_abs = make_absolute(Path::new(base))?;
        let rel = diff_paths(&target_abs, &base_abs);
        Ok(path_to_string_lossy(&rel).into_owned())
    }

    /// Normalizes path components lexically (collapses repeated separators, `.` and `..` where possible).
    /// Does not touch filesystem or resolve symlinks.
    fn normalize(path: &str) -> anyhow::Result<String> {
        let normalized = normalize_lexical(Path::new(path));
        Ok(path_to_string_lossy(&normalized).into_owned())
    }

    /// Expands leading `~` to home directory.
    fn expand_user(path: &str) -> anyhow::Result<String> {
        if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
            let home = home_dir().context(format_context!(
                "Cannot expand `~`: home directory is not available"
            ))?;
            if path == "~" {
                return Ok(path_to_string_lossy(&home).into_owned());
            }
            let suffix = &path[2..];
            return Ok(path_to_string_lossy(&home.join(suffix)).into_owned());
        }

        Ok(path.to_string())
    }

    /// Expands `$VAR` and `${VAR}` tokens from process environment.
    fn expand_vars(path: &str) -> anyhow::Result<String> {
        Ok(expand_env_vars(path))
    }

    /// Returns normalized path components as strings.
    fn components(path: &str) -> anyhow::Result<Vec<String>> {
        let p = Path::new(path);
        let comps = p
            .components()
            .filter_map(component_to_string)
            .collect::<Vec<_>>();
        Ok(comps)
    }

    /// Returns the `n`-th parent (default n=1).
    ///
    /// ```python
    /// path.parent("a/b/c", 2)  # "a"
    /// ```
    fn parent(path: &str, n: Option<usize>) -> anyhow::Result<String> {
        let mut p = PathBuf::from(path);
        let steps = n.unwrap_or(1);
        for _ in 0..steps {
            match p.parent() {
                Some(next) => p = next.to_path_buf(),
                None => return Ok(String::new()),
            }
        }
        Ok(path_to_string_lossy(&p).into_owned())
    }

    /// Returns the platform path separator.
    fn separator() -> anyhow::Result<String> {
        Ok(std::path::MAIN_SEPARATOR.to_string())
    }

    /// Convenience no-op to mirror other modules that expose at least one side-effect-free utility.
    fn _path_module_loaded() -> anyhow::Result<NoneType> {
        Ok(NoneType)
    }
}

fn path_to_string_lossy(p: &Path) -> Cow<'_, str> {
    p.to_string_lossy()
}

fn make_absolute(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(normalize_lexical(path))
    } else {
        Ok(normalize_lexical(
            &std::env::current_dir()
                .context(format_context!("Failed to read current working directory"))?
                .join(path),
        ))
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();

    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                match out.components().last() {
                    // A real segment: consume it
                    Some(Component::Normal(_)) => {
                        out.pop();
                    }
                    // At a root or drive prefix: clamp — can't go above the filesystem root
                    Some(Component::RootDir) | Some(Component::Prefix(_)) => {}
                    // Output is empty or ends with `..` already: keep ascending
                    _ => {
                        out.push("..");
                    }
                }
            }
            Component::RootDir | Component::Prefix(_) => out.push(comp.as_os_str()),
            Component::Normal(seg) => out.push(seg),
        }
    }

    out
}

fn diff_paths(target: &Path, base: &Path) -> PathBuf {
    let target_components = target.components().collect::<Vec<_>>();
    let base_components = base.components().collect::<Vec<_>>();

    let mut common_len = 0usize;
    while common_len < target_components.len()
        && common_len < base_components.len()
        && target_components[common_len] == base_components[common_len]
    {
        common_len += 1;
    }

    let mut rel = PathBuf::new();

    for _ in common_len..base_components.len() {
        rel.push("..");
    }

    for comp in target_components.iter().skip(common_len) {
        rel.push(comp.as_os_str());
    }

    if rel.as_os_str().is_empty() {
        rel.push(".");
    }

    rel
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Ok(v) = std::env::var("USERPROFILE") {
            if !v.is_empty() {
                return Some(PathBuf::from(v));
            }
        }
        let drive = std::env::var("HOMEDRIVE").ok();
        let path = std::env::var("HOMEPATH").ok();
        match (drive, path) {
            (Some(d), Some(p)) if !d.is_empty() && !p.is_empty() => {
                Some(PathBuf::from(format!("{d}{p}")))
            }
            _ => None,
        }
    }

    #[cfg(not(windows))]
    {
        std::env::var("HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    }
}

fn component_to_string(component: Component<'_>) -> Option<String> {
    match component {
        Component::CurDir => None,
        Component::RootDir => Some(std::path::MAIN_SEPARATOR.to_string()),
        Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().into_owned()),
        Component::ParentDir => Some("..".to_string()),
        Component::Normal(seg) => Some(seg.to_string_lossy().into_owned()),
    }
}

fn expand_env_vars(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut i = 0usize;
    let mut out = String::with_capacity(input.len());

    while i < bytes.len() {
        if bytes[i] != b'$' {
            // Bulk-copy the non-$ span as a UTF-8 substring to avoid
            // byte-cast corruption of multi-byte characters.
            let start = i;
            while i < bytes.len() && bytes[i] != b'$' {
                i += 1;
            }
            out.push_str(&input[start..i]);
            continue;
        }

        if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let start = i + 2;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'}' {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'}' {
                let key = &input[start..j];
                if is_valid_env_key(key) {
                    let val = std::env::var(key).unwrap_or_default();
                    out.push_str(&val);
                    i = j + 1;
                    continue;
                }
            }
            out.push('$');
            i += 1;
            continue;
        }

        let start = i + 1;
        let mut j = start;
        while j < bytes.len() {
            let c = bytes[j] as char;
            if c == '_' || c.is_ascii_alphanumeric() {
                j += 1;
            } else {
                break;
            }
        }

        if j > start {
            let key = &input[start..j];
            let val = std::env::var(key).unwrap_or_default();
            out.push_str(&val);
            i = j;
        } else {
            out.push('$');
            i += 1;
        }
    }

    out
}

fn is_valid_env_key(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
}
