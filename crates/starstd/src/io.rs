use crate::is_lsp_mode;
use anyhow::{Context, anyhow};
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use std::io::{IsTerminal, Read, Write};
use std::sync::OnceLock;

static STDIN_BYTES: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();

fn get_stdin_bytes() -> anyhow::Result<&'static [u8]> {
    let cached = STDIN_BYTES.get_or_init(|| {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Failed to read stdin: {e}"))?;
        Ok(bytes)
    });

    match cached {
        Ok(bytes) => Ok(bytes.as_slice()),
        Err(err) => Err(anyhow!(err.clone())).context(format_context!("Failed to read stdin")),
    }
}

fn validate_positive_bound(name: &str, value: Option<i64>) -> anyhow::Result<Option<usize>> {
    match value {
        None => Ok(None),
        Some(v) if v <= 0 => Err(anyhow!(format_context!(
            "{name} must be a positive integer, got {v}"
        ))),
        Some(v) => usize::try_from(v).map(Some).context(format_context!(
            "{name} value {v} is too large for this platform"
        )),
    }
}

fn enforce_max_bytes(bytes: &[u8], max_bytes: Option<usize>) -> anyhow::Result<()> {
    if let Some(max) = max_bytes
        && bytes.len() > max
    {
        return Err(anyhow!(format_context!(
            "stdin exceeded max_bytes: {} bytes > {}",
            bytes.len(),
            max
        )));
    }

    Ok(())
}

fn decode_bytes(bytes: &[u8], encoding: &str) -> anyhow::Result<String> {
    match encoding {
        "utf-8" => Ok(std::str::from_utf8(bytes)
            .context(format_context!(
                "stdin contains invalid UTF-8 (encoding='utf-8'); use encoding='lossy' to replace invalid bytes"
            ))?
            .to_owned()),
        "lossy" => Ok(String::from_utf8_lossy(bytes).into_owned()),
        _ => Err(anyhow!(format_context!(
            "unsupported encoding '{}'; use 'utf-8' or 'lossy'",
            encoding
        ))),
    }
}

fn strip_one_trailing_line_ending(s: &str) -> String {
    if let Some(stripped) = s.strip_suffix("\r\n") {
        stripped.to_owned()
    } else if let Some(stripped) = s.strip_suffix('\n') {
        stripped.to_owned()
    } else {
        s.to_owned()
    }
}

fn split_lines_preserve_terminators(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }

    s.split_inclusive('\n').map(ToOwned::to_owned).collect()
}

fn split_lines_strip_terminators(s: &str) -> Vec<String> {
    split_lines_preserve_terminators(s)
        .into_iter()
        .map(|line| {
            if let Some(stripped) = line.strip_suffix("\r\n") {
                stripped.to_owned()
            } else if let Some(stripped) = line.strip_suffix('\n') {
                stripped.to_owned()
            } else {
                line
            }
        })
        .collect()
}

