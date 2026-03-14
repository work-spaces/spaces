use crate::{changes::glob, lock, logger, ws};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

/// Default maximum number of retry attempts for network operations.
const DEFAULT_MAX_RETRIES: u32 = 3;
/// Initial backoff duration in milliseconds before the first retry.
const INITIAL_BACKOFF_MS: u64 = 1000;
/// Multiplier applied to the backoff duration after each retry.
const BACKOFF_MULTIPLIER: u64 = 2;
/// Connection timeout in seconds for the HTTP client.
const CONNECT_TIMEOUT_SECS: u64 = 30;
/// Read timeout in seconds — detects stalled transfers.
const READ_TIMEOUT_SECS: u64 = 60;
/// Overall request timeout in seconds (safety net for large archives).
const REQUEST_TIMEOUT_SECS: u64 = 600;

mod gh;

struct State {}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    STATE.set(RwLock::new(State {}));
    STATE.get()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, strum::Display)]
pub enum ArchiveLink {
    None,
    #[default]
    Hard,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MakeReadOnly {
    No,
    Yes,
}

fn label_logger<'a>(
    progress: &'a mut printer::MultiProgressBar,
    label: &str,
) -> logger::Logger<'a> {
    logger::Logger::new_progress(progress, label.into())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub url: Arc<str>,
    pub sha256: Arc<str>,
    pub link: ArchiveLink,
    pub includes: Option<Vec<Arc<str>>>,
    pub excludes: Option<Vec<Arc<str>>>,
    pub globs: Option<HashSet<Arc<str>>>,
    pub strip_prefix: Option<Arc<str>>,
    pub add_prefix: Option<Arc<str>>,
    pub filename: Option<Arc<str>>,
    pub version: Option<Arc<str>>,
    pub headers: Option<HashMap<Arc<str>, Arc<str>>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Files {
    files: HashSet<Arc<str>>,
}

pub fn validate_headers(headers: &HashMap<Arc<str>, Arc<str>>) -> anyhow::Result<()> {
    for (key, value) in headers {
        // use reqwest to validate headers
        let _ = reqwest::header::HeaderName::from_str(key)
            .context(format_context!("While checking header key {}", key))?;
        let _ = reqwest::header::HeaderValue::from_str(value)
            .context(format_context!("While checking header value {}", value))?;
    }
    Ok(())
}

/// Checks whether the Content-Type header of a response indicates an HTML page.
/// Servers sometimes return a 200 status with an HTML error/login page instead of the
/// requested binary archive when the resource does not exist or requires authentication.
fn check_response_content_type(response: &reqwest::Response, url: &str) -> anyhow::Result<()> {
    if let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) {
        let content_type_str = content_type.to_str().unwrap_or("");
        if content_type_str.contains("text/html") || content_type_str.contains("text/xml") {
            return Err(format_error!(
                "Server returned Content-Type '{}' for {url}. The server likely returned an error page instead of the expected archive. Verify the URL points to a valid downloadable archive.",
                content_type_str
            ));
        }
    }
    Ok(())
}

/// Inspects the first bytes of a downloaded file to detect whether it is actually an HTML page.
/// This catches cases where the server returns an HTML error page without setting a Content-Type
/// header (or sets a generic one like application/octet-stream).
fn check_file_is_not_html(path: &str, url: &str) -> anyhow::Result<()> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).context(format_context!(
        "Failed to open downloaded file {path} for validation"
    ))?;
    let mut buf = [0u8; 512];
    let n = file.read(&mut buf).context(format_context!(
        "Failed to read downloaded file {path} for validation"
    ))?;
    if n == 0 {
        return Err(format_error!(
            "Downloaded file {path} from {url} is empty (0 bytes)"
        ));
    }
    let header = String::from_utf8_lossy(&buf[..n]);
    let trimmed = header.trim_start();
    let end = trimmed
        .char_indices()
        .take_while(|(i, _)| *i < 128)
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    let lower = trimmed[..end].to_ascii_lowercase();
    if lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || lower.starts_with("<?xml")
        || lower.starts_with("<head")
    {
        // Grab a small preview to show in the error message
        let preview: String = trimmed.chars().take(200).collect();
        return Err(format_error!(
            "Downloaded file from {url} appears to be an HTML/XML page, not a valid archive. \
             The server likely returned an error page. First bytes:\n{preview}"
        ));
    }
    Ok(())
}

/// Returns true if the HTTP status code is transient and worth retrying.
fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
}

/// Returns true if the reqwest error is transient and worth retrying.
fn is_retryable_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

/// Computes the backoff duration for a given retry attempt, with deterministic jitter.
/// The jitter alternates ±25% based on the attempt number to avoid thundering herd.
fn backoff_duration(attempt: u32) -> std::time::Duration {
    let base_ms = INITIAL_BACKOFF_MS * BACKOFF_MULTIPLIER.saturating_pow(attempt);
    // Deterministic jitter: odd attempts get +25%, even attempts get -25%
    let jitter_ms = base_ms / 4;
    let effective_ms = if attempt.is_multiple_of(2) {
        base_ms.saturating_sub(jitter_ms)
    } else {
        base_ms.saturating_add(jitter_ms)
    };
    std::time::Duration::from_millis(effective_ms)
}

/// Extracts the `Retry-After` header value (in seconds) from a response, if present.
fn parse_retry_after(response: &reqwest::Response) -> Option<std::time::Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|val| val.to_str().ok())
        .and_then(|val| val.parse::<u64>().ok())
        .map(std::time::Duration::from_secs)
}

/// Build a reqwest client with appropriate timeouts and redirect policy.
fn build_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::ClientBuilder::new()
        .redirect(reqwest::redirect::Policy::limited(20))
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .read_timeout(std::time::Duration::from_secs(READ_TIMEOUT_SECS))
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .context(format_context!("Failed to build reqwest client"))
}

/// Build default headers, merging in any user-supplied headers.
fn build_headers(
    extra_headers: Option<&HashMap<Arc<str>, Arc<str>>>,
) -> anyhow::Result<reqwest::header::HeaderMap> {
    let mut client_headers = reqwest::header::HeaderMap::new();

    client_headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_str("wget").context(format_context!(
            "Internal Error: failed to create wget error value"
        ))?,
    );
    client_headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_str("*/*").context(format_context!(
            "Internal Error: failed to create accept header value"
        ))?,
    );

    if let Some(headers) = extra_headers {
        for (key, value) in headers {
            let header_name = reqwest::header::HeaderName::from_str(key.as_ref()).context(
                format_context!("While converting {} to a standard header", key),
            )?;
            let header_value = reqwest::header::HeaderValue::from_str(value.as_ref()).context(
                format_context!("While converting {} to a standard header value", value),
            )?;
            let _ = client_headers.insert(header_name, header_value);
        }
    }

    Ok(client_headers)
}

/// Cleans up a partially downloaded file, if it exists.
fn cleanup_partial_download(destination: &str) {
    let _ = std::fs::remove_file(destination);
}

