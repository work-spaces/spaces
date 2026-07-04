# Exec-mode error scripts

These scripts intentionally fail to surface specific error paths implemented in `spaces/crates/starstd/src`.

## Running

- Direct: `spaces <path-to-script>`
- Shebang mode: `chmod +x <script>` then execute it directly

## Scripts

- `error-args-invalid-option-kind.exec.star` → `args.rs` (`Unknown option kind`)
- `error-fs-copy-directory-without-recursive.exec.star` → `fs.rs` (`Source is a directory; set recursive=True`)
- `error-fs-invalid-chmod-spec.exec.star` → `fs.rs` (`Invalid chmod spec ...`)
- `error-fs-missing-path.exec.star` → `fs.rs` (`Path does not exist`)
- `error-hash-invalid-base64.exec.star` → `hash.rs` (base64 decode error)
- `error-io-invalid-max-lines.exec.star` → `io.rs` (`max_lines must be a positive integer`)
- `error-io-unsupported-encoding.exec.star` → `io.rs` (`unsupported encoding`)
- `error-json-indent-out-of-range.exec.star` → `json.rs` (`indent must be between 0 and 16`)
- `error-log-invalid-format.exec.star` → `log.rs` (`Invalid log format`)
- `error-log-invalid-level.exec.star` → `log.rs` (`Invalid log level`)
- `error-process-check-nonzero.exec.star` → `process.rs` / `process_error.rs` (formatted non-zero `check=True` failure)
- `error-process-invalid-stdout-mode.exec.star` → `process.rs` (`invalid stdout mode`)
- `error-process-timeout.exec.star` → `process.rs` / `process_error.rs` (formatted timeout)
- `error-process-unknown-handle.exec.star` → `process.rs` (`unknown process handle`)
- `error-process-unsupported-signal.exec.star` → `process.rs` (`unsupported signal`)
- `error-sh-check-nonzero.exec.star` → `sh.rs` / `process_error.rs` (formatted shell non-zero failure)
- `error-string-empty-fill.exec.star` → `string.rs` (`fill must not be empty`)
- `error-string-invalid-regex.exec.star` → `string.rs` (`invalid regex pattern`)
- `error-text-invalid-line-range.exec.star` → `text.rs` (`start must be >= 1`)
- `error-text-invalid-render-format.exec.star` → `text.rs` (`format must be one of: human, github, json, sarif`)
- `error-time-invalid-timer-handle.exec.star` → `time.rs` (`Invalid timer handle`)
- `error-time-parse-failure.exec.star` → `time.rs` (`Failed to parse datetime with format`)
- `error-tmp-unknown-path.exec.star` → `tmp.rs` (`Unknown temp path`)
- `error-toml-null-value.exec.star` → `toml.rs` (`TOML does not support null values`)
- `error-yaml-bad-input.exec.star` → `yaml.rs` (`bad yaml string`)
