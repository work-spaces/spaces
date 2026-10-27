//! Removing old records from a metrics file.

use super::Record;
use crate::fs_mutex;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PruneReport {
    pub path: PathBuf,
    pub kept: usize,
    pub removed: usize,
    /// Lines that could not be parsed. They are always kept.
    pub unparseable: usize,
    pub dry_run: bool,
}

/// Removes records with `timestamp_ms <= cutoff_ms`, preserving the order of the
/// remaining lines. Unparseable lines are kept because prune must never destroy data it
/// doesn't understand.
///
/// Holds the same lock as the writer (`telemetry::append_record`), so concurrent appends
/// are never lost. The kept lines are written to `<file>.tmp`, which then replaces the file.
pub fn prune_file(path: &Path, cutoff_ms: u64, dry_run: bool) -> anyhow::Result<PruneReport> {
    let mut mutex = fs_mutex::FsMutex::new(path.into());
    mutex.with_lock(|path| {
        let contents = std::fs::read(path).context(format_context!(
            "Failed to read metrics file {}",
            path.display()
        ))?;

        let mut report = PruneReport {
            path: path.to_path_buf(),
            kept: 0,
            removed: 0,
            unparseable: 0,
            dry_run,
        };
        let mut kept: Vec<&[u8]> = Vec::new();
        for line in contents.split(|byte| *byte == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<Record>(line) {
                Ok(record) if record.timestamp_ms <= cutoff_ms => report.removed += 1,
                Ok(_) => {
                    report.kept += 1;
                    kept.push(line);
                }
                Err(_) => {
                    report.unparseable += 1;
                    kept.push(line);
                }
            }
        }

        if dry_run || report.removed == 0 {
            return Ok(report);
        }

        let mut tmp_name = path.as_os_str().to_owned();
        tmp_name.push(".tmp");
        let tmp_path = PathBuf::from(tmp_name);
        let write_tmp = || -> std::io::Result<()> {
            let mut file = std::io::BufWriter::new(std::fs::File::create(&tmp_path)?);
            for line in &kept {
                file.write_all(line)?;
                file.write_all(b"\n")?;
            }
            file.into_inner()?.sync_all()
        };
        write_tmp().context(format_context!(
            "Failed to write pruned metrics to {}",
            tmp_path.display()
        ))?;
        std::fs::rename(&tmp_path, path).context(format_context!(
            "Failed to replace {} with {}",
            path.display(),
            tmp_path.display()
        ))?;
        Ok(report)
    })
}

#[cfg(test)]
mod tests {
    use super::super::read_records;
    use super::super::tests::sample_record;
    use super::*;

    fn write_file(path: &Path) {
        let mut contents = String::new();
        for (timestamp, command) in [(1, "a"), (5, "b"), (2, "c"), (10, "d")] {
            contents.push_str(
                &serde_json::to_string(&sample_record(timestamp, command, &[1.0])).unwrap(),
            );
            contents.push('\n');
            if timestamp == 5 {
                contents.push_str("garbage line\n");
            }
        }
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn test_prune_keeps_unparseable_and_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spaces.perf.jsonl");
        write_file(&path);

        let report = prune_file(&path, 2, false).unwrap();
        assert_eq!(report.removed, 2);
        assert_eq!(report.kept, 2);
        assert_eq!(report.unparseable, 1);

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1], "garbage line");
        let outcome = read_records(&path).unwrap();
        let commands: Vec<&str> = outcome.records.iter().map(|r| r.command.as_str()).collect();
        assert_eq!(commands, vec!["b", "d"]);
        assert_eq!(outcome.skipped, 1);
        assert!(!dir.path().join("spaces.perf.jsonl.tmp").exists());
    }

    #[test]
    fn test_prune_dry_run_does_not_modify() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spaces.perf.jsonl");
        write_file(&path);
        let before = std::fs::read(&path).unwrap();

        let report = prune_file(&path, 100, true).unwrap();
        assert!(report.dry_run);
        assert_eq!(report.removed, 4);
        assert_eq!(report.kept, 0);
        assert_eq!(report.unparseable, 1);
        assert_eq!(std::fs::read(&path).unwrap(), before);

        // nothing to remove: file is untouched
        let report = prune_file(&path, 0, false).unwrap();
        assert_eq!(report.removed, 0);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