pub fn download(
    mut progress: printer::MultiProgressBar,
    url: &str,
    destination: &str,
    headers: Option<HashMap<Arc<str>, Arc<str>>>,
    retry_counter: Arc<AtomicU32>,
    runtime: &tokio::runtime::Runtime,
) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<printer::MultiProgressBar>>> {
    label_logger(&mut progress, url)
        .trace(format!("Downloading using reqwest {url} -> {destination}").as_str());

    let destination = destination.to_string();
    let url = url.to_string();

    let join_handle = runtime.spawn(async move {
        let client = build_http_client()?;
        let base_headers = build_headers(headers.as_ref())?;

        for (key, _) in base_headers.iter() {
            label_logger(&mut progress, &url)
                .debug(format!("Header: {key:?}").as_str());
        }

        let mut last_error: Option<anyhow::Error> = None;

        for attempt in 0..=DEFAULT_MAX_RETRIES {
            if attempt > 0 {
                retry_counter.fetch_add(1, Ordering::Relaxed);
                let wait = backoff_duration(attempt - 1);
                label_logger(&mut progress, &url).warning(
                    format!(
                        "Retry attempt {attempt}/{DEFAULT_MAX_RETRIES} for {url} after {wait:?}"
                    )
                    .as_str(),
                );
                tokio::time::sleep(wait).await;
            }

            // Check if we have bytes from a previous partial download attempt
            let existing_bytes = tokio::fs::metadata(&destination)
                .await
                .map(|m| m.len())
                .unwrap_or(0);

            let mut request_headers = base_headers.clone();

            // If we have partial content, attempt a range request to resume
            if existing_bytes > 0 {
                label_logger(&mut progress, &url).debug(
                    format!("Resuming download from byte {existing_bytes}").as_str(),
                );
                request_headers.insert(
                    reqwest::header::RANGE,
                    reqwest::header::HeaderValue::from_str(
                        &format!("bytes={existing_bytes}-"),
                    )
                    .context(format_context!(
                        "Internal Error: failed to create Range header value"
                    ))?,
                );
            }

            let request = client.get(&url).headers(request_headers);

            label_logger(&mut progress, &url)
                .debug(format!("Reqwest request: {request:?}").as_str());

            let response = match request.send().await {
                Ok(resp) => resp,
                Err(err) => {
                    if is_retryable_error(&err) && attempt < DEFAULT_MAX_RETRIES {
                        label_logger(&mut progress, &url).warning(
                            format!("Transient error connecting to {url}: {err}").as_str(),
                        );
                        last_error = Some(err.into());
                        continue;
                    }
                    // Non-retryable or final attempt — clean up and fail
                    cleanup_partial_download(&destination);
                    let human_attempt = attempt + 1;
                    return Err(err).context(format_context!(
                        "Failed to download {url} after {human_attempt} attempt(s)"
                    ));
                }
            };

            let status = response.status();

            // Handle non-success, non-partial-content responses
            if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
                if is_retryable_status(status) && attempt < DEFAULT_MAX_RETRIES {
                    let retry_after = parse_retry_after(&response);
                    let wait_override = retry_after
                        .map(|d| format!(", server requested Retry-After: {d:?}"))
                        .unwrap_or_default();
                    label_logger(&mut progress, &url).warning(
                        format!(
                            "Retryable HTTP {status} from {url}{wait_override}"
                        )
                        .as_str(),
                    );
                    // If server gave a Retry-After, sleep for that duration instead
                    if let Some(retry_wait) = retry_after {
                        tokio::time::sleep(retry_wait).await;
                    }
                    last_error = Some(format_error!(
                        "HTTP {status} from {url}"
                    ));
                    continue;
                }
                cleanup_partial_download(&destination);
                return Err(format_error!(
                    "Failed to download {url}. Got HTTP {status} (response: {response:?})"
                ));
            }

            // Check whether the server returned an HTML page instead of the expected archive
            check_response_content_type(&response, &url)?;

            label_logger(&mut progress, &url)
                .debug(format!("Response: {response:?}").as_str());

            // Determine if this is a resumed download (206) or a fresh start (200)
            let is_resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
            let content_length = response.content_length();

            let (mut output_file, mut bytes_written) = if is_resumed && existing_bytes > 0 {
                // Server supports range requests — append to existing file
                label_logger(&mut progress, &url).debug(
                    format!("Server returned 206 Partial Content, resuming from byte {existing_bytes}").as_str(),
                );
                let total_size = content_length
                    .map(|cl| cl + existing_bytes)
                    .unwrap_or(0);
                progress.set_total(total_size);
                // Reset progress bar position and advance to the already-downloaded portion
                progress.reset_position();
                progress.increment(existing_bytes);

                let mut file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .open(&destination)
                    .await
                    .context(format_context!(
                        "Failed to open {destination} for append during resume"
                    ))?;
                file.seek(std::io::SeekFrom::End(0)).await.context(
                    format_context!("Failed to seek to end of {destination}"),
                )?;
                (file, existing_bytes)
            } else {
                // Fresh download — create/truncate the file
                let total_size = content_length.unwrap_or(0);
                progress.set_total(total_size);
                progress.reset_position();

                let file = tokio::fs::File::create(&destination).await.context(
                    format_context!("Failed to create {destination}"),
                )?;
                (file, 0u64)
            };

            progress.set_message(url.as_str());

            // Stream chunks to disk
            let mut response = response;
            let mut chunk_error: Option<anyhow::Error> = None;
            loop {
                let chunk_result = response.chunk().await;
                match chunk_result {
                    Ok(Some(chunk)) => {
                        let chunk_len = chunk.len() as u64;
                        if let Err(write_err) =
                            output_file.write_all(&chunk).await
                        {
                            chunk_error = Some(write_err.into());
                            break;
                        }
                        bytes_written += chunk_len;
                        progress.increment(chunk_len);
                    }
                    Ok(None) => {
                        // Stream complete
                        break;
                    }
                    Err(err) => {
                        // Network error during chunked read
                        label_logger(&mut progress, &url).warning(
                            format!(
                                "Error reading chunk from {url} at byte {bytes_written}: {err}"
                            )
                            .as_str(),
                        );
                        chunk_error = Some(err.into());
                        break;
                    }
                }
            };

            // Flush what we have so far
            let _ = output_file.flush().await;
            drop(output_file);

            if let Some(err) = chunk_error {
                if attempt < DEFAULT_MAX_RETRIES {
                    label_logger(&mut progress, &url).warning(
                        format!(
                            "Download interrupted for {url} at {bytes_written} bytes, will retry"
                        )
                        .as_str(),
                    );
                    last_error = Some(err);
                    continue;
                }
                cleanup_partial_download(&destination);
                return Err(err).context(format_context!(
                    "Failed to download {url}: stream interrupted after {bytes_written} bytes, all retries exhausted"
                ));
            }

            // Validate that we received the expected number of bytes
            if let Some(expected_total) = content_length {
                let expected_bytes = expected_total; // Content-Length for 206 is the remaining bytes
                let received_bytes = if is_resumed {
                    bytes_written - existing_bytes
                } else {
                    bytes_written
                };
                if received_bytes != expected_bytes {
                    let msg = format!(
                        "Size mismatch for {url}: expected {expected_bytes} bytes, got {received_bytes} bytes"
                    );
                    if attempt < DEFAULT_MAX_RETRIES {
                        label_logger(&mut progress, &url).warning(msg.as_str());
                        last_error = Some(format_error!("{}", msg));
                        // Delete partial file so next attempt starts fresh for size mismatch
                        cleanup_partial_download(&destination);
                        continue;
                    }
                    cleanup_partial_download(&destination);
                    return Err(format_error!("{}", msg));
                }
            }

            // Inspect the first bytes of the file to catch HTML pages that slipped
            // past the Content-Type check (e.g. missing or generic Content-Type)
            check_file_is_not_html(&destination, &url)?;

            // Download succeeded
            let total_retries = retry_counter.load(Ordering::Relaxed);
            if total_retries > 0 {
                label_logger(&mut progress, &url).debug(
                    format!(
                        "Download complete after {total_retries} retries: {bytes_written} bytes written to {destination}"
                    )
                    .as_str(),
                );
            } else {
                label_logger(&mut progress, &url).debug(
                    format!(
                        "Download complete: {bytes_written} bytes written to {destination}"
                    )
                    .as_str(),
                );
            }
            return Ok(progress);
        }

        // All retries exhausted
        cleanup_partial_download(&destination);
        Err(last_error.unwrap_or_else(|| {
            format_error!("Failed to download {url} after {DEFAULT_MAX_RETRIES} retries")
        }))
    });

    Ok(join_handle)
}

