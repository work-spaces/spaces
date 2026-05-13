use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub enum Driver {
    #[serde(rename = "tar.gz")]
    Gzip,
    #[serde(rename = "tar.bz2")]
    Bzip2,
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.7z")]
    SevenZ,
    #[serde(rename = "tar.xz")]
    Xz,
}

pub(crate) const SEVEN_Z_TAR_FILENAME: &str = "swiss_army_archive_seven7_temp.tar";

/// Stream a file through SHA-256 hasher.
/// Memory usage is constant regardless of file size.
pub(crate) fn stream_sha256_hash(path: &Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let file = std::fs::File::open(path).context(format_context!("{}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536]; // 64 KiB read buffer

    loop {
        let n = reader
            .read(&mut buf)
            .context(format_context!("{}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

impl Driver {
    pub fn extension(&self) -> String {
        match &self {
            Driver::Gzip => "tar.gz".to_string(),
            Driver::Bzip2 => "tar.bz2".to_string(),
            Driver::Zip => "zip".to_string(),
            Driver::SevenZ => "tar.7z".to_string(),
            Driver::Xz => "tar.xz".to_string(),
        }
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "tar.gz" => Some(Driver::Gzip),
            "tar.tgz" => Some(Driver::Gzip),
            "tar.bz2" => Some(Driver::Bzip2),
            "zip" => Some(Driver::Zip),
            "tar.7z" => Some(Driver::SevenZ),
            "tar.xz" => Some(Driver::Xz),
            _ => None,
        }
    }

    pub fn from_filename(filename: &str) -> Option<Self> {
        if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
            Some(Driver::Gzip)
        } else if filename.ends_with(".tar.bz") || filename.ends_with(".tar.bz2") {
            Some(Driver::Bzip2)
        } else if filename.ends_with(".zip") {
            Some(Driver::Zip)
        } else if filename.ends_with(".tar.7z") {
            Some(Driver::SevenZ)
        } else if filename.ends_with(".tar.xz") {
            Some(Driver::Xz)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UpdateStatus {
    pub brief: Option<String>,
    pub detail: Option<String>,
    pub increment: Option<u64>,
    pub total: Option<u64>,
}

pub(crate) fn update_status(progress: &mut console::Progress, update_status: UpdateStatus) {
    if let Some(brief) = update_status.brief {
        progress.set_prefix(brief.as_str());
    }

    if let Some(detail) = update_status.detail {
        progress.set_message(detail.as_str());
    }

    if let Some(total) = update_status.total {
        progress.set_total(Some(total));
        if let Some(increment) = update_status.increment {
            progress.increment_with_overflow(increment);
        }
    } else {
        progress.set_total(Some(100_u64));
        progress.increment_with_overflow(1);
    }
}

pub(crate) fn digest_file(
    file_path: &str,
    progress: &mut console::Progress,
) -> anyhow::Result<String> {
    update_status(
        progress,
        UpdateStatus {
            brief: None,
            detail: Some("Verifying SHA256...".to_string()),
            total: Some(200),
            ..Default::default()
        },
    );

    let file_path = file_path.to_owned();

    let handle = std::thread::spawn(move || -> anyhow::Result<String> {
        stream_sha256_hash(Path::new(&file_path)).context(format_context!("{file_path}"))
    });

    wait_handle(handle, progress).context(format_context!(""))
}

pub(crate) fn wait_handle<OkType>(
    handle: std::thread::JoinHandle<Result<OkType, anyhow::Error>>,
    progress: &mut console::Progress,
) -> anyhow::Result<OkType> {
    while !handle.is_finished() {
        update_status(
            progress,
            UpdateStatus {
                increment: Some(1),
                ..Default::default()
            },
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let result = handle
        .join()
        .map_err(|err| format_error!("failed to join thread: {:?}", err))?;

    result.map_err(|err| format_error!("{:?}", err))
}
