use anyhow::Context;
use anyhow_source_location::{format_context, format_error};

pub fn pre_process_url(url: &str) -> String {
    // the oras url is needs some pre-processing to get the relative path

    if url.starts_with("oras://") {
        if let Some(pos) = url.rfind(':') {
            let mut url = url.to_string();
            // convert the final : to a -
            url.replace_range(pos..pos + 1, "-");
            url
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    }
}

pub fn transform_url_to_arguments(
    url: &str,
    full_path_to_archive: &str,
) -> Option<Vec<String>> {
    if let Some(url) = url.strip_prefix("oras://") {
        // Return the GitHub CLI command arguments
        let path = std::path::Path::new(full_path_to_archive);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok()?;
            Some(vec![
                "pull".to_string(),
                "--no-tty".to_string(),
                format!("--output={}", parent.to_string_lossy()),
                url.to_string(),
            ])
        } else {
            None
        }
    } else {
        None
    }
}

pub fn get_sha256(oras_command: &str, sha256: &str) -> anyhow::Result<Option<(String, String)>> {
    if let Some(label) = sha256.strip_prefix("oras://") {
        // label address:version:json_path
        let parts: Vec<&str> = label.split(':').collect();
        if parts.len() != 4 {
            return Err(format_error!(
                "Invalid oras label for sha256 download {sha256}"
            ));
        }

        let address = parts[0];
        let version = parts[1];
        let digest_path = parts[2];
        let filename_path = parts[3];

        let fetch_label = format!("{}:{}", address, version);

        let options = printer::ExecuteOptions {
            arguments: vec!["manifest".to_string(), "fetch".to_string(), fetch_label],
            is_return_stdout: true,
            ..Default::default()
        };

        let mut printer = printer::Printer::new_null_term();

        let manifest = printer
            .execute_process(oras_command, options)
            .context(format_context!("failed to download {sha256} using oras",))?;

        if let Some(manifest) = manifest {
            let value: serde_json::Value = serde_json::from_str(&manifest)
                .context(format_context!("failed to parse manifest"))?;
            let mut sha256_option = None;
            let mut filename_option = None;

            if let Some(digest) = value.pointer(digest_path) {
                if let Some(digest) = digest.as_str() {
                    if let Some(sha256) = digest.strip_prefix("sha256:") {
                        sha256_option = Some(sha256.to_string());
                    }
                }
            }

            if let Some(filename) = value.pointer(filename_path) {
                if let Some(filename) = filename.as_str() {
                    filename_option = Some(filename.to_string());
                }
            }

            if let (Some(sha256), Some(filename)) = (sha256_option, filename_option) {
                return Ok(Some((filename, sha256)));
            }

            return Err(format_error!(
                "Failed to find sha256 or filename in manifest {sha256}"
            ));
        }

        Ok(None)
    } else {
        Ok(None)
    }
}

pub fn download(
    oras_command: &str,
    url: &str,
    arguments: Vec<String>,
    progress_bar: &mut printer::MultiProgressBar,
) -> anyhow::Result<()> {
    let options = printer::ExecuteOptions {
        arguments,
        ..Default::default()
    };

    progress_bar.log(
        printer::Level::Trace,
        format!("{url} Downloading using oras {options:?}").as_str(),
    );

    progress_bar
        .execute_process(oras_command, options)
        .context(format_context!("failed to download {url} using oras",))?;

    Ok(())
}