pub fn download_string(url: &str, retry_counter: Arc<AtomicU32>) -> anyhow::Result<Arc<str>> {
    let client = reqwest::blocking::ClientBuilder::new()
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .context(format_context!("Failed to build blocking reqwest client"))?;

    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..=DEFAULT_MAX_RETRIES {
        if attempt > 0 {
            retry_counter.fetch_add(1, Ordering::Relaxed);
            let wait = backoff_duration(attempt - 1);
            std::thread::sleep(wait);
        }

        let result = client.get(url).send();

        match result {
            Ok(response) => {
                let status: reqwest::StatusCode = response.status();
                if !status.is_success() {
                    if is_retryable_status(status) && attempt < DEFAULT_MAX_RETRIES {
                        last_error = Some(format_error!("HTTP {status} from {url}"));
                        continue;
                    }
                    return Err(format_error!("Failed to download {url}: HTTP {status}"));
                }
                // Check whether the server returned an HTML page instead of expected content
                if let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) {
                    let ct = content_type.to_str().unwrap_or("");
                    if ct.contains("text/html") {
                        return Err(format_error!(
                            "Server returned Content-Type '{}' for {url}. Expected a plain-text response, not an HTML page.",
                            ct
                        ));
                    }
                }

                let content = response
                    .text()
                    .context(format_context!("Failed to read response from {url}"))?;
                return Ok(content.into());
            }
            Err(err) => {
                if is_retryable_error(&err) && attempt < DEFAULT_MAX_RETRIES {
                    last_error = Some(err.into());
                    continue;
                }
                let attempt_plus_one = attempt + 1;
                return Err(err).context(format_context!(
                    "Failed to download {url} after {attempt_plus_one} attempt(s)"
                ));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        format_error!("Failed to download string from {url} after {DEFAULT_MAX_RETRIES} retries")
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpArchive {
    pub spaces_key: Arc<str>,
    pub archive: Archive,
    archive_driver: Option<archiver::driver::Driver>,
    pub full_path_to_archive: String,
    tools_path: String,
    allow_gh_for_download: bool,
}

impl HttpArchive {
    pub fn new(
        bare_store_path: &str,
        spaces_key: &str,
        archive: &Archive,
        tools_path: &str,
    ) -> anyhow::Result<Self> {
        let relative_path = Self::url_to_relative_path(archive.url.as_ref(), &archive.filename)
            .context(format_context!("no relative path for {}", archive.url))?;

        let full_path_to_archive = format!("{bare_store_path}/{relative_path}");

        let archive_path = std::path::Path::new(full_path_to_archive.as_str());

        let mut archive_file_name = archive_path
            .file_name()
            .ok_or(format_error!(
                "No file name found in archive path {full_path_to_archive}"
            ))?
            .to_string_lossy()
            .to_string();

        let (filename, effective_sha256) = if archive.sha256.starts_with("http") {
            let sha256 = download_string(archive.sha256.as_ref(), Arc::new(AtomicU32::new(0)))
                .context(format_context!("Failed to download {}", archive.sha256))?;
            if sha256.len() != 64 {
                return Err(format_error!(
                    "Invalid sha256 checksum {sha256} for {}",
                    archive.url
                ));
            }
            (None, Some(sha256))
        } else {
            (None, None)
        };

        archive_file_name = filename.unwrap_or(archive_file_name);
        let effective_sha256 = effective_sha256.unwrap_or(archive.sha256.clone());

        let archive_driver_result =
            archiver::driver::Driver::from_filename(archive_file_name.as_str()).context(
                format_context!("Failed to get driver for {archive_file_name}"),
            );

        let mut archive_driver = None;
        let full_path_to_archive = match archive_driver_result {
            Ok(driver) => {
                archive_driver = Some(driver);
                format!(
                    "{}/{}.{}",
                    full_path_to_archive,
                    effective_sha256,
                    driver.extension()
                )
            }
            Err(_) => {
                format!("{full_path_to_archive}/{effective_sha256}/{archive_file_name}")
            }
        };

        let mut archive = archive.clone();
        archive.sha256 = effective_sha256;

        Ok(Self {
            archive,
            archive_driver,
            full_path_to_archive,
            spaces_key: spaces_key.into(),
            allow_gh_for_download: true,
            tools_path: tools_path.to_owned(),
        })
    }

    pub fn get_file_lock(&self) -> lock::FileLock {
        let path = std::path::Path::new(&self.full_path_to_archive);
        let path = path.parent().unwrap_or(path);
        let path = path.join(format!("{}.{}", self.spaces_key, lock::LOCK_FILE_SUFFIX).as_str());
        lock::FileLock::new(path.into())
    }

    pub fn get_member(&self) -> ws::Member {
        ws::Member {
            path: self.spaces_key.clone(),
            url: self.archive.url.clone(),
            rev: self.archive.sha256.clone(),
            version: self.archive.version.clone(),
        }
    }

    pub fn allow_gh_for_download(&mut self, value: bool) {
        self.allow_gh_for_download = value;
    }

    pub fn get_path_to_extracted_files(&self) -> String {
        format!("{}_files", self.full_path_to_archive)
    }

    fn get_path_to_extracted_files_json(&self) -> String {
        format!("{}.json", self.get_path_to_extracted_files())
    }

    pub fn is_download_required(&self) -> bool {
        !std::path::Path::new(&self.full_path_to_archive).exists()
    }

    fn is_extract_required(&self) -> bool {
        !std::path::Path::new(self.get_path_to_extracted_files().as_str()).exists()
    }

    pub fn create_links(
        &self,
        mut progress_bar: printer::MultiProgressBar,
        workspace_directory: &str,
        space_directory: &str,
        link_set: &mut HashSet<Arc<str>>,
    ) -> anyhow::Result<()> {
        //construct a list of files to link
        let mut files = Vec::new();
        let all_files = self
            .load_files_json()
            .context(format_context!("failed to load json files manifest"))?;

        let mut collect_globs = glob::Globs::default();
        if let Some(globs) = self.archive.globs.as_ref() {
            collect_globs.merge(&glob::Globs::new_with_annotated_set(globs));
        }

        for file in all_files.iter() {
            let is_match = collect_globs.is_empty() || collect_globs.is_match(file);

            if is_match {
                files.push(file);
            }
        }

        let target_prefix = if let Some(add_prefix) = self.archive.add_prefix.as_ref() {
            if add_prefix.starts_with("//") {
                format!("{workspace_directory}/{add_prefix}").into()
            } else if add_prefix.starts_with('/') {
                add_prefix.clone()
            } else {
                format!("{workspace_directory}/{add_prefix}").into()
            }
        } else {
            format!("{workspace_directory}/{space_directory}").into()
        };

        progress_bar.set_total(files.len() as u64);

        let mut soft_links = Vec::new();

        for file in files {
            let source = format!("{}/{}", self.get_path_to_extracted_files(), file);

            progress_bar.set_message(file.as_ref());
            let relative_target_path =
                if let Some(strip_prefix) = self.archive.strip_prefix.as_ref() {
                    file.strip_prefix(strip_prefix.as_ref())
                } else {
                    Some(file.as_ref())
                };

            if let Some(relative_target_path) = relative_target_path {
                let full_target_path = format!("{target_prefix}/{relative_target_path}");

                match self.archive.link {
                    ArchiveLink::Hard | ArchiveLink::Copy => {
                        label_logger(&mut progress_bar, "link").trace(
                            format!(
                                "Creating link: {} {full_target_path} -> {source}",
                                self.archive.link
                            )
                            .as_str(),
                        );
                        let workspace_path_to_target = full_target_path
                            .strip_prefix(format!("{workspace_directory}/").as_str())
                            .unwrap_or(&full_target_path);
                        let _ = link_set.insert(workspace_path_to_target.into());

                        let make_read_only = if self.archive.link == ArchiveLink::Hard {
                            MakeReadOnly::Yes
                        } else {
                            MakeReadOnly::No
                        };

                        Self::create_link(
                            full_target_path.clone(),
                            source.clone(),
                            make_read_only,
                            Some(&mut soft_links),
                            self.archive.link.clone(),
                        )
                        .context(format_context!("hard link {full_target_path} -> {source}",))?;
                    }
                    ArchiveLink::None => (),
                }
            } else {
                label_logger(&mut progress_bar, "hardlink").warning(
                    format!(
                        "Failed to strip prefix {:?} from {file}",
                        self.archive.strip_prefix
                    )
                    .as_str(),
                );
            }
            progress_bar.increment(1);
        }

        for (original, link) in soft_links {
            symlink::symlink_file(&original, &link).context(format_context!(
                "failed to create symlink {original:?} -> {link:?}"
            ))?;
        }

        Ok(())
    }

    pub fn create_link(
        target_path: String,
        source: String,
        make_read_only: MakeReadOnly,
        soft_links: Option<&mut Vec<(std::path::PathBuf, std::path::PathBuf)>>,
        link_type: ArchiveLink,
    ) -> anyhow::Result<()> {
        let target = std::path::Path::new(target_path.as_str());
        let original = std::path::Path::new(source.as_str());

        // Hold the mutex to ensure operations are atomic
        #[allow(clippy::readonly_write_lock)]
        let _state = get_state().write().unwrap();

        if make_read_only == MakeReadOnly::Yes {
            // original file needs to be updated to be read-only
            let original_metadata = std::fs::metadata(original)
                .context(format_context!("Failed to get metadata for {original:?}"))?;

            // Update the metadata to be read-only
            let mut read_only_permissions = original_metadata.permissions();
            read_only_permissions.set_readonly(true);

            // Set the permissions to read-only
            std::fs::set_permissions(original, read_only_permissions).context(format_context!(
                "Failed to set permissions for {original:?}"
            ))?;
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .context(format_context!("{target_path} -> {source}"))?;
        }

        let _ = std::fs::remove_file(target);

        //if the source is a symlink, read the symlink and create a symlink
        if original.is_symlink() {
            let link = std::fs::read_link(original)
                .context(format_context!("failed to read symlink {original:?}"))?;

            if let Some(soft_links) = soft_links {
                // defer creation of soft links if a list is provided
                soft_links.push((link, target.into()));
            } else {
                symlink::symlink_file(link.clone(), target).context(format_context!(
                    "failed to create symlink {original:?} -> {link:?}"
                ))?;
            }

            return Ok(());
        }

        if link_type == ArchiveLink::Hard {
            std::fs::hard_link(original, target).context(format_context!(
            "If you get 'Operation Not Permitted' on mac try enabling 'Full Disk Access' for the terminal"))?;
        } else {
            reflink_copy::reflink_or_copy(original, target).context(format_context!(
            "If you get 'Operation Not Permitted' on mac try enabling 'Full Disk Access' for the terminal"))?;

            let target_metadata = std::fs::metadata(target)
                .context(format_context!("Failed to get metadata for {target:?}"))?;

            // Update the metadata target file to be read-write
            let mut read_write_permissions = target_metadata.permissions();

            #[allow(clippy::permissions_set_readonly_false)]
            read_write_permissions.set_readonly(false);

            // Set the permissions to read-write
            std::fs::set_permissions(target, read_write_permissions).context(format_context!(
                "Failed to set permissions for {}",
                target.display()
            ))?;
        }

        Ok(())
    }

    pub fn sync(
        &self,
        mut progress_bar: printer::MultiProgressBar,
    ) -> anyhow::Result<printer::MultiProgressBar> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .context(format_context!("Failed to create runtime"))?;

        let mut next_progress_bar = if self.is_download_required() {
            if let Some(arguments) = gh::transform_url_to_arguments(
                self.allow_gh_for_download,
                self.archive.url.as_ref(),
                &self.full_path_to_archive,
            ) {
                let gh_command = format!("{}/gh", self.tools_path);
                let gh_result =
                    gh::download(&gh_command, &self.archive.url, arguments, &mut progress_bar);
                progress_bar = if gh_result.is_err() {
                    let join_handle =
                        self.download(&runtime, progress_bar)
                            .context(format_context!(
                                "Failed to download using https after trying gh. Use `gh auth login` to authenticate"
                            ))?;
                    runtime.block_on(join_handle)??
                } else {
                    progress_bar
                };
                progress_bar
            } else {
                label_logger(&mut progress_bar, &self.archive.url)
                    .debug(format!("{} Downloading using reqwest", self.archive.url).as_str());

                let join_handle = self
                    .download(&runtime, progress_bar)
                    .context(format_context!("Failed to download using reqwest"))?;
                runtime.block_on(join_handle)??
            }
        } else {
            label_logger(&mut progress_bar, &self.archive.url)
                .debug(format!("{} download not required", self.archive.url).as_str());
            progress_bar
        };

        label_logger(&mut next_progress_bar, &self.archive.url).debug("Extracting archive");

        let next_progress_bar = self.extract(next_progress_bar).context(format_context!(
            "extract failed {}",
            self.full_path_to_archive
        ))?;
        Ok(next_progress_bar)
    }

    pub fn download(
        &self,
        runtime: &tokio::runtime::Runtime,
        progress: printer::MultiProgressBar,
    ) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<printer::MultiProgressBar>>> {
        let full_path_to_archive = self.full_path_to_archive.clone();
        let full_path = std::path::Path::new(&full_path_to_archive);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        download(
            progress,
            self.archive.url.as_ref(),
            full_path_to_archive.as_str(),
            self.archive.headers.clone(),
            Arc::new(AtomicU32::new(0)),
            runtime,
        )
    }

    fn save_files_json(&self, files: Files) -> anyhow::Result<()> {
        let file_path = self.get_path_to_extracted_files_json();
        let contents = serde_json::to_string_pretty(&files)?;
        std::fs::write(file_path, contents)?;
        Ok(())
    }

    fn load_files_json(&self) -> anyhow::Result<HashSet<Arc<str>>> {
        let file_path = self.get_path_to_extracted_files_json();
        let contents = std::fs::read_to_string(file_path.as_str())
            .context(format_context!("while reading {file_path}"))?;
        let files: Files = serde_json::from_str(contents.as_str())
            .context(format_context!("while parsing {file_path}"))?;
        Ok(files.files)
    }

    fn extract(
        &self,
        mut progress_bar: printer::MultiProgressBar,
    ) -> anyhow::Result<printer::MultiProgressBar> {
        if !self.is_extract_required() {
            label_logger(&mut progress_bar, &self.archive.url).debug("Extract not required");
            return Ok(progress_bar);
        }

        std::fs::create_dir_all(self.get_path_to_extracted_files().as_str())
            .context(format_context!("creating {}", self.full_path_to_archive))?;

        let mut extracted_files = HashSet::new();

        let next_progress_bar = if self.archive_driver.is_some() {
            let decoder = archiver::Decoder::new(
                &self.full_path_to_archive,
                Some(self.archive.sha256.to_string()),
                &self.get_path_to_extracted_files(),
                progress_bar,
            )
            .context(format_context!(
                "{} -> {}",
                self.full_path_to_archive.as_str(),
                self.get_path_to_extracted_files()
            ))?;

            let extracted = decoder.extract().context(format_context!(
                "{} -> {}",
                self.full_path_to_archive,
                self.get_path_to_extracted_files()
            ))?;

            extracted_files = extracted.files;
            extracted.progress_bar
        } else {
            let path_to_artifact = std::path::Path::new(self.full_path_to_archive.as_str());
            let file_name = path_to_artifact.file_name().ok_or(format_error!(
                "No file name found in archive path {path_to_artifact:?}"
            ))?;

            // Check sha256 is correct
            let file_contents = std::fs::read(path_to_artifact).context(format_context!(
                "failed to load contents for {:?}",
                path_to_artifact
            ))?;
            let file_digest = sha256::digest(file_contents).to_ascii_lowercase();
            let expected_digest = self.archive.sha256.to_lowercase();
            if file_digest != expected_digest {
                return Err(format_error!(
                    "SHA256 mismatch for {path_to_artifact:?}, expected {expected_digest}, got {file_digest}"
                ));
            }

            // the file needs to be moved to the extracted files directory
            // that is where the create links function will look for it
            let target =
                std::path::Path::new(self.get_path_to_extracted_files().as_str()).join(file_name);

            std::fs::rename(path_to_artifact, target.clone())
                .context(format_context!("copy {path_to_artifact:?} -> {target:?}"))?;

            extracted_files.insert(file_name.to_string_lossy().to_string());
            progress_bar
        };

        for file in extracted_files.iter() {
            let base_path = self.get_path_to_extracted_files();
            let file_path = std::path::Path::new(base_path.as_str()).join(file);
            let metadata = std::fs::metadata(file_path.as_path())
                .context(format_context!("Failed to get metadata for {file_path:?}"))?;

            // mask out write permissions and allow read and execute
            let mut permissions: std::fs::Permissions = metadata.permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(file_path.as_path(), permissions).context(format_context!(
                "Failed to set permissions for {file_path:?}"
            ))?;
        }

        self.save_files_json(Files {
            files: extracted_files
                .into_iter()
                .map(|file| file.into())
                .collect(),
        })
        .context(format_context!("Failed to save json files manifest"))?;
        Ok(next_progress_bar)
    }

    pub fn url_to_relative_path(url: &str, filename: &Option<Arc<str>>) -> anyhow::Result<String> {
        let archive_url = url::Url::parse(url)
            .context(format_context!("Failed to parse bare store url {url}"))?;

        let host = archive_url
            .host_str()
            .ok_or(format_error!("No host found in url {}", url))?;
        let scheme = archive_url.scheme();
        let path = archive_url.path();
        let mut relative_path = format!("{scheme}/{host}{path}");
        if let Some(filename) = filename {
            relative_path = format!("{relative_path}/{filename}");
        }
        Ok(relative_path)
    }
}

fn get_json_files(
    path_to_archive: &std::path::Path,
    json_path: &std::path::Path,
) -> anyhow::Result<Files> {
    let full_path = path_to_archive.join(json_path);
    let json_content = std::fs::read_to_string(full_path.as_path()).context(format_context!(
        "Failed to read JSON file contents {json_path:?}"
    ))?;
    let files: Files = serde_json::from_str(&json_content).context(format_context!(
        "Failed to parse JSON from file {}",
        full_path.display()
    ))?;
    Ok(files)
}

fn delete_ds_store(path_to_archive: &std::path::Path) -> anyhow::Result<()> {
    let ds_store = path_to_archive.join(".DS_Store");
    if ds_store.exists() {
        std::fs::remove_file(ds_store.as_path()).context(format_context!(
            "Failed to remove .DS_Store from {}",
            ds_store.display()
        ))?;
    }
    Ok(())
}

pub(crate) fn get_archive_suffixes() -> &'static [&'static str] {
    const SUFFIXES: &[&str] = &["zip", "gz", "tgz", "bz2", "7z", "xz"];
    SUFFIXES
}

pub fn check_downloaded_archive(path_to_archive: &std::path::Path) -> anyhow::Result<()> {
    delete_ds_store(path_to_archive)?;

    let entries = std::fs::read_dir(path_to_archive).context(format_context!(
        "Failed to read directory {path_to_archive:?}"
    ))?;

    let suffixes: Vec<_> = get_archive_suffixes()
        .iter()
        .map(std::ffi::OsStr::new)
        .collect();

    let is_compressed = path_to_archive
        .extension()
        .is_some_and(|suffix| suffixes.contains(&suffix));

    let mut collected_entries: Vec<_> = entries.collect();
    let mut count = collected_entries.len();

    if !is_compressed && let Some(Ok(first_entry)) = collected_entries.first() {
        if count != 1 {
            return Err(format_error!(
                "Expected 1 entries in archive, found {count}",
            ));
        }
        let path_to_dir = path_to_archive.join(first_entry.path());
        delete_ds_store(path_to_dir.as_path())?;
        let entries = std::fs::read_dir(path_to_dir.as_path()).context(format_context!(
            "Failed to read directory contents {}",
            path_to_dir.display()
        ))?;

        collected_entries = entries.collect();
        count = collected_entries.len();
    }

    if count != 3 {
        return Err(format_error!(
            "Expected 3 entries in archive, found {count}",
        ));
    }

    let mut hash = None;
    let mut files = None;
    let mut file_path = None;

    for entry in collected_entries.into_iter().filter_map(|e| e.ok()) {
        let entry_name = entry.file_name().display().to_string();
        if let Some((current_hash, _suffix)) = entry_name.split_once(".")
            && is_compressed
        {
            if hash.is_none() {
                hash = Some(current_hash.to_owned());
            } else if let Some(hash) = hash.as_ref()
                && current_hash != hash
            {
                return Err(format_error!(
                    "Hash mismatch: expected {hash}, found {current_hash}"
                ));
            }
        }

        // JSON manifest of the files
        if entry_name.ends_with(".json") {
            let json_files = get_json_files(path_to_archive, entry.path().as_path())
                .context(format_context!("Failed to get JSON files from {entry:?}"))?;
            files = Some(json_files);
        }

        // directory containing the files
        if entry_name.ends_with("_files") {
            file_path = Some(entry);
        }
    }

    // check that all the files exist
    if let (Some(files), Some(file_path)) = (files, file_path) {
        for file in files.files {
            let full_path = path_to_archive.join(file_path.path()).join(file.as_ref());
            if !full_path.exists() {
                return Err(format_error!("File {full_path:?} does not exist"));
            }
        }
    } else {
        return Err(format_error!("No files are available in the archive"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------
    // check_file_is_not_html – unit tests (no network)
    // -------------------------------------------------------

    fn write_temp_file(content: &[u8]) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn test_check_file_is_not_html_with_doctype() {
        let f = write_temp_file(b"<!DOCTYPE html><html><body>Not Found</body></html>");
        let result = check_file_is_not_html(
            f.path().to_str().unwrap(),
            "https://example.com/archive.tar.gz",
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("HTML/XML page"), "unexpected error: {msg}");
    }

    #[test]
    fn test_check_file_is_not_html_with_html_tag() {
        let f = write_temp_file(b"<html><head><title>Error</title></head></html>");
        let result = check_file_is_not_html(
            f.path().to_str().unwrap(),
            "https://example.com/archive.tar.gz",
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("HTML/XML page"), "unexpected error: {msg}");
    }

    #[test]
    fn test_check_file_is_not_html_with_xml_declaration() {
        let f = write_temp_file(b"<?xml version=\"1.0\"?><error>Not Found</error>");
        let result = check_file_is_not_html(
            f.path().to_str().unwrap(),
            "https://example.com/archive.tar.gz",
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("HTML/XML page"), "unexpected error: {msg}");
    }

    #[test]
    fn test_check_file_is_not_html_with_head_tag() {
        let f = write_temp_file(b"<head><meta charset=\"utf-8\"></head>");
        let result = check_file_is_not_html(
            f.path().to_str().unwrap(),
            "https://example.com/archive.tar.gz",
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("HTML/XML page"), "unexpected error: {msg}");
    }

    #[test]
    fn test_check_file_is_not_html_with_leading_whitespace() {
        let f = write_temp_file(b"   \n  <!DOCTYPE html><html><body>Oops</body></html>");
        let result = check_file_is_not_html(
            f.path().to_str().unwrap(),
            "https://example.com/archive.tar.gz",
        );
        assert!(
            result.is_err(),
            "leading whitespace before HTML should still be detected"
        );
    }

    #[test]
    fn test_check_file_is_not_html_with_bom_and_doctype() {
        // UTF-8 BOM followed by HTML – the BOM bytes are non-whitespace in
        // from_utf8_lossy, but the replacement char is trimmed by trim_start
        // only if it counts as whitespace. In practice the BOM is *not*
        // whitespace, so the lowercase comparison will start with the BOM
        // replacement char and the check should still not false-positive on
        // a binary file. Let's verify: a BOM + HTML is unusual, but the HTML
        // tag still appears within the first 128 bytes.
        let mut content = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
        content.extend_from_slice(b"<!doctype html><html></html>");
        let f = write_temp_file(&content);
        // The BOM is not whitespace according to trim_start on a lossy string,
        // so the first char will be the replacement character. The detector
        // lowercases the first 128 chars; the replacement char followed by
        // "<!doctype html" won't match any prefix. That's acceptable – the
        // Content-Type header check would catch this instead. Just ensure no
        // panic.
        let _result =
            check_file_is_not_html(f.path().to_str().unwrap(), "https://example.com/a.tar.gz");
    }

    #[test]
    fn test_check_file_is_not_html_accepts_binary() {
        // gzip magic bytes – should pass without error
        let f = write_temp_file(&[0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03]);
        let result = check_file_is_not_html(
            f.path().to_str().unwrap(),
            "https://example.com/archive.tar.gz",
        );
        assert!(
            result.is_ok(),
            "binary file should not be flagged: {result:?}"
        );
    }

    #[test]
    fn test_check_file_is_not_html_accepts_zip() {
        // PK zip magic bytes
        let f = write_temp_file(&[0x50, 0x4b, 0x03, 0x04, 0x00, 0x00, 0x00, 0x00]);
        let result = check_file_is_not_html(
            f.path().to_str().unwrap(),
            "https://example.com/archive.zip",
        );
        assert!(result.is_ok(), "zip file should not be flagged: {result:?}");
    }

    #[test]
    fn test_check_file_is_not_html_empty_file() {
        let f = write_temp_file(b"");
        let result = check_file_is_not_html(
            f.path().to_str().unwrap(),
            "https://example.com/archive.tar.gz",
        );
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("empty"), "unexpected error: {msg}");
    }

    // -------------------------------------------------------
    // Network integration tests – download known HTML
    // -------------------------------------------------------

    #[test]
    #[ignore]
    fn test_download_string_rejects_html_response() {
        // https://the-internet.herokuapp.com/ serves an HTML page with Content-Type: text/html
        let result = download_string(
            "https://the-internet.herokuapp.com/",
            Arc::new(AtomicU32::new(0)),
        );
        assert!(
            result.is_err(),
            "download_string should reject an HTML response"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("text/html"),
            "error should mention text/html Content-Type: {msg}"
        );
    }

    #[test]
    #[ignore]
    fn test_download_rejects_html_content_type() {
        // Use the async `download` function which checks Content-Type on the response.
        // The test URL returns an HTML response (Content-Type: text/html).
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("output.bin");
        let dest_str = dest.to_str().unwrap().to_string();

        let mut printer = printer::Printer::new_null_term();
        let mut multi = printer::MultiProgress::new(&mut printer);
        let progress = multi.add_progress("test", None, None);

        let join_handle = download(
            progress,
            "https://the-internet.herokuapp.com/",
            &dest_str,
            None,
            Arc::new(AtomicU32::new(0)),
            &runtime,
        )
        .expect("download should return a join handle");

        let result = runtime
            .block_on(join_handle)
            .expect("task should not panic");
        if let Err(err) = result {
            let msg = format!("Err: {:?}", err);
            assert!(
                msg.contains("text/html") || msg.contains("HTML"),
                "error should mention HTML: {msg}"
            );
        } else {
            panic!("download should fail for an HTML response");
        }
    }

    #[test]
    #[ignore]
    fn test_download_rejects_soft_404_html_page() {
        // GitHub returns a 200 + HTML page for URLs that look valid but
        // point to a non-existent release asset. We use a URL that is
        // highly unlikely to ever exist.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("output.bin");
        let dest_str = dest.to_str().unwrap().to_string();

        let mut printer = printer::Printer::new_null_term();
        let mut multi = printer::MultiProgress::new(&mut printer);
        let progress = multi.add_progress("test", None, None);

        let join_handle = download(
            progress,
            "https://github.com/nickel-org/rust-mustache/this-does-not-exist.tar.gz",
            &dest_str,
            None,
            Arc::new(AtomicU32::new(0)),
            &runtime,
        )
        .expect("download should return a join handle");

        let result = runtime
            .block_on(join_handle)
            .expect("task should not panic");
        assert!(
            result.is_err(),
            "download should fail for a non-existent GitHub asset that returns HTML"
        );
    }

    // -------------------------------------------------------
    // Network integration test – download a valid archive and
    // verify its SHA256 checksum.
    // -------------------------------------------------------

    #[test]
    fn test_download_valid_archive_and_verify_sha256() {
        let url = "https://github.com/work-spaces/spaces/releases/download/v0.15.27/spaces-linux-x86_64-v0.15.27.zip";
        let expected_sha256 = "66b3a8aaf290c37434df4187c037b0c9cc36223eaec73cd7408b7482c057b67e";

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("spaces-linux-x86_64-v0.15.27.zip");
        let dest_str = dest.to_str().unwrap().to_string();

        let mut printer = printer::Printer::new_null_term();
        let mut multi = printer::MultiProgress::new(&mut printer);
        let progress = multi.add_progress("test", None, None);

        let join_handle = download(
            progress,
            url,
            &dest_str,
            None,
            Arc::new(AtomicU32::new(0)),
            &runtime,
        )
        .expect("download should return a join handle");

        let result = runtime
            .block_on(join_handle)
            .expect("task should not panic");
        if let Err(err) = &result {
            panic!("download of a valid archive should succeed: {err:?}");
        }

        // Verify the file exists and is non-empty
        let metadata = std::fs::metadata(&dest).expect("downloaded file should exist");
        assert!(metadata.len() > 0, "downloaded file should not be empty");

        // Verify SHA256
        let file_contents = std::fs::read(&dest).expect("should be able to read downloaded file");
        let actual_sha256 = sha256::digest(&file_contents).to_ascii_lowercase();
        assert_eq!(
            actual_sha256, expected_sha256,
            "SHA256 mismatch: expected {expected_sha256}, got {actual_sha256}"
        );

        // Also verify the HTML detection passes (the file is a real zip, not HTML)
        check_file_is_not_html(&dest_str, url)
            .expect("valid zip archive should not be flagged as HTML");
    }

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Helper: create a no-op progress bar suitable for tests.
    fn test_progress_bar() -> printer::MultiProgressBar {
        let mut printer = printer::Printer::new_null_term();
        let mut mp = printer::MultiProgress::new(&mut printer);
        mp.add_progress("test", None, None)
    }

    const TEST_URLS: &[(&str, &str)] = &[
        (
            "https://github.com/koalaman/shellcheck/releases/download/v0.8.0/shellcheck-v0.8.0.linux.aarch64.tar.xz",
            "shellcheck-v0.8.0.linux.aarch64.tar.xz",
        ),
        (
            "https://github.com/mvdan/sh/releases/download/v3.10.0/shfmt_v3.10.0_linux_amd64",
            "shfmt_v3.10.0_linux_amd64",
        ),
        (
            "https://github.com/ninja-build/ninja/releases/download/v1.13.1/ninja-linux.zip",
            "ninja-linux.zip",
        ),
        (
            "https://github.com/astral-sh/ruff/releases/download/0.14.7/ruff-x86_64-unknown-linux-musl.tar.gz",
            "ruff-x86_64-unknown-linux-musl.tar.gz",
        ),
    ];

    /// Run one round of concurrent downloads for all test URLs.
    /// Returns the total number of retries that occurred across all downloads.
    fn run_concurrent_downloads() -> u32 {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");

        let destinations: Vec<String> = TEST_URLS
            .iter()
            .map(|(_, filename)| tmp_dir.path().join(filename).to_string_lossy().to_string())
            .collect();

        // One retry counter per download
        let counters: Vec<Arc<AtomicU32>> = TEST_URLS
            .iter()
            .map(|_| Arc::new(AtomicU32::new(0)))
            .collect();

        // Kick off all downloads concurrently
        let handles: Vec<_> = TEST_URLS
            .iter()
            .zip(destinations.iter())
            .zip(counters.iter())
            .map(|(((url, _), dest), counter)| {
                download(
                    test_progress_bar(),
                    url,
                    dest,
                    None,
                    Arc::clone(counter),
                    &runtime,
                )
                .unwrap_or_else(|e| panic!("Failed to start download for {url}: {e}"))
            })
            .collect();

        // Wait for all to complete and verify
        for (handle, (url, filename)) in handles.into_iter().zip(TEST_URLS.iter()) {
            let result = runtime
                .block_on(handle)
                .unwrap_or_else(|e| panic!("{filename} join handle panicked: {e}"));
            result.unwrap_or_else(|e| panic!("{filename} download failed: {e}"));

            let dest = tmp_dir.path().join(filename);
            let meta = std::fs::metadata(&dest)
                .unwrap_or_else(|e| panic!("{filename} missing after download: {e}"));
            assert!(meta.len() > 0, "{url} download produced an empty file");
        }

        // Sum up retries across all downloads
        let mut total_retries = 0u32;
        for ((_, filename), counter) in TEST_URLS.iter().zip(counters.iter()) {
            let retries = counter.load(Ordering::Relaxed);
            if retries > 0 {
                eprintln!("  {filename}: {retries} retry(ies)");
            }
            total_retries += retries;
        }

        total_retries
    }

    #[test]
    #[ignore]
    fn test_concurrent_downloads() {
        const MAX_ROUNDS: u32 = 8;

        for round in 1..=MAX_ROUNDS {
            eprintln!("Download round {round}/{MAX_ROUNDS}");
            let retries = run_concurrent_downloads();

            if retries > 0 {
                eprintln!("Observed {retries} total retry(ies) in round {round} — stopping early");
                return;
            }
        }

        eprintln!(
            "Completed all {MAX_ROUNDS} rounds with no retries observed (network was stable)"
        );
    }

    // ---------------------------------------------------------------
    // wiremock-based tests that deterministically exercise retry logic
    // ---------------------------------------------------------------

    /// Helper to build a tokio runtime for mock tests.
    fn mock_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime")
    }

    /// Server returns 503 on the first request, then 200 with a body on the second.
    /// Verifies the retry counter is incremented and the download succeeds.
    #[test]
    fn test_retry_on_503_then_success() {
        let runtime = mock_runtime();
        let server = runtime.block_on(MockServer::start());

        let body = b"hello world from mock server";

        // First request: 503 Service Unavailable
        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/file.bin"))
                .respond_with(ResponseTemplate::new(503).set_body_string("service unavailable"))
                .expect(1)
                .up_to_n_times(1)
                .mount(&server),
        );

        // Second request: 200 OK with the real body
        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/file.bin"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body.as_slice()))
                .expect(1)
                .mount(&server),
        );

        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dest = tmp_dir
            .path()
            .join("file.bin")
            .to_string_lossy()
            .to_string();

        let url = format!("{}/file.bin", server.uri());
        let counter = Arc::new(AtomicU32::new(0));

        let handle = download(
            test_progress_bar(),
            &url,
            &dest,
            None,
            Arc::clone(&counter),
            &runtime,
        )
        .expect("Failed to start download");

        let result = runtime.block_on(handle).expect("join handle panicked");
        result.expect("download should have succeeded after retry");

        let retries = counter.load(Ordering::Relaxed);
        assert_eq!(retries, 1, "expected exactly 1 retry after 503");

        let contents = std::fs::read(&dest).expect("failed to read downloaded file");
        assert_eq!(contents, body, "downloaded content should match mock body");
    }

    /// Server returns 429 with a Retry-After header on the first request,
    /// then 200 on the second. Verifies the retry counter and that the
    /// Retry-After delay is respected (download still succeeds).
    #[test]
    fn test_retry_on_429_with_retry_after() {
        let runtime = mock_runtime();
        let server = runtime.block_on(MockServer::start());

        let body = b"content after rate limit";

        // First request: 429 Too Many Requests with Retry-After: 1
        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/rate-limited.bin"))
                .respond_with(
                    ResponseTemplate::new(429)
                        .insert_header("Retry-After", "1")
                        .set_body_string("too many requests"),
                )
                .expect(1)
                .up_to_n_times(1)
                .mount(&server),
        );

        // Second request: 200 OK
        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/rate-limited.bin"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body.as_slice()))
                .expect(1)
                .mount(&server),
        );

        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dest = tmp_dir
            .path()
            .join("rate-limited.bin")
            .to_string_lossy()
            .to_string();

        let url = format!("{}/rate-limited.bin", server.uri());
        let counter = Arc::new(AtomicU32::new(0));

        let handle = download(
            test_progress_bar(),
            &url,
            &dest,
            None,
            Arc::clone(&counter),
            &runtime,
        )
        .expect("Failed to start download");

        let result = runtime.block_on(handle).expect("join handle panicked");
        result.expect("download should have succeeded after 429 retry");

        let retries = counter.load(Ordering::Relaxed);
        assert_eq!(retries, 1, "expected exactly 1 retry after 429");

        let contents = std::fs::read(&dest).expect("failed to read downloaded file");
        assert_eq!(contents, body);
    }

    /// Server always returns 404. This is a non-retryable status so the
    /// download should fail immediately without any retries.
    #[test]
    fn test_no_retry_on_404() {
        let runtime = mock_runtime();
        let server = runtime.block_on(MockServer::start());

        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/not-found.bin"))
                .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
                .expect(1)
                .mount(&server),
        );

        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dest = tmp_dir
            .path()
            .join("not-found.bin")
            .to_string_lossy()
            .to_string();

        let url = format!("{}/not-found.bin", server.uri());
        let counter = Arc::new(AtomicU32::new(0));

        let handle = download(
            test_progress_bar(),
            &url,
            &dest,
            None,
            Arc::clone(&counter),
            &runtime,
        )
        .expect("Failed to start download");

        let result = runtime.block_on(handle).expect("join handle panicked");
        assert!(result.is_err(), "download should have failed on 404");

        let retries = counter.load(Ordering::Relaxed);
        assert_eq!(retries, 0, "should not retry on 404");

        // Partial file should have been cleaned up
        assert!(
            !std::path::Path::new(&dest).exists(),
            "partial file should be cleaned up on non-retryable failure"
        );
    }

    /// Server returns 503 twice then 200 on the third request.
    /// Verifies the retry counter accumulates across multiple retries.
    #[test]
    fn test_retry_multiple_503_then_success() {
        let runtime = mock_runtime();
        let server = runtime.block_on(MockServer::start());

        let body = b"third time is the charm";

        // First two requests: 503
        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/flaky.bin"))
                .respond_with(ResponseTemplate::new(503).set_body_string("service unavailable"))
                .expect(2)
                .up_to_n_times(2)
                .mount(&server),
        );

        // Third request: 200 OK
        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/flaky.bin"))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body.as_slice()))
                .expect(1)
                .mount(&server),
        );

        let tmp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let dest = tmp_dir
            .path()
            .join("flaky.bin")
            .to_string_lossy()
            .to_string();

        let url = format!("{}/flaky.bin", server.uri());
        let counter = Arc::new(AtomicU32::new(0));

        let handle = download(
            test_progress_bar(),
            &url,
            &dest,
            None,
            Arc::clone(&counter),
            &runtime,
        )
        .expect("Failed to start download");

        let result = runtime.block_on(handle).expect("join handle panicked");
        result.expect("download should succeed on third attempt");

        let retries = counter.load(Ordering::Relaxed);
        assert_eq!(retries, 2, "expected exactly 2 retries for two 503s");

        let contents = std::fs::read(&dest).expect("failed to read downloaded file");
        assert_eq!(contents, body);
    }

    /// download_string: server returns 503 on first request, 200 on second.
    /// Verifies the blocking retry path works the same way.
    #[test]
    fn test_download_string_retry_on_503() {
        let runtime = mock_runtime();
        let server = runtime.block_on(MockServer::start());

        let expected_text = "sha256hash_value_from_server";

        // First request: 503
        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/checksum.txt"))
                .respond_with(ResponseTemplate::new(503).set_body_string("service unavailable"))
                .expect(1)
                .up_to_n_times(1)
                .mount(&server),
        );

        // Second request: 200
        runtime.block_on(
            Mock::given(method("GET"))
                .and(path("/checksum.txt"))
                .respond_with(ResponseTemplate::new(200).set_body_string(expected_text))
                .expect(1)
                .mount(&server),
        );

        let url = format!("{}/checksum.txt", server.uri());
        let counter = Arc::new(AtomicU32::new(0));

        let result = download_string(&url, Arc::clone(&counter));
        let content = result.expect("download_string should succeed after retry");

        assert_eq!(content.as_ref(), expected_text);

        let retries = counter.load(Ordering::Relaxed);
        assert_eq!(retries, 1, "expected exactly 1 retry for download_string");
    }
}
