use crate::is_lsp_mode;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::Deserialize;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;
use starlark::values::none::NoneType;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn mode_to_permissions_string(mode: u32) -> String {
    let mut s = String::with_capacity(9);
    let flags = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    for (bit, ch) in flags {
        s.push(if mode & bit != 0 { ch } else { '-' });
    }
    s
}

fn system_time_to_epoch_seconds(t: std::time::SystemTime) -> anyhow::Result<f64> {
    use anyhow::anyhow;
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => Ok(d.as_secs() as f64 + f64::from(d.subsec_nanos()) / 1_000_000_000.0),
        Err(e) => {
            let d = e.duration();
            let secs = d.as_secs() as f64 + f64::from(d.subsec_nanos()) / 1_000_000_000.0;
            if secs == 0.0 {
                Ok(0.0)
            } else {
                Err(anyhow!("timestamp is before UNIX_EPOCH by {secs} seconds"))
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadGlobsOptions {
    includes: Vec<String>,
    excludes: Option<Vec<String>>,
    root: Option<String>,
    include_files: Option<bool>,
    include_dirs: Option<bool>,
    follow_symlinks: Option<bool>,
    max_depth: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WalkDirectoryOptions {
    path: String,
    recursive: Option<bool>,
    follow_symlinks: Option<bool>,
    include_files: Option<bool>,
    include_dirs: Option<bool>,
    max_depth: Option<usize>,
}

fn normalize_path_for_glob_match(input: &str) -> String {
    let normalized = input.replace('\\', "/");
    if let Some(stripped) = normalized.strip_prefix("./") {
        stripped.to_string()
    } else {
        normalized
    }
}

fn normalize_glob_pattern(pattern: &str) -> String {
    pattern.replace('\\', "/")
}

fn first_glob_char_index(input: &str) -> Option<usize> {
    input
        .char_indices()
        .find(|(_, c)| matches!(c, '*' | '?' | '['))
        .map(|(i, _)| i)
}

fn glob_walk_root(pattern: &str) -> PathBuf {
    let normalized = normalize_glob_pattern(pattern);

    if let Some(glob_index) = first_glob_char_index(&normalized) {
        let prefix = &normalized[..glob_index];

        if let Some(last_slash) = prefix.rfind('/') {
            let base = &prefix[..last_slash];
            if base.is_empty() {
                PathBuf::from(".")
            } else {
                PathBuf::from(base)
            }
        } else {
            PathBuf::from(".")
        }
    } else if normalized.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(normalized)
    }
}

fn is_glob_match(pattern: &str, rel_path: &str, full_path: &str) -> bool {
    glob_match::glob_match(pattern, rel_path) || glob_match::glob_match(pattern, full_path)
}

fn globs_match_path(
    includes: &[String],
    excludes: &[String],
    rel_path: &str,
    full_path: &str,
) -> bool {
    if !includes
        .iter()
        .any(|pattern| is_glob_match(pattern, rel_path, full_path))
    {
        return false;
    }

    !excludes
        .iter()
        .any(|pattern| is_glob_match(pattern, rel_path, full_path))
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Writes a string to a file at the specified path relative to the workspace root.
    fn write_string_to_file(
        #[starlark(require = named)] path: &str,
        #[starlark(require = named)] content: &str,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        use std::io::Write;
        let mut file = std::fs::File::create(path)
            .map_err(|err| format_error!("while creating file {} because {err:?}", path))?;

        file.write_all(content.as_bytes()).map_err(|err| {
            format_error!("while writing string to file {} because {err:?}", path)
        })?;

        Ok(NoneType)
    }

    /// Appends a string to the end of a file at the specified path.
    fn append_string_to_file(
        #[starlark(require = named)] path: &str,
        #[starlark(require = named)] content: &str,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        use std::io::Write;

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .map_err(|err| {
                format_error!("while opening or creating file {path} because {err:?}")
            })?;

        file.write_all(content.as_bytes()).map_err(|err| {
            format_error!("while appending string to file {} because {err:?}", path)
        })?;

        Ok(NoneType)
    }

    /// Reads the contents of a file and returns it as a string.
    fn read_file_to_string(path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|err| format_error!("while reading file {} because {err:?}", path))?;
        Ok(content)
    }

    /// Returns true if the given path exists.
    fn exists(path: &str) -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }
        Ok(std::path::Path::new(path).exists())
    }

    /// Returns true if the given path is a file.
    fn is_file(path: &str) -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }
        Ok(std::path::Path::new(path).is_file())
    }

    /// Returns true if the given path is a directory.
    fn is_directory(path: &str) -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }
        Ok(std::path::Path::new(path).is_dir())
    }

    /// Returns true if the given path is a symbolic link.
    fn is_symlink(path: &str) -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }
        Ok(std::path::Path::new(path).is_symlink())
    }

    /// Returns true if the given path is a text file (valid UTF-8 with no NUL bytes).
    fn is_text_file(path: &str) -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }
        let file_path = std::path::Path::new(path);
        if !file_path.is_file() {
            return Ok(false);
        }

        use std::io::Read;
        let mut file = std::fs::File::open(file_path).map_err(|err| {
            format_error!(
                "while opening file {} for text detection because {err:?}",
                path
            )
        })?;

        // Read up to 8KB for text detection - enough to be confident about file type
        // This avoids loading entire large files into memory
        const SAMPLE_SIZE: usize = 8192;
        let mut buffer = vec![0u8; SAMPLE_SIZE];
        let bytes_read = file.read(&mut buffer).map_err(|err| {
            format_error!(
                "while reading file {} for text detection because {err:?}",
                path
            )
        })?;
        buffer.truncate(bytes_read);

        // A file is considered text if it is valid UTF-8 and contains no NUL bytes.
        if buffer.contains(&0u8) {
            return Ok(false);
        }
        Ok(std::str::from_utf8(&buffer).is_ok())
    }

    /// Reads a TOML file and returns its contents as a dictionary.
    fn read_toml_to_dict<'v>(
        path: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            let heap = eval.heap();
            return Ok(heap.alloc(serde_json::json!({})));
        }
        let heap = eval.heap();
        let content = std::fs::read_to_string(path)
            .map_err(|err| format_error!("while reading TOML file {} because {err:?}", path))?;

        let toml_value: toml::Value = toml::from_str(&content)
            .map_err(|err| format_error!("while parsing TOML file {path} because {err:?}"))?;

        let json_value = serde_json::to_value(toml_value).map_err(|err| {
            format_error!("while converting TOML to JSON for {} because {err:?}", path)
        })?;

        Ok(heap.alloc(json_value))
    }

    /// Reads a YAML file and returns its contents as a dictionary.
    fn read_yaml_to_dict<'v>(
        path: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            let heap = eval.heap();
            return Ok(heap.alloc(serde_json::json!({})));
        }
        let heap = eval.heap();
        let content = std::fs::read_to_string(path)
            .map_err(|err| format_error!("while reading YAML file {} because {err:?}", path))?;

        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&content)
            .map_err(|err| format_error!("while parsing YAML file {} because {err:?}", path))?;

        let json_value = serde_json::to_value(&yaml_value).map_err(|err| {
            format_error!("while converting YAML to JSON for {} because {err:?}", path)
        })?;

        Ok(heap.alloc(json_value))
    }

    /// Reads a JSON file and returns its contents as a dictionary.
    fn read_json_to_dict<'v>(
        path: &str,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            let heap = eval.heap();
            return Ok(heap.alloc(serde_json::json!({})));
        }
        let heap = eval.heap();
        let content = std::fs::read_to_string(path)
            .map_err(|err| format_error!("while reading JSON file {} because {err:?}", path))?;

        let json_value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|err| format_error!("while parsing JSON file {} because {err:?}", path))?;

        Ok(heap.alloc(json_value))
    }

    /// Reads the contents of a directory and returns a list of paths.
    fn read_directory(path: &str) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }
        let entries = std::fs::read_dir(path)
            .map_err(|err| format_error!("while reading directory {} because {err:?}", path))?;

        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| {
                format_error!("while reading directory entry in {} because {err:?}", path)
            })?;
            let p = entry.path();
            let s = p
                .to_str()
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("directory entry path is not UTF-8: {}", p.display()),
                    )
                })
                .map_err(|err| {
                    format_error!(
                        "while converting directory entry path to UTF-8 in {} because {err:?}",
                        path
                    )
                })?;
            result.push(s.to_string());
        }

        Ok(result)
    }

    /// Resolves include/exclude glob expressions to a deduplicated list of filesystem paths.
    ///
    /// `options` must contain:
    /// - `includes` (list[str], required): include patterns.
    /// - `excludes` (list[str], optional, default []): exclude patterns.
    /// - `root` (str, optional, default "."): base path for relative glob roots.
    /// - `include_files` (bool, optional, default true): include non-directory entries.
    /// - `include_dirs` (bool, optional, default false): include directory entries.
    /// - `follow_symlinks` (bool, optional, default false): follow symlinks while walking.
    /// - `max_depth` (int, optional): maximum walk depth relative to each walked include root.
    fn read_globs(options: Value) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let opts_json = options.to_json_value().map_err(|err| {
            format_error!("while converting read_globs options to JSON because {err:?}")
        })?;
        let opts: ReadGlobsOptions = serde_json::from_value(opts_json)
            .map_err(|err| format_error!("while parsing read_globs options because {err:?}"))?;

        let includes = opts
            .includes
            .into_iter()
            .map(|pattern| normalize_glob_pattern(&pattern))
            .collect::<Vec<_>>();
        let excludes = opts
            .excludes
            .unwrap_or_default()
            .into_iter()
            .map(|pattern| normalize_glob_pattern(&pattern))
            .collect::<Vec<_>>();

        let root = PathBuf::from(opts.root.unwrap_or_else(|| ".".to_string()));
        let include_files = opts.include_files.unwrap_or(true);
        let include_dirs = opts.include_dirs.unwrap_or(false);
        let follow_symlinks = opts.follow_symlinks.unwrap_or(false);
        let max_depth = opts.max_depth;

        let mut output = BTreeSet::new();

        for include in &includes {
            let candidate_walk_root = glob_walk_root(include);
            let walk_root = if candidate_walk_root.is_absolute() {
                candidate_walk_root
            } else {
                root.join(candidate_walk_root)
            };

            match std::fs::symlink_metadata(&walk_root) {
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(format_error!(
                        "while accessing glob walk root {} for include pattern {} because {err:?}",
                        walk_root.display(),
                        include
                    ));
                }
            }

            let mut walker = walkdir::WalkDir::new(&walk_root).follow_links(follow_symlinks);
            if let Some(depth) = max_depth {
                walker = walker.max_depth(depth);
            }

            for entry in walker {
                let entry = entry.map_err(|err| {
                    format_error!(
                        "while traversing include pattern {} from root {} because {err:?}",
                        include,
                        walk_root.display()
                    )
                })?;

                if entry.depth() == 0 && entry.file_type().is_dir() {
                    continue;
                }

                let is_dir = entry.file_type().is_dir();
                if is_dir {
                    if !include_dirs {
                        continue;
                    }
                } else if !include_files {
                    continue;
                }

                let full_path = entry.path();
                let full_path_str = full_path
                    .to_str()
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("glob path is not UTF-8: {}", full_path.display()),
                        )
                    })
                    .map_err(|err| {
                        format_error!(
                            "while converting path to UTF-8 while reading globs because {err:?}"
                        )
                    })?;

                let full_norm = normalize_path_for_glob_match(full_path_str);
                let rel_norm = full_path
                    .strip_prefix(&root)
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(normalize_path_for_glob_match)
                    .unwrap_or_else(|| full_norm.clone());

                if globs_match_path(&includes, &excludes, &rel_norm, &full_norm) {
                    output.insert(full_path_str.to_string());
                }
            }
        }

        Ok(output.into_iter().collect())
    }

    /// Walks a directory and invokes a callback with metadata for each entry.
    ///
    /// `options` must contain:
    /// - `path` (str, required): directory path to walk.
    /// - `recursive` (bool, optional, default true): recurse into subdirectories.
    /// - `follow_symlinks` (bool, optional, default false): follow symlinks while walking.
    /// - `include_files` (bool, optional, default true): include non-directory entries.
    /// - `include_dirs` (bool, optional, default false): include directory entries.
    /// - `max_depth` (int, optional): maximum walk depth. Ignored when `recursive` is false.
    ///
    /// Callback signature:
    /// - `callback(entry: dict) -> any`
    ///
    /// The `entry` dictionary contains:
    /// - `path`, `relative_path`, `name`, `depth`, `is_file`, `is_dir`, `is_symlink`
    ///
    /// Return `None` from callback to skip an entry in the returned result list.
    fn walk_directory<'v>(
        options: Value<'v>,
        callback: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Vec<Value<'v>>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let opts_json = options.to_json_value().map_err(|err| {
            format_error!("while converting walk_directory options to JSON because {err:?}")
        })?;
        let opts: WalkDirectoryOptions = serde_json::from_value(opts_json)
            .map_err(|err| format_error!("while parsing walk_directory options because {err:?}"))?;

        let root = PathBuf::from(&opts.path);
        let recursive = opts.recursive.unwrap_or(true);
        let follow_symlinks = opts.follow_symlinks.unwrap_or(false);
        let include_files = opts.include_files.unwrap_or(true);
        let include_dirs = opts.include_dirs.unwrap_or(false);

        let mut walker = walkdir::WalkDir::new(&root).follow_links(follow_symlinks);
        if !recursive {
            walker = walker.max_depth(1);
        } else if let Some(depth) = opts.max_depth {
            walker = walker.max_depth(depth);
        }

        let mut results = Vec::new();

        for entry in walker {
            let entry = entry.map_err(|err| {
                format_error!(
                    "while walking directory {} because {err:?}",
                    opts.path.as_str()
                )
            })?;

            if entry.depth() == 0 && entry.file_type().is_dir() {
                continue;
            }

            let is_dir = entry.file_type().is_dir();
            if is_dir {
                if !include_dirs {
                    continue;
                }
            } else if !include_files {
                continue;
            }

            let path = entry.path();
            let path_str = path
                .to_str()
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("walked path is not UTF-8: {}", path.display()),
                    )
                })
                .map_err(|err| {
                    format_error!(
                        "while converting directory entry to UTF-8 while walking {} because {err:?}",
                        opts.path
                    )
                })?;

            let relative_path = path
                .strip_prefix(&root)
                .ok()
                .and_then(|p| p.to_str())
                .map(|s| {
                    if s.is_empty() {
                        ".".to_string()
                    } else {
                        s.to_string()
                    }
                })
                .unwrap_or_else(|| path_str.to_string());

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let depth = i32::try_from(entry.depth()).unwrap_or(i32::MAX);
            let heap = eval.heap();
            let entry_value = heap.alloc(serde_json::json!({
                "path": path_str,
                "relative_path": relative_path,
                "name": name,
                "depth": depth,
                "is_file": entry.file_type().is_file(),
                "is_dir": is_dir,
                "is_symlink": entry.file_type().is_symlink(),
            }));

            let callback_result =
                eval.eval_function(callback, &[entry_value], &[])
                    .map_err(|err| {
                        format_error!("while executing walk_directory callback because {err:?}")
                    })?;

            if !callback_result.is_none() {
                results.push(callback_result);
            }
        }

        Ok(results)
    }

    // -------------------------
    // Extended additive builtins
    // -------------------------

    fn mkdir(
        path: &str,
        #[starlark(require = named, default = false)] parents: bool,
        #[starlark(require = named, default = false)] exist_ok: bool,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        if parents {
            if exist_ok {
                std::fs::create_dir_all(path).map_err(|err| {
                    format_error!("while creating directory tree {} because {err:?}", path)
                })?;
            } else {
                if std::path::Path::new(path).exists() {
                    anyhow::bail!("Directory already exists: {}", path);
                }
                std::fs::create_dir_all(path).map_err(|err| {
                    format_error!("while creating directory tree {} because {err:?}", path)
                })?;
            }
        } else {
            match std::fs::create_dir(path) {
                Ok(()) => {}
                Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(err) => {
                    return Err(format_error!(
                        "while creating directory {} because {err:?}",
                        path
                    ));
                }
            }
        }
        Ok(NoneType)
    }

    fn copy(
        src: &str,
        dst: &str,
        #[starlark(require = named, default = false)] recursive: bool,
        #[starlark(require = named, default = false)] overwrite: bool,
        #[starlark(require = named, default = true)] follow_symlinks: bool,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        fn copy_one(
            src: &std::path::Path,
            dst: &std::path::Path,
            overwrite: bool,
            follow_symlinks: bool,
        ) -> anyhow::Result<()> {
            let md = if follow_symlinks {
                std::fs::metadata(src)
            } else {
                std::fs::symlink_metadata(src)
            }
            .map_err(|err| {
                format_error!(
                    "while reading source metadata {} because {err:?}",
                    src.display()
                )
            })?;

            if md.is_dir() {
                anyhow::bail!("Source is a directory; use recursive=True for directory copy");
            }

            if dst.exists() && !overwrite {
                anyhow::bail!("Destination exists and overwrite=False: {}", dst.display());
            }

            if let Some(parent) = dst.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent).map_err(|err| {
                    format_error!(
                        "while creating destination parent {} because {err:?}",
                        parent.display()
                    )
                })?;
            }

            if dst.exists() && overwrite {
                if dst.is_dir() {
                    std::fs::remove_dir_all(dst).map_err(|err| {
                        format_error!(
                            "while removing destination directory {} because {err:?}",
                            dst.display()
                        )
                    })?;
                } else {
                    std::fs::remove_file(dst).map_err(|err| {
                        format_error!(
                            "while removing destination file {} because {err:?}",
                            dst.display()
                        )
                    })?;
                }
            }

            // When not following symlinks and source is a symlink, recreate it.
            if !follow_symlinks && md.file_type().is_symlink() {
                let link_target = std::fs::read_link(src).map_err(|err| {
                    format_error!(
                        "while reading symlink target of {} because {err:?}",
                        src.display()
                    )
                })?;
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&link_target, dst).map_err(|err| {
                        format_error!(
                            "while creating symlink {} -> {} because {err:?}",
                            dst.display(),
                            link_target.display()
                        )
                    })?;
                    return Ok(());
                }
                #[cfg(windows)]
                {
                    if link_target.is_dir() {
                        std::os::windows::fs::symlink_dir(&link_target, dst).map_err(|err| {
                            format_error!(
                                "while creating directory symlink {} -> {} because {err:?}",
                                dst.display(),
                                link_target.display()
                            )
                        })?;
                    } else {
                        std::os::windows::fs::symlink_file(&link_target, dst).map_err(|err| {
                            format_error!(
                                "while creating file symlink {} -> {} because {err:?}",
                                dst.display(),
                                link_target.display()
                            )
                        })?;
                    }
                    return Ok(());
                }
                #[allow(unreachable_code)]
                return Ok(());
            }

            std::fs::copy(src, dst).map_err(|err| {
                format_error!(
                    "while copying {} -> {} because {err:?}",
                    src.display(),
                    dst.display()
                )
            })?;
            Ok(())
        }

        fn copy_dir_recursive(
            src: &std::path::Path,
            dst: &std::path::Path,
            overwrite: bool,
            follow_symlinks: bool,
        ) -> anyhow::Result<()> {
            if dst.exists() {
                if !overwrite {
                    anyhow::bail!("Destination exists and overwrite=False: {}", dst.display());
                }
            } else {
                std::fs::create_dir_all(dst).map_err(|err| {
                    format_error!(
                        "while creating destination directory {} because {err:?}",
                        dst.display()
                    )
                })?;
            }

            for entry in std::fs::read_dir(src).map_err(|err| {
                format_error!("while reading directory {} because {err:?}", src.display())
            })? {
                let entry = entry.map_err(|err| {
                    format_error!(
                        "while reading directory entry in {} because {err:?}",
                        src.display()
                    )
                })?;
                let from = entry.path();
                let to = dst.join(entry.file_name());

                let md = if follow_symlinks {
                    std::fs::metadata(&from)
                } else {
                    std::fs::symlink_metadata(&from)
                }
                .map_err(|err| {
                    format_error!(
                        "while reading metadata for {} because {err:?}",
                        from.display()
                    )
                })?;

                if md.is_dir() {
                    copy_dir_recursive(&from, &to, overwrite, follow_symlinks)?;
                } else {
                    copy_one(&from, &to, overwrite, follow_symlinks)?;
                }
            }

            Ok(())
        }

        let src_path = std::path::Path::new(src);
        let dst_path = std::path::Path::new(dst);

        let md = if follow_symlinks {
            std::fs::metadata(src_path)
        } else {
            std::fs::symlink_metadata(src_path)
        }
        .map_err(|err| format_error!("while reading source metadata {} because {err:?}", src))?;

        if md.is_dir() {
            if !recursive {
                anyhow::bail!("Source is a directory; set recursive=True");
            }
            copy_dir_recursive(src_path, dst_path, overwrite, follow_symlinks)?;
        } else {
            copy_one(src_path, dst_path, overwrite, follow_symlinks)?;
        }

        Ok(NoneType)
    }

    fn r#move(
        src: &str,
        dst: &str,
        #[starlark(require = named, default = false)] overwrite: bool,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let src_path = std::path::Path::new(src);
        let dst_path = std::path::Path::new(dst);

        if dst_path.exists() {
            if !overwrite {
                anyhow::bail!("Destination exists and overwrite=False: {}", dst);
            }
            if dst_path.is_dir() {
                std::fs::remove_dir_all(dst_path).map_err(|err| {
                    format_error!(
                        "while removing destination directory {} because {err:?}",
                        dst
                    )
                })?;
            } else {
                std::fs::remove_file(dst_path).map_err(|err| {
                    format_error!("while removing destination file {} because {err:?}", dst)
                })?;
            }
        } else if let Some(parent) = dst_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|err| {
                format_error!(
                    "while creating destination parent {} because {err:?}",
                    parent.display()
                )
            })?;
        }

        match std::fs::rename(src_path, dst_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
                // Cross-device move: fall back to copy then delete.
                let src_md = std::fs::symlink_metadata(src_path).map_err(|err| {
                    format_error!(
                        "while reading source metadata {} for cross-device move because {err:?}",
                        src
                    )
                })?;
                if src_md.is_dir() {
                    copy_dir_all(src_path, dst_path).map_err(|err| {
                        format_error!(
                            "while copying directory {} -> {} for cross-device move because {err:?}",
                            src,
                            dst
                        )
                    })?;
                    std::fs::remove_dir_all(src_path).map_err(|err| {
                        format_error!(
                            "while removing source directory {} after cross-device move because {err:?}",
                            src
                        )
                    })?;
                } else {
                    std::fs::copy(src_path, dst_path).map_err(|err| {
                        format_error!(
                            "while copying {} -> {} for cross-device move because {err:?}",
                            src,
                            dst
                        )
                    })?;
                    std::fs::remove_file(src_path).map_err(|err| {
                        format_error!(
                            "while removing source {} after cross-device move because {err:?}",
                            src
                        )
                    })?;
                }
            }
            Err(err) => {
                return Err(format_error!(
                    "while moving {} -> {} because {err:?}",
                    src,
                    dst
                ));
            }
        }
        Ok(NoneType)
    }

    fn remove(
        path: &str,
        #[starlark(require = named, default = false)] recursive: bool,
        #[starlark(require = named, default = true)] missing_ok: bool,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let p = std::path::Path::new(path);
        if !p.exists() {
            if missing_ok {
                return Ok(NoneType);
            }
            anyhow::bail!("Path does not exist: {}", path);
        }

        let md = std::fs::symlink_metadata(p).map_err(|err| {
            format_error!("while reading metadata for path {} because {err:?}", path)
        })?;

        if md.is_dir() {
            if recursive {
                std::fs::remove_dir_all(p).map_err(|err| {
                    format_error!(
                        "while removing directory recursively {} because {err:?}",
                        path
                    )
                })?;
            } else {
                std::fs::remove_dir(p).map_err(|err| {
                    format_error!("while removing directory {} because {err:?}", path)
                })?;
            }
        } else {
            std::fs::remove_file(p)
                .map_err(|err| format_error!("while removing file {} because {err:?}", path))?;
        }

        Ok(NoneType)
    }

    fn symlink(target: &str, link: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).map_err(|err| {
                format_error!(
                    "while creating symlink {} -> {} because {err:?}",
                    link,
                    target
                )
            })?;
            return Ok(NoneType);
        }

        #[cfg(windows)]
        {
            let target_path = std::path::Path::new(target);
            if target_path.is_dir() {
                std::os::windows::fs::symlink_dir(target, link).map_err(|err| {
                    format_error!(
                        "while creating directory symlink {} -> {} because {err:?}",
                        link,
                        target
                    )
                })?;
            } else {
                std::os::windows::fs::symlink_file(target, link).map_err(|err| {
                    format_error!(
                        "while creating file symlink {} -> {} because {err:?}",
                        link,
                        target
                    )
                })?;
            }
            return Ok(NoneType);
        }

        #[allow(unreachable_code)]
        Ok(NoneType)
    }

    fn read_link(path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let target = std::fs::read_link(path)
            .map_err(|err| format_error!("while reading symlink {} because {err:?}", path))?;
        let s = target
            .to_str()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("symlink target is not UTF-8: {}", target.display()),
                )
            })
            .map_err(|err| {
                format_error!(
                    "while converting symlink target to UTF-8 for {} because {err:?}",
                    path
                )
            })?;
        Ok(s.to_owned())
    }

    fn touch(
        path: &str,
        #[starlark(require = named, default = true)] create: bool,
        #[starlark(require = named, default = true)] update_mtime: bool,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        let p = std::path::Path::new(path);

        if !p.exists() {
            if !create {
                return Ok(NoneType);
            }
            std::fs::File::create(p)
                .map_err(|err| format_error!("while creating file {} because {err:?}", path))?;
            return Ok(NoneType);
        }

        if update_mtime {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(p)
                .map_err(|err| {
                    format_error!("while opening file {} for touch because {err:?}", path)
                })?;
            let now = std::time::SystemTime::now();
            let times = std::fs::FileTimes::new()
                .set_accessed(now)
                .set_modified(now);
            f.set_times(times).map_err(|err| {
                format_error!("while setting file times for {} because {err:?}", path)
            })?;
        }

        Ok(NoneType)
    }

    fn metadata<'v>(path: &str, eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            let mut obj = serde_json::Map::new();
            obj.insert("size".to_string(), serde_json::json!(0));
            obj.insert("modified".to_string(), serde_json::json!(null));
            obj.insert("created".to_string(), serde_json::json!(null));
            obj.insert("is_dir".to_string(), serde_json::json!(false));
            obj.insert("is_file".to_string(), serde_json::json!(false));
            obj.insert("is_symlink".to_string(), serde_json::json!(false));
            obj.insert("permissions".to_string(), serde_json::json!(""));
            obj.insert("mode".to_string(), serde_json::json!(0));
            return Ok(eval.heap().alloc(serde_json::Value::Object(obj)));
        }

        // symlink_metadata sees the link itself; used for is_symlink and Unix mode bits.
        let lmd = std::fs::symlink_metadata(path).map_err(|err| {
            format_error!("while reading metadata for path {} because {err:?}", path)
        })?;
        let is_symlink = lmd.file_type().is_symlink();

        // metadata follows symlinks; used for is_file/is_dir/size/timestamps so they
        // agree with fs_is_file, fs_is_directory, fs_size, and fs_modified.
        // For a dangling symlink metadata() will fail; fall back to lmd values.
        let (is_file, is_dir, size, modified, created) = match std::fs::metadata(path) {
            Ok(md) => {
                let modified = md
                    .modified()
                    .ok()
                    .and_then(|t| system_time_to_epoch_seconds(t).ok());
                let created = md
                    .created()
                    .ok()
                    .and_then(|t| system_time_to_epoch_seconds(t).ok());
                (md.is_file(), md.is_dir(), md.len(), modified, created)
            }
            Err(_) => {
                // dangling symlink or unresolvable path
                let modified = lmd
                    .modified()
                    .ok()
                    .and_then(|t| system_time_to_epoch_seconds(t).ok());
                let created = lmd
                    .created()
                    .ok()
                    .and_then(|t| system_time_to_epoch_seconds(t).ok());
                (false, false, lmd.len(), modified, created)
            }
        };

        #[cfg(unix)]
        let mode: u32 = lmd.mode() & 0o7777;
        #[cfg(not(unix))]
        let mode: u32 = if lmd.permissions().readonly() {
            0o444
        } else {
            0o666
        };

        let permissions = mode_to_permissions_string(mode & 0o777);

        let mut obj = serde_json::Map::new();
        obj.insert("size".to_string(), serde_json::json!(size));
        obj.insert("modified".to_string(), serde_json::json!(modified));
        obj.insert("created".to_string(), serde_json::json!(created));
        obj.insert("is_dir".to_string(), serde_json::json!(is_dir));
        obj.insert("is_file".to_string(), serde_json::json!(is_file));
        obj.insert("is_symlink".to_string(), serde_json::json!(is_symlink));
        obj.insert("permissions".to_string(), serde_json::json!(permissions));
        obj.insert("mode".to_string(), serde_json::json!(mode));

        Ok(eval.heap().alloc(serde_json::Value::Object(obj)))
    }

    fn size(path: &str) -> anyhow::Result<u64> {
        if is_lsp_mode() {
            return Ok(0);
        }
        let md = std::fs::metadata(path).map_err(|err| {
            format_error!("while reading file metadata for {} because {err:?}", path)
        })?;
        Ok(md.len())
    }

    fn modified(path: &str) -> anyhow::Result<f64> {
        if is_lsp_mode() {
            return Ok(0.0);
        }
        let md = std::fs::metadata(path).map_err(|err| {
            format_error!("while reading file metadata for {} because {err:?}", path)
        })?;
        let t = md.modified().map_err(|err| {
            format_error!("while reading modified time for {} because {err:?}", path)
        })?;
        system_time_to_epoch_seconds(t)
    }

    fn set_permissions(
        path: &str,
        #[starlark(require = named)] mode: i32,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        #[cfg(unix)]
        {
            let perms = std::fs::Permissions::from_mode(mode as u32);
            std::fs::set_permissions(path, perms).map_err(|err| {
                format_error!(
                    "while setting permissions {:o} on {} because {err:?}",
                    mode,
                    path
                )
            })?;
            return Ok(NoneType);
        }

        #[cfg(not(unix))]
        {
            let mut perms = std::fs::metadata(path)
                .map_err(|err| {
                    format_error!("while reading metadata for path {} because {err:?}", path)
                })?
                .permissions();
            perms.set_readonly((mode & 0o222) == 0);
            std::fs::set_permissions(path, perms).map_err(|err| {
                format_error!("while setting permissions on {} because {err:?}", path)
            })?;
            return Ok(NoneType);
        }
    }

    fn chmod(path: &str, spec: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        #[cfg(unix)]
        {
            let md = std::fs::metadata(path).map_err(|err| {
                format_error!("while reading metadata for {} because {err:?}", path)
            })?;
            let mut mode = md.mode() & 0o7777;

            if spec.len() < 3 {
                anyhow::bail!("Invalid chmod spec '{}': must be [ugoa][+-=][rwx]+", spec);
            }
            let mut chars = spec.chars();
            let who = chars
                .next()
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing chmod subject")
                })
                .map_err(|err| {
                    format_error!("while parsing chmod spec '{}' because {err:?}", spec)
                })?;
            let op = chars
                .next()
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing chmod operator")
                })
                .map_err(|err| {
                    format_error!("while parsing chmod spec '{}' because {err:?}", spec)
                })?;
            let perm_chars: String = chars.collect();
            if perm_chars.is_empty() {
                anyhow::bail!("Invalid chmod spec '{}': no permission characters", spec);
            }

            let scope = match who {
                'u' => 0o700u32,
                'g' => 0o070u32,
                'o' => 0o007u32,
                'a' => 0o777u32,
                _ => anyhow::bail!("Invalid chmod subject '{}' in spec '{}'", who, spec),
            };

            let mut perm_mask = 0u32;
            for perm in perm_chars.chars() {
                let bits = match perm {
                    'r' => 0o444u32,
                    'w' => 0o222u32,
                    'x' => 0o111u32,
                    _ => anyhow::bail!("Invalid chmod permission '{}' in spec '{}'", perm, spec),
                };
                perm_mask |= bits & scope;
            }

            match op {
                '+' => mode |= perm_mask,
                '-' => mode &= !perm_mask,
                '=' => {
                    mode &= !scope;
                    mode |= perm_mask;
                }
                _ => anyhow::bail!("Invalid chmod operator '{}' in spec '{}'", op, spec),
            }

            let perms = std::fs::Permissions::from_mode(mode);
            std::fs::set_permissions(path, perms).map_err(|err| {
                format_error!(
                    "while applying chmod {} with '{}' because {err:?}",
                    path,
                    spec
                )
            })?;
            return Ok(NoneType);
        }

        #[cfg(not(unix))]
        {
            anyhow::bail!("chmod symbolic mode is only supported on unix platforms");
        }
    }

    fn chown(
        path: &str,
        #[starlark(require = named)] user: &str,
        #[starlark(require = named)] group: &str,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        #[cfg(unix)]
        {
            use std::process::Command;
            let status = Command::new("chown")
                .arg(format!("{user}:{group}"))
                .arg("--")
                .arg(path)
                .status()
                .map_err(|err| format_error!("while running chown for {} because {err:?}", path))?;
            if !status.success() {
                anyhow::bail!("chown failed for {} to {}:{}", path, user, group);
            }
            return Ok(NoneType);
        }

        #[cfg(not(unix))]
        {
            let _ = (path, user, group);
            anyhow::bail!("chown is only supported on unix platforms");
        }
    }

    fn read_bytes(path: &str) -> anyhow::Result<Vec<u32>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(path)
            .map_err(|err| format_error!("while reading bytes from {} because {err:?}", path))?;
        Ok(bytes.into_iter().map(u32::from).collect())
    }

    fn write_bytes(path: &str, data: Value) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let json_text = data.to_json().map_err(|err| {
            format_error!(
                "while converting data argument to JSON for {} because {err:?}",
                path
            )
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&json_text).map_err(|err| {
            format_error!("while parsing data as JSON for {} because {err:?}", path)
        })?;
        let arr = parsed
            .as_array()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "data was not a list")
            })
            .map_err(|err| {
                format_error!("while validating byte data for {} because {err:?}", path)
            })?;

        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let n = item
                .as_u64()
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "byte value was not an integer",
                    )
                })
                .map_err(|err| {
                    format_error!("while validating byte data for {} because {err:?}", path)
                })?;
            let b = u8::try_from(n).map_err(|err| {
                format_error!("while converting byte value for {} because {err:?}", path)
            })?;
            out.push(b);
        }

        std::fs::write(path, out)
            .map_err(|err| format_error!("while writing bytes to {} because {err:?}", path))?;
        Ok(NoneType)
    }

    fn read_lines(path: &str) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|err| format_error!("while reading file {} because {err:?}", path))?;
        Ok(content.lines().map(|s| s.to_string()).collect())
    }

    fn write_lines(path: &str, lines: Value) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let json_text = lines.to_json().map_err(|err| {
            format_error!(
                "while converting lines argument to JSON for {} because {err:?}",
                path
            )
        })?;
        let parsed: serde_json::Value = serde_json::from_str(&json_text).map_err(|err| {
            format_error!("while parsing lines as JSON for {} because {err:?}", path)
        })?;
        let arr = parsed
            .as_array()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "lines was not a list")
            })
            .map_err(|err| format_error!("while validating lines for {} because {err:?}", path))?;

        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let s = item
                .as_str()
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "line value was not a string",
                    )
                })
                .map_err(|err| {
                    format_error!("while validating lines for {} because {err:?}", path)
                })?;
            out.push(s.to_owned());
        }

        let content = if out.is_empty() {
            String::new()
        } else {
            out.join("\n") + "\n"
        };
        std::fs::write(path, content)
            .map_err(|err| format_error!("while writing lines to {} because {err:?}", path))?;
        Ok(NoneType)
    }

    fn write_toml_from_dict(path: &str, value: Value) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let json_text = value.to_json().map_err(|err| {
            format_error!(
                "while converting Starlark value to JSON for {} because {err:?}",
                path
            )
        })?;
        let json_value: serde_json::Value = serde_json::from_str(&json_text).map_err(|err| {
            format_error!(
                "while parsing JSON representation for {} because {err:?}",
                path
            )
        })?;
        let s = toml::to_string_pretty(&json_value)
            .map_err(|err| format_error!("while serializing TOML for {} because {err:?}", path))?;
        std::fs::write(path, s)
            .map_err(|err| format_error!("while writing TOML file {} because {err:?}", path))?;
        Ok(NoneType)
    }

    fn write_yaml_from_dict(path: &str, value: Value) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let json_text = value.to_json().map_err(|err| {
            format_error!(
                "while converting Starlark value to JSON for {} because {err:?}",
                path
            )
        })?;
        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&json_text).map_err(|err| {
            format_error!(
                "while converting JSON to YAML value for {} because {err:?}",
                path
            )
        })?;
        let s = serde_yaml::to_string(&yaml_value)
            .map_err(|err| format_error!("while serializing YAML for {} because {err:?}", path))?;
        std::fs::write(path, s)
            .map_err(|err| format_error!("while writing YAML file {} because {err:?}", path))?;
        Ok(NoneType)
    }

    fn write_json_from_dict(
        path: &str,
        value: Value,
        #[starlark(require = named, default = true)] pretty: bool,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let json = value.to_json_value().map_err(|err| {
            format_error!(
                "while converting Starlark value to JSON for {} because {err:?}",
                path
            )
        })?;
        let s = if pretty {
            serde_json::to_string_pretty(&json)
        } else {
            serde_json::to_string(&json)
        }
        .map_err(|err| format_error!("while serializing JSON for {} because {err:?}", path))?;
        std::fs::write(path, s)
            .map_err(|err| format_error!("while writing JSON file {} because {err:?}", path))?;
        Ok(NoneType)
    }

    fn write_string_atomic(
        path: &str,
        content: &str,
        #[starlark(require = named, default = 0o644)] mode: i32,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        use std::io::Write;

        let dst = std::path::Path::new(path);
        let parent = dst.parent().unwrap_or_else(|| std::path::Path::new("."));
        std::fs::create_dir_all(parent).map_err(|err| {
            format_error!(
                "while creating parent directory {} because {err:?}",
                parent.display()
            )
        })?;

        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp_name = format!(
            ".{}.{}.{}.tmp",
            dst.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("tmpfile"),
            pid,
            nanos
        );
        let tmp_path = parent.join(tmp_name);

        {
            let mut f = std::fs::File::create(&tmp_path).map_err(|err| {
                format_error!(
                    "while creating temporary file {} because {err:?}",
                    tmp_path.display()
                )
            })?;
            f.write_all(content.as_bytes()).map_err(|err| {
                format_error!(
                    "while writing temporary file {} because {err:?}",
                    tmp_path.display()
                )
            })?;
            f.sync_all().map_err(|err| {
                format_error!(
                    "while syncing temporary file {} because {err:?}",
                    tmp_path.display()
                )
            })?;
        }

        #[cfg(unix)]
        {
            let perms = std::fs::Permissions::from_mode(mode as u32);
            std::fs::set_permissions(&tmp_path, perms).map_err(|err| {
                format_error!(
                    "while setting mode {:o} on temporary file {} because {err:?}",
                    mode,
                    tmp_path.display()
                )
            })?;
        }

        std::fs::rename(&tmp_path, dst).map_err(|err| {
            format_error!(
                "while atomically renaming {} -> {} because {err:?}",
                tmp_path.display(),
                dst.display()
            )
        })?;

        Ok(NoneType)
    }

    /// Acquire an advisory file lock for the duration of a callback.
    ///
    /// The callback is invoked while the lock is held, and the lock is always
    /// released before this function returns (even if the callback errors).
    ///
    /// This is implemented using the `fd-lock` crate and is advisory: other
    /// processes must also use advisory locking to participate.
    ///
    /// Example:
    ///
    /// ```python
    /// def critical_section():
    ///     fs.append_string_to_file(path = "build.log", content = "locked write\n")
    ///
    /// fs.with_file_lock(".spaces/build.log.lock", critical_section)
    /// ```
    fn with_file_lock<'v>(
        path: &str,
        callback: Value<'v>,
        #[starlark(require = named, default = true)] exclusive: bool,
        #[starlark(require = named, default = true)] blocking: bool,
        #[starlark(require = named, default = true)] create: bool,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            return Ok(Value::new_none());
        }

        let lock_path = std::path::Path::new(path);
        if create
            && let Some(parent) = lock_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|err| {
                format_error!(
                    "while creating parent directory for lock {} because {err:?}",
                    parent.display()
                )
            })?;
        }

        let file = if create && !exclusive {
            // For shared locks, prefer opening read-only when the file already exists.
            // This avoids requiring write permission unless creation is actually needed.
            match std::fs::OpenOptions::new().read(true).open(lock_path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(lock_path)
                    .map_err(|err| {
                        format_error!("while opening lock file {} because {err:?}", path)
                    })?,
                Err(err) => {
                    return Err(format_error!(
                        "while opening lock file {} because {err:?}",
                        path
                    ));
                }
            }
        } else {
            let mut open_options = std::fs::OpenOptions::new();
            if create {
                // `OpenOptions::create` requires write or append mode.
                open_options
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false);
            } else if exclusive {
                open_options.read(true).write(true);
            } else {
                open_options.read(true);
            }

            open_options
                .open(lock_path)
                .map_err(|err| format_error!("while opening lock file {} because {err:?}", path))?
        };
        let mut lock = fd_lock::RwLock::new(file);

        if exclusive {
            if blocking {
                let _guard = lock.write().map_err(|err| {
                    format_error!("while acquiring exclusive lock on {} because {err:?}", path)
                })?;
                return eval.eval_function(callback, &[], &[]).map_err(|err| {
                    format_error!("while executing lock callback for {} because {err:?}", path)
                });
            }

            let _guard = match lock.try_write() {
                Ok(guard) => guard,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    anyhow::bail!("Lock is currently held and blocking=False: {}", path)
                }
                Err(err) => {
                    return Err(format_error!(
                        "while acquiring exclusive lock on {} because {err:?}",
                        path
                    ));
                }
            };
            return eval.eval_function(callback, &[], &[]).map_err(|err| {
                format_error!("while executing lock callback for {} because {err:?}", path)
            });
        }

        if blocking {
            let _guard = lock.read().map_err(|err| {
                format_error!("while acquiring shared lock on {path} because {err:?}")
            })?;
            return eval.eval_function(callback, &[], &[]).map_err(|err| {
                format_error!("while executing lock callback for {path} because {err:?}")
            });
        }

        let _guard = match lock.try_read() {
            Ok(guard) => guard,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::bail!("Lock is currently held and blocking=False: {path}")
            }
            Err(err) => {
                return Err(format_error!(
                    "while acquiring shared lock on {path} because {err:?}"
                ));
            }
        };

        eval.eval_function(callback, &[], &[]).map_err(|err| {
            format_error!("while executing lock callback for {path} because {err:?}")
        })
    }
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst).context(format_context!(
        "Failed to create directory {}",
        dst.display()
    ))?;
    for entry in std::fs::read_dir(src).context(format_context!(
        "Failed to read directory {}",
        src.display()
    ))? {
        let entry = entry.context(format_context!(
            "Failed to read directory entry in {}",
            src.display()
        ))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let md = std::fs::symlink_metadata(&from)
            .context(format_context!("Failed to stat {}", from.display()))?;
        if md.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).context(format_context!(
                "Failed to copy {} -> {}",
                from.display(),
                to.display()
            ))?;
        }
    }
    Ok(())
}
