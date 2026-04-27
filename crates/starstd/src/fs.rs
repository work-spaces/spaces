use crate::is_lsp_mode;
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;
use starlark::values::none::NoneType;

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
        let mut file = std::fs::File::create(path).context(format_context!(
            "Failed to create file {} all paths must be relative to the workspace root",
            path
        ))?;

        file.write_all(content.as_bytes())
            .context(format_context!("Failed to write to file {}", path))?;

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
            .context(format_context!("Failed to open/create {path}"))?;

        file.write_all(content.as_bytes())
            .context(format_context!("Failed to write to file {}", path))?;

        Ok(NoneType)
    }

    /// Reads the contents of a file and returns it as a string.
    fn read_file_to_string(path: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;
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

        let bytes = std::fs::read(file_path).context(format_context!(
            "Failed to read file {} for text detection",
            path
        ))?;

        // A file is considered text if it is valid UTF-8 and contains no NUL bytes.
        if bytes.contains(&0u8) {
            return Ok(false);
        }
        Ok(std::str::from_utf8(&bytes).is_ok())
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
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let toml_value: toml::Value = toml::from_str(&content)
            .context(format_context!("Failed to parse TOML file {}", path))?;

        let json_value = serde_json::to_value(toml_value)
            .context(format_context!("Failed to convert TOML to JSON {}", path))?;

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
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&content)
            .context(format_context!("Failed to parse YAML file {}", path))?;

        let json_value = serde_json::to_value(&yaml_value)
            .context(format_context!("Failed to convert YAML to JSON {}", path))?;

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
        let content = std::fs::read_to_string(path).context(format_context!(
            "Failed to read file {} all paths must be relative to the workspace root",
            path
        ))?;

        let json_value: serde_json::Value = serde_json::from_str(&content)
            .context(format_context!("Failed to parse JSON file {}", path))?;

        Ok(heap.alloc(json_value))
    }

    /// Reads the contents of a directory and returns a list of paths.
    fn read_directory(path: &str) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }
        let entries = std::fs::read_dir(path).context(format_context!(
            "Failed to read directory {} all paths must be relative to the workspace root",
            path
        ))?;

        let mut result = Vec::new();
        for entry in entries {
            let entry = entry.context(format_context!(
                "Failed to read directory entry in {}",
                path
            ))?;
            let p = entry.path();
            let s = p.to_str().context(format_context!(
                "Non-UTF-8 directory entry in {}: {}",
                path,
                p.display()
            ))?;
            result.push(s.to_string());
        }

        Ok(result)
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
                std::fs::create_dir_all(path)
                    .context(format_context!("Failed to create directory tree {}", path))?;
            } else {
                if std::path::Path::new(path).exists() {
                    anyhow::bail!("Directory already exists: {}", path);
                }
                std::fs::create_dir_all(path)
                    .context(format_context!("Failed to create directory tree {}", path))?;
            }
        } else {
            match std::fs::create_dir(path) {
                Ok(()) => {}
                Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => {
                    return Err(e).context(format_context!("Failed to create directory {}", path));
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
            .context(format_context!("Failed to stat source {}", src.display()))?;

            if md.is_dir() {
                anyhow::bail!("Source is a directory; use recursive=True for directory copy");
            }

            if dst.exists() && !overwrite {
                anyhow::bail!("Destination exists and overwrite=False: {}", dst.display());
            }

            if let Some(parent) = dst.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent).context(format_context!(
                    "Failed to create destination parent {}",
                    parent.display()
                ))?;
            }

            if dst.exists() && overwrite {
                if dst.is_dir() {
                    std::fs::remove_dir_all(dst).context(format_context!(
                        "Failed to remove destination dir {}",
                        dst.display()
                    ))?;
                } else {
                    std::fs::remove_file(dst).context(format_context!(
                        "Failed to remove destination file {}",
                        dst.display()
                    ))?;
                }
            }

            // When not following symlinks and source is a symlink, recreate it.
            if !follow_symlinks && md.file_type().is_symlink() {
                let link_target = std::fs::read_link(src).context(format_context!(
                    "Failed to read symlink target of {}",
                    src.display()
                ))?;
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&link_target, dst).context(format_context!(
                        "Failed to create symlink {} -> {}",
                        dst.display(),
                        link_target.display()
                    ))?;
                    return Ok(());
                }
                #[cfg(windows)]
                {
                    if link_target.is_dir() {
                        std::os::windows::fs::symlink_dir(&link_target, dst).context(
                            format_context!(
                                "Failed to create dir symlink {} -> {}",
                                dst.display(),
                                link_target.display()
                            ),
                        )?;
                    } else {
                        std::os::windows::fs::symlink_file(&link_target, dst).context(
                            format_context!(
                                "Failed to create file symlink {} -> {}",
                                dst.display(),
                                link_target.display()
                            ),
                        )?;
                    }
                    return Ok(());
                }
                #[allow(unreachable_code)]
                return Ok(());
            }

            std::fs::copy(src, dst).context(format_context!(
                "Failed to copy {} -> {}",
                src.display(),
                dst.display()
            ))?;
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
                std::fs::create_dir_all(dst).context(format_context!(
                    "Failed to create destination dir {}",
                    dst.display()
                ))?;
            }

            for entry in std::fs::read_dir(src)
                .context(format_context!("Failed to read dir {}", src.display()))?
            {
                let entry = entry.context(format_context!(
                    "Failed to read dir entry in {}",
                    src.display()
                ))?;
                let from = entry.path();
                let to = dst.join(entry.file_name());

                let md = if follow_symlinks {
                    std::fs::metadata(&from)
                } else {
                    std::fs::symlink_metadata(&from)
                }
                .context(format_context!("Failed to stat {}", from.display()))?;

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
        .context(format_context!("Failed to stat source {}", src))?;

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
                std::fs::remove_dir_all(dst_path).context(format_context!(
                    "Failed to remove destination directory {}",
                    dst
                ))?;
            } else {
                std::fs::remove_file(dst_path)
                    .context(format_context!("Failed to remove destination file {}", dst))?;
            }
        } else if let Some(parent) = dst_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).context(format_context!(
                "Failed to create destination parent {}",
                parent.display()
            ))?;
        }

        match std::fs::rename(src_path, dst_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
                // Cross-device move: fall back to copy then delete.
                let src_md = std::fs::symlink_metadata(src_path).context(format_context!(
                    "Failed to stat source {} for cross-device move",
                    src
                ))?;
                if src_md.is_dir() {
                    copy_dir_all(src_path, dst_path).context(format_context!(
                        "Failed to copy directory {} -> {} for cross-device move",
                        src,
                        dst
                    ))?;
                    std::fs::remove_dir_all(src_path).context(format_context!(
                        "Failed to remove source directory {} after cross-device move",
                        src
                    ))?;
                } else {
                    std::fs::copy(src_path, dst_path).context(format_context!(
                        "Failed to copy {} -> {} for cross-device move",
                        src,
                        dst
                    ))?;
                    std::fs::remove_file(src_path).context(format_context!(
                        "Failed to remove source {} after cross-device move",
                        src
                    ))?;
                }
            }
            Err(e) => {
                return Err(e).context(format_context!("Failed to move {} -> {}", src, dst));
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

        let md = std::fs::symlink_metadata(p)
            .context(format_context!("Failed to stat path {}", path))?;

        if md.is_dir() {
            if recursive {
                std::fs::remove_dir_all(p).context(format_context!(
                    "Failed to remove directory recursively {}",
                    path
                ))?;
            } else {
                std::fs::remove_dir(p)
                    .context(format_context!("Failed to remove directory {}", path))?;
            }
        } else {
            std::fs::remove_file(p).context(format_context!("Failed to remove file {}", path))?;
        }

        Ok(NoneType)
    }

    fn symlink(target: &str, link: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).context(format_context!(
                "Failed to create symlink {} -> {}",
                link,
                target
            ))?;
            return Ok(NoneType);
        }

        #[cfg(windows)]
        {
            let target_path = std::path::Path::new(target);
            if target_path.is_dir() {
                std::os::windows::fs::symlink_dir(target, link).context(format_context!(
                    "Failed to create directory symlink {} -> {}",
                    link,
                    target
                ))?;
            } else {
                std::os::windows::fs::symlink_file(target, link).context(format_context!(
                    "Failed to create file symlink {} -> {}",
                    link,
                    target
                ))?;
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
        let target =
            std::fs::read_link(path).context(format_context!("Failed to read symlink {}", path))?;
        let s = target.to_str().context(format_context!(
            "Symlink target is not valid UTF-8 for {}",
            path
        ))?;
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
            std::fs::File::create(p).context(format_context!("Failed to create file {}", path))?;
            return Ok(NoneType);
        }

        if update_mtime {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(p)
                .context(format_context!("Failed to open file for touch {}", path))?;
            let now = std::time::SystemTime::now();
            let times = std::fs::FileTimes::new()
                .set_accessed(now)
                .set_modified(now);
            f.set_times(times)
                .context(format_context!("Failed to set file times for {}", path))?;
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
        let lmd = std::fs::symlink_metadata(path)
            .context(format_context!("Failed to stat path {}", path))?;
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
        let md =
            std::fs::metadata(path).context(format_context!("Failed to stat file {}", path))?;
        Ok(md.len())
    }

    fn modified(path: &str) -> anyhow::Result<f64> {
        if is_lsp_mode() {
            return Ok(0.0);
        }
        let md =
            std::fs::metadata(path).context(format_context!("Failed to stat file {}", path))?;
        let t = md
            .modified()
            .context(format_context!("Failed to read modified time for {}", path))?;
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
            std::fs::set_permissions(path, perms).context(format_context!(
                "Failed to set permissions {:o} on {}",
                mode,
                path
            ))?;
            return Ok(NoneType);
        }

        #[cfg(not(unix))]
        {
            let mut perms = std::fs::metadata(path)
                .context(format_context!("Failed to stat path {}", path))?
                .permissions();
            perms.set_readonly((mode & 0o222) == 0);
            std::fs::set_permissions(path, perms)
                .context(format_context!("Failed to set permissions on {}", path))?;
            return Ok(NoneType);
        }
    }

    fn chmod(path: &str, spec: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        #[cfg(unix)]
        {
            let md = std::fs::metadata(path).context(format_context!("Failed to stat {}", path))?;
            let mut mode = md.mode() & 0o7777;

            if spec.len() < 3 {
                anyhow::bail!("Invalid chmod spec '{}': must be [ugoa][+-=][rwx]+", spec);
            }
            let mut chars = spec.chars();
            let who = chars
                .next()
                .context(format_context!("Invalid chmod spec '{}'", spec))?;
            let op = chars
                .next()
                .context(format_context!("Invalid chmod spec '{}'", spec))?;
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
            std::fs::set_permissions(path, perms).context(format_context!(
                "Failed to chmod {} with '{}'",
                path,
                spec
            ))?;
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
                .context(format_context!("Failed to run chown for {}", path))?;
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
        let bytes =
            std::fs::read(path).context(format_context!("Failed to read bytes from {}", path))?;
        Ok(bytes.into_iter().map(u32::from).collect())
    }

    fn write_bytes(path: &str, data: Value) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let json_text = data.to_json().context(format_context!(
            "Failed to convert data argument to JSON for {}",
            path
        ))?;
        let parsed: serde_json::Value = serde_json::from_str(&json_text)
            .context(format_context!("Failed to parse data as JSON for {}", path))?;
        let arr = parsed.as_array().context(format_context!(
            "data must be a list of byte integers for {}",
            path
        ))?;

        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let n = item.as_u64().context(format_context!(
                "data must contain only integer byte values for {}",
                path
            ))?;
            let b = u8::try_from(n).context(format_context!(
                "Byte value out of range 0..255 while writing {}: {}",
                path,
                n
            ))?;
            out.push(b);
        }

        std::fs::write(path, out).context(format_context!("Failed to write bytes to {}", path))?;
        Ok(NoneType)
    }

    fn read_lines(path: &str) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(path)
            .context(format_context!("Failed to read file {}", path))?;
        Ok(content.lines().map(|s| s.to_string()).collect())
    }

    fn write_lines(path: &str, lines: Value) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let json_text = lines.to_json().context(format_context!(
            "Failed to convert lines argument to JSON for {}",
            path
        ))?;
        let parsed: serde_json::Value = serde_json::from_str(&json_text).context(
            format_context!("Failed to parse lines as JSON for {}", path),
        )?;
        let arr = parsed.as_array().context(format_context!(
            "lines must be a list of strings for {}",
            path
        ))?;

        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let s = item.as_str().context(format_context!(
                "lines must contain only strings for {}",
                path
            ))?;
            out.push(s.to_owned());
        }

        let content = if out.is_empty() {
            String::new()
        } else {
            out.join("\n") + "\n"
        };
        std::fs::write(path, content)
            .context(format_context!("Failed to write lines to {}", path))?;
        Ok(NoneType)
    }

    fn write_toml_from_dict(path: &str, value: Value) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let json_text = value.to_json().context(format_context!(
            "Failed to convert Starlark value to JSON for {}",
            path
        ))?;
        let json_value: serde_json::Value = serde_json::from_str(&json_text).context(
            format_context!("Failed to parse JSON representation for {}", path),
        )?;
        let s = toml::to_string_pretty(&json_value)
            .context(format_context!("Failed to serialize TOML for {}", path))?;
        std::fs::write(path, s).context(format_context!("Failed to write TOML file {}", path))?;
        Ok(NoneType)
    }

    fn write_yaml_from_dict(path: &str, value: Value) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        let json_text = value.to_json().context(format_context!(
            "Failed to convert Starlark value to JSON for {}",
            path
        ))?;
        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&json_text).context(
            format_context!("Failed to convert JSON to YAML value for {}", path),
        )?;
        let s = serde_yaml::to_string(&yaml_value)
            .context(format_context!("Failed to serialize YAML for {}", path))?;
        std::fs::write(path, s).context(format_context!("Failed to write YAML file {}", path))?;
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
        let json_text = value.to_json().context(format_context!(
            "Failed to convert Starlark value to JSON for {}",
            path
        ))?;
        let json: serde_json::Value = serde_json::from_str(&json_text).context(format_context!(
            "Failed to parse JSON representation for {}",
            path
        ))?;
        let s = if pretty {
            serde_json::to_string_pretty(&json)
        } else {
            serde_json::to_string(&json)
        }
        .context(format_context!("Failed to serialize JSON for {}", path))?;
        std::fs::write(path, s).context(format_context!("Failed to write JSON file {}", path))?;
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
        std::fs::create_dir_all(parent).context(format_context!(
            "Failed to create parent directory {}",
            parent.display()
        ))?;

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
            let mut f = std::fs::File::create(&tmp_path).context(format_context!(
                "Failed to create temporary file {}",
                tmp_path.display()
            ))?;
            f.write_all(content.as_bytes()).context(format_context!(
                "Failed to write temporary file {}",
                tmp_path.display()
            ))?;
            f.sync_all().context(format_context!(
                "Failed to sync temporary file {}",
                tmp_path.display()
            ))?;
        }

        #[cfg(unix)]
        {
            let perms = std::fs::Permissions::from_mode(mode as u32);
            std::fs::set_permissions(&tmp_path, perms).context(format_context!(
                "Failed to set mode {:o} on temporary file {}",
                mode,
                tmp_path.display()
            ))?;
        }

        std::fs::rename(&tmp_path, dst).context(format_context!(
            "Failed to atomically rename {} -> {}",
            tmp_path.display(),
            dst.display()
        ))?;

        Ok(NoneType)
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
