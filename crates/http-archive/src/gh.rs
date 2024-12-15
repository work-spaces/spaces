use anyhow::Context;
use anyhow_source_location::format_context;

pub fn transform_url_to_arguments(
    allow_gh_for_download: bool,
    url: &str,
    full_path_to_archive: &str,
) -> Option<Vec<String>> {
    if !allow_gh_for_download {
        return None;
    }

    if url.starts_with("oras://") {
        return None;
    }

    // Parse the URL
    let parsed_url = url::Url::parse(url).ok()?;

    // Ensure the URL is for GitHub releases
    if parsed_url.domain()? != "github.com" {
        return None;
    }

    // Split the path to extract owner, repo, and tag
    let mut path_segments = parsed_url.path_segments()?;
    let owner = path_segments.next()?;
    let repo = path_segments.next()?;
    let release_segment = path_segments.next()?;

    // Ensure it's a release download URL
    if release_segment != "releases" {
        return None;
    }

    // Check if it has "download/tag" structure
    let download_segment = path_segments.next()?;
    let tag = if download_segment == "download" {
        path_segments.next()?
    } else {
        return None;
    };
    let pattern = path_segments.next()?;

    // Return the GitHub CLI command arguments
    Some(vec![
        "release".to_string(),
        "download".to_string(),
        tag.to_string(),
        format!("--repo={}/{}", owner, repo),
        format!("--pattern={}", pattern),
        format!("--output={full_path_to_archive}"),
    ])
}

pub fn download(gh_command: &str, url: &str, arguments: Vec<String>, progress_bar: &mut printer::MultiProgressBar) -> anyhow::Result<()> {
    let options = printer::ExecuteOptions {
        arguments,
        ..Default::default()
    };

    progress_bar.log(
        printer::Level::Trace,
        format!("{url} Downloading using gh {options:?}").as_str(),
    );

    progress_bar
        .execute_process(gh_command, options)
        .context(format_context!(
            "failed to download {url} using gh",
        ))?;

    Ok(())
}