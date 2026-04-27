use crate::is_lsp_mode;
use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::eval::Evaluator;
use starlark::values::Value;
use starlark::values::none::NoneType;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

// This defines the functions that are visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Gets an environment variable by name.
    ///
    /// ```python
    /// env.get("PATH")                    # -> str | None
    /// env.get("PATH", default="/usr/bin")
    /// ```
    fn get(name: &str, default: Option<String>) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(default.unwrap_or_default());
        }
        match std::env::var(name) {
            Ok(value) => Ok(value),
            Err(std::env::VarError::NotPresent) => Ok(default.unwrap_or_default()),
            Err(std::env::VarError::NotUnicode(_)) => Err(anyhow::anyhow!(
                "Environment variable `{name}` is not valid UTF-8"
            ))
            .context(format_context!("Failed to read environment variable")),
        }
    }

    /// Sets an environment variable.
    ///
    /// ```python
    /// env.set("FOO", "bar")
    /// ```
    fn set(name: &str, value: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        // SAFETY:
        // This crate exposes process-scoped mutability for environment variables by design.
        // Callers are responsible for not racing env mutation across threads.
        unsafe {
            std::env::set_var(name, value);
        }
        Ok(NoneType)
    }

    /// Unsets (removes) an environment variable.
    ///
    /// ```python
    /// env.unset("FOO")
    /// ```
    fn unset(name: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        // SAFETY:
        // This crate exposes process-scoped mutability for environment variables by design.
        // Callers are responsible for not racing env mutation across threads.
        unsafe {
            std::env::remove_var(name);
        }
        Ok(NoneType)
    }

    /// Returns whether an environment variable is present.
    ///
    /// ```python
    /// env.has("CI")
    /// ```
    fn has(name: &str) -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }
        Ok(std::env::var_os(name).is_some())
    }

    /// Returns all environment variables as a dictionary.
    ///
    /// ```python
    /// vars = env.all()   # -> dict[str, str]
    /// ```
    fn all<'v>(eval: &mut Evaluator<'v, '_, '_>) -> anyhow::Result<Value<'v>> {
        if is_lsp_mode() {
            let heap = eval.heap();
            return Ok(heap.alloc(serde_json::Value::Object(serde_json::Map::new())));
        }
        let heap = eval.heap();
        let mut map = serde_json::Map::new();
        for (k, v) in std::env::vars() {
            map.insert(k, serde_json::Value::String(v));
        }
        Ok(heap.alloc(serde_json::Value::Object(map)))
    }

    /// Returns current working directory.
    ///
    /// ```python
    /// env.cwd()
    /// ```
    fn cwd() -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        let dir = std::env::current_dir()
            .context(format_context!("Failed to get current working directory"))?;
        Ok(path_to_string_lossy(&dir))
    }

    /// Changes current working directory.
    ///
    /// ```python
    /// env.chdir("subdir")
    /// ```
    fn chdir(path: &str) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }
        std::env::set_current_dir(path).context(format_context!(
            "Failed to change working directory to `{path}`"
        ))?;
        Ok(NoneType)
    }

    /// Splits PATH into a list of entries.
    ///
    /// ```python
    /// env.path_list()
    /// ```
    fn path_list() -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }
        let raw: OsString = std::env::var_os("PATH").unwrap_or_default();
        Ok(std::env::split_paths(&raw)
            .map(|p| path_to_string_lossy(&p))
            .collect())
    }

    /// Finds the first executable in PATH by command name.
    ///
    /// ```python
    /// env.which("git")  # -> str | None
    /// ```
    fn which(name: &str) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }
        Ok(which_impl(name)
            .map(|p| path_to_string_lossy(&p))
            .unwrap_or_default())
    }

    /// Finds all executable matches in PATH by command name.
    ///
    /// ```python
    /// env.which_all("python")  # -> list[str]
    /// ```
    fn which_all(name: &str) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }
        Ok(which_all_impl(name)
            .into_iter()
            .map(|p| path_to_string_lossy(&p))
            .collect())
    }

    /// Convenience no-op to mirror other modules that expose at least one side-effect-free utility.
    fn _env_module_loaded() -> anyhow::Result<NoneType> {
        Ok(NoneType)
    }
}

fn path_to_string_lossy(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn which_impl(name: &str) -> Option<PathBuf> {
    which_all_impl(name).into_iter().next()
}

fn which_all_impl(name: &str) -> Vec<PathBuf> {
    let candidate = Path::new(name);

    // If name includes path separators, treat it as a direct path probe.
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return if is_executable_file(candidate) {
            vec![candidate.to_path_buf()]
        } else {
            vec![]
        };
    }

    let path_var: OsString = std::env::var_os("PATH").unwrap_or_default();
    if path_var.is_empty() {
        return vec![];
    }

    let mut out = Vec::<PathBuf>::new();

    #[cfg(windows)]
    {
        let pathext = parse_windows_pathext();
        for dir in std::env::split_paths(&path_var) {
            for candidate in windows_candidates(&dir, name, &pathext) {
                if is_executable_file(&candidate) {
                    push_unique(&mut out, candidate);
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                push_unique(&mut out, candidate);
            }
        }
    }

    out
}

fn push_unique(items: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !items.iter().any(|p| p == &candidate) {
        items.push(candidate);
    }
}

#[cfg(not(windows))]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(windows)]
fn is_executable_file(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file(),
        Err(_) => false,
    }
}

#[cfg(windows)]
fn parse_windows_pathext() -> Vec<String> {
    let raw = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());

    raw.split(';')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| {
            if e.starts_with('.') {
                e.to_ascii_lowercase()
            } else {
                format!(".{}", e.to_ascii_lowercase())
            }
        })
        .collect()
}

#[cfg(windows)]
fn windows_candidates(dir: &Path, name: &str, pathext: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let lower_name = name.to_ascii_lowercase();
    let has_known_ext = pathext.iter().any(|ext| lower_name.ends_with(ext));

    if has_known_ext {
        out.push(dir.join(name));
    } else {
        out.push(dir.join(name));
        for ext in pathext {
            out.push(dir.join(format!("{name}{ext}")));
        }
    }

    out
}