#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Return whether stdin is attached to a terminal.
    ///
    /// In LSP mode this always returns `False`.
    fn stdin_is_terminal() -> anyhow::Result<bool> {
        if is_lsp_mode() {
            return Ok(false);
        }

        Ok(std::io::stdin().is_terminal())
    }

    /// Read all stdin bytes once and decode to a string.
    ///
    /// The underlying stdin stream is drained at most once per process. Repeated
    /// calls return data decoded from the same cached bytes.
    ///
    /// Note: stdin is fully read and cached before `max_bytes` is enforced, so
    /// `max_bytes` validates size but does not bound memory usage while reading.
    ///
    /// # Arguments
    ///
    /// * `encoding` - `"utf-8"` (strict, default) or `"lossy"`.
    /// * `strip_trailing_newline` - If true, remove one trailing `\n` or `\r\n`.
    /// * `max_bytes` - Optional positive byte limit (checked after stdin is fully
    ///   read and cached). Errors when exceeded.
    ///
    /// # Returns
    ///
    /// The decoded stdin content as `str`.
    fn read_stdin_to_string(
        #[starlark(require = named, default = "utf-8")] encoding: &str,
        #[starlark(require = named, default = false)] strip_trailing_newline: bool,
        #[starlark(require = named)] max_bytes: Option<i64>,
    ) -> anyhow::Result<String> {
        if is_lsp_mode() {
            return Ok(String::new());
        }

        let max_bytes = validate_positive_bound("max_bytes", max_bytes)?;
        let bytes = get_stdin_bytes()?;
        enforce_max_bytes(bytes, max_bytes)?;

        let mut content = decode_bytes(bytes, encoding)?;
        if strip_trailing_newline {
            content = strip_one_trailing_line_ending(&content);
        }

        Ok(content)
    }

    /// Read stdin and split into lines.
    ///
    /// The underlying stdin stream is drained at most once per process. Repeated
    /// calls return lines derived from the same cached bytes.
    ///
    /// Note: stdin is fully read and cached before `max_bytes` is enforced, so
    /// `max_bytes` validates size but does not bound memory usage while reading.
    ///
    /// # Arguments
    ///
    /// * `encoding` - `"utf-8"` (strict, default) or `"lossy"`.
    /// * `strip_newline` - If true (default), strip line terminators.
    /// * `max_lines` - Optional positive line limit. Errors when exceeded.
    /// * `max_bytes` - Optional positive byte limit (checked after stdin is fully
    ///   read and cached). Errors when exceeded.
    ///
    /// # Returns
    ///
    /// A `list[str]` of lines.
    fn read_stdin_lines(
        #[starlark(require = named, default = "utf-8")] encoding: &str,
        #[starlark(require = named, default = true)] strip_newline: bool,
        #[starlark(require = named)] max_lines: Option<i64>,
        #[starlark(require = named)] max_bytes: Option<i64>,
    ) -> anyhow::Result<Vec<String>> {
        if is_lsp_mode() {
            return Ok(Vec::new());
        }

        let max_lines = validate_positive_bound("max_lines", max_lines)?;
        let max_bytes = validate_positive_bound("max_bytes", max_bytes)?;

        let bytes = get_stdin_bytes()?;
        enforce_max_bytes(bytes, max_bytes)?;

        let content = decode_bytes(bytes, encoding)?;
        let lines = if strip_newline {
            split_lines_strip_terminators(&content)
        } else {
            split_lines_preserve_terminators(&content)
        };

        if let Some(max) = max_lines
            && lines.len() > max
        {
            return Err(anyhow!(format_context!(
                "stdin exceeded max_lines: {} lines > {}",
                lines.len(),
                max
            )));
        }

        Ok(lines)
    }

    /// Write text to stdout.
    ///
    /// # Arguments
    ///
    /// * `content` - Text to write.
    /// * `newline` - If true, append `\n` after content.
    fn write_stdout(
        content: &str,
        #[starlark(require = named, default = false)] newline: bool,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(content.as_bytes())
            .context(format_context!("Failed to write to stdout"))?;
        if newline {
            stdout
                .write_all(b"\n")
                .context(format_context!("Failed to write newline to stdout"))?;
        }

        Ok(NoneType)
    }

    /// Write text to stderr.
    ///
    /// # Arguments
    ///
    /// * `content` - Text to write.
    /// * `newline` - If true, append `\n` after content.
    fn write_stderr(
        content: &str,
        #[starlark(require = named, default = false)] newline: bool,
    ) -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        let mut stderr = std::io::stderr().lock();
        stderr
            .write_all(content.as_bytes())
            .context(format_context!("Failed to write to stderr"))?;
        if newline {
            stderr
                .write_all(b"\n")
                .context(format_context!("Failed to write newline to stderr"))?;
        }

        Ok(NoneType)
    }

    /// Flush stdout.
    fn flush_stdout() -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        std::io::stdout()
            .flush()
            .context(format_context!("Failed to flush stdout"))?;
        Ok(NoneType)
    }

    /// Flush stderr.
    fn flush_stderr() -> anyhow::Result<NoneType> {
        if is_lsp_mode() {
            return Ok(NoneType);
        }

        std::io::stderr()
            .flush()
            .context(format_context!("Failed to flush stderr"))?;
        Ok(NoneType)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_bytes_utf8_and_lossy() {
        assert_eq!(decode_bytes("hello".as_bytes(), "utf-8").unwrap(), "hello");

        let invalid = [0xff, b'a'];
        assert!(decode_bytes(&invalid, "utf-8").is_err());
        assert_eq!(decode_bytes(&invalid, "lossy").unwrap(), "�a");
    }

    #[test]
    fn decode_bytes_rejects_unknown_encoding() {
        let err = decode_bytes(b"x", "utf16").unwrap_err().to_string();
        assert!(err.contains("unsupported encoding"));
    }

    #[test]
    fn strip_one_trailing_line_ending_behaves_as_expected() {
        assert_eq!(strip_one_trailing_line_ending("a\n"), "a");
        assert_eq!(strip_one_trailing_line_ending("a\r\n"), "a");
        assert_eq!(strip_one_trailing_line_ending("a\n\n"), "a\n");
        assert_eq!(strip_one_trailing_line_ending("a"), "a");
    }

    #[test]
    fn split_lines_semantics_match_modes() {
        let s = "a\nb\r\nc";
        assert_eq!(
            split_lines_preserve_terminators(s),
            vec!["a\n".to_string(), "b\r\n".to_string(), "c".to_string()]
        );
        assert_eq!(
            split_lines_strip_terminators(s),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );

        assert_eq!(split_lines_preserve_terminators(""), Vec::<String>::new());
        assert_eq!(split_lines_strip_terminators(""), Vec::<String>::new());
    }

    #[test]
    fn validate_positive_bound_rejects_non_positive() {
        assert_eq!(validate_positive_bound("max_bytes", None).unwrap(), None);
        assert_eq!(
            validate_positive_bound("max_bytes", Some(7)).unwrap(),
            Some(7)
        );
        assert!(validate_positive_bound("max_lines", Some(0)).is_err());
        assert!(validate_positive_bound("max_lines", Some(-2)).is_err());
    }

    #[test]
    fn enforce_max_bytes_checks_upper_bound() {
        assert!(enforce_max_bytes(b"abc", Some(3)).is_ok());
        assert!(enforce_max_bytes(b"abc", Some(2)).is_err());
        assert!(enforce_max_bytes(b"abc", None).is_ok());
    }
}
