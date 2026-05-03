use crate::{http_archive, logger, platform, store, ws};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use url::Url;

pub const VERSION_FILE_NAME: &str = "spaces.version.json";
pub const GH_HOST_ENV: &str = "SPACES_ENV_GH_HOST";
pub const GH_REPO: &str = "SPACES_ENV_GH_REPO";

fn logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "version/manifest".into())
}

/// GitHub API asset format (from `gh api repos/{repo}/releases`)
/// The GitHub API uses `browser_download_url` for the actual download link
/// and `digest` (format: "sha256:HASH") instead of a bare `sha256` field.
#[derive(Deserialize)]
struct GhApiAsset {
    browser_download_url: Arc<str>,
    name: Arc<str>,
    #[serde(default)]
    digest: Option<Arc<str>>,
}

#[derive(Deserialize)]
struct GhApiRelease {
    tag_name: Arc<str>,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<GhApiAsset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub url: Arc<str>,
    pub name: Arc<str>,
    pub sha256: Arc<str>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: Arc<str>,
    #[serde(default)]
    pub prerelease: bool,
    pub assets: HashMap<platform::Platform, Asset>,
}

pub struct Manifest {
    path_to_store: Arc<std::path::Path>,
    releases: Vec<Release>,
}

impl Manifest {
    pub fn new(path_to_store: &std::path::Path) -> Self {
        Self {
            path_to_store: path_to_store.into(),
            releases: Vec::new(),
        }
    }

    pub fn releases(&self) -> &Vec<Release> {
        &self.releases
    }

    pub fn load_from_store(
        path_to_store: &std::path::Path,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<Self> {
        let save_path = path_to_store.join(VERSION_FILE_NAME);
        progress_bar.set_message("loading manifest");
        let json = std::fs::read_to_string(&save_path).context(format_context!(
            "Failed to load manifest from {}",
            save_path.display()
        ))?;
        let releases: Vec<Release> =
            serde_json::from_str(&json).context(format_context!("Failed to parse manifest"))?;

        let mut manifest = Self {
            path_to_store: path_to_store.into(),
            releases,
        };
        manifest.sort_releases_desc();
        Ok(manifest)
    }

    pub fn populate_using_gh(
        &mut self,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<()> {
        let repo = std::env::var(GH_REPO).unwrap_or_else(|_| "work-spaces/spaces".to_string());
        let host: Arc<str> = std::env::var(GH_HOST_ENV)
            .unwrap_or_else(|_| "github.com".to_string())
            .into();

        let options = console::ExecuteOptions {
            arguments: vec!["api".into(), format!("repos/{}/releases", repo).into()],
            is_return_stdout: true,
            environment: vec![("GH_HOST".into(), host)],
            ..Default::default()
        };

        progress_bar.set_message("getting latest version using gh");

        let gh_command =
            ws::get_spaces_tools_path_to_sysroot_bin(self.path_to_store.as_ref()).join("gh");

        if let Some(stdout) = progress_bar
            .execute_process(gh_command.to_string_lossy().as_ref(), options)
            .context(format_context!("Failed to execute gh api to get releases"))?
            .stdout
        {
            self.set_releases_from_gh_json(stdout.as_str())
                .context(format_context!("Failed to parse manifest response from gh"))?;
            self.save_to_store()
                .context(format_context!("Failed to save manifest from gh"))?;
            Ok(())
        } else {
            Err(format_error!("Failed to fetch latest release"))
        }
    }

    pub fn populate_from_url(
        &mut self,
        progress_bar: &mut console::Progress,
        url: &str,
        headers: &HashMap<Arc<str>, Arc<str>>,
    ) -> anyhow::Result<()> {
        // Reject non-HTTPS URLs immediately so that misconfiguration (e.g. an
        // accidental `http://` URL) fails fast with a clear message instead of
        // silently downloading release metadata over a plaintext channel.
        let parsed_url =
            Url::parse(url).context(format_context!("Invalid manifest URL '{}'", url))?;
        if parsed_url.scheme() != "https" {
            return Err(format_error!(
                "Manifest URL must use HTTPS, but '{}' has scheme '{}'",
                url,
                parsed_url.scheme()
            ));
        }

        progress_bar.set_message("downloading custom version manifest");

        let mut req_headers = reqwest::header::HeaderMap::new();
        for (key, value) in headers {
            let header_name = reqwest::header::HeaderName::from_str(key.as_ref())
                .context(format_context!("Invalid header name '{}'", key))?;
            let header_value = reqwest::header::HeaderValue::from_str(value.as_ref())
                .context(format_context!("Invalid header value for '{}'", key))?;
            req_headers.insert(header_name, header_value);
        }

        let client = reqwest::blocking::ClientBuilder::new()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context(format_context!(
                "Failed to create HTTP client for manifest download"
            ))?;

        let response = client
            .get(url)
            .headers(req_headers)
            .send()
            .context(format_context!("Failed to download manifest from {}", url))?;

        let status = response.status();
        if !status.is_success() {
            return Err(format_error!(
                "Failed to download manifest from {}: HTTP {}",
                url,
                status
            ));
        }

        let body = response.text().context(format_context!(
            "Failed to read manifest response from {}",
            url
        ))?;

        self.set_releases_from_json(body.as_str())
            .context(format_context!(
                "Failed to parse manifest response from {}",
                url
            ))?;
        self.save_to_store()
            .context(format_context!("Failed to save downloaded manifest"))?;

        Ok(())
    }

    /// Sort `self.releases` in descending semver order (newest first).
    ///
    /// Both `set_releases_from_json` (used for custom HTTPS manifests) and
    /// `set_releases_from_gh_json` (used for GitHub API responses) call this
    /// after populating `self.releases`, so `find_release` can safely use
    /// `.first()` / `.find()` regardless of the order items arrived in.
    ///
    /// Tag names are expected to carry an optional leading `v` (e.g. `v1.2.3`).
    /// Tags that do not parse as valid semver sort after all valid-semver tags,
    /// and among themselves are ordered lexicographically descending.
    fn sort_releases_desc(&mut self) {
        self.releases.sort_by(|a, b| {
            fn parse(tag: &str) -> Option<semver::Version> {
                semver::Version::parse(tag.trim_start_matches('v')).ok()
            }
            match (parse(&a.tag_name), parse(&b.tag_name)) {
                (Some(va), Some(vb)) => vb.cmp(&va),         // descending semver
                (Some(_), None) => std::cmp::Ordering::Less, // valid semver before unparseable
                (None, Some(_)) => std::cmp::Ordering::Greater, // unparseable after valid semver
                (None, None) => b.tag_name.cmp(&a.tag_name), // lex descending fallback
            }
        });
    }

    fn set_releases_from_json(&mut self, json: &str) -> anyhow::Result<()> {
        let releases: Vec<Release> = serde_json::from_str(json).context(format_context!(
            "Failed to parse JSON response \
             (expected a top-level array of {{tag_name, prerelease, assets}} objects); \
             body prefix: {}",
            Self::truncate_body(json)
        ))?;
        self.releases = releases;
        self.sort_releases_desc();
        Ok(())
    }

    /// Returns at most [`Self::BODY_LOG_LIMIT`] characters from `body` for use
    /// in error messages, appending `…` when the input is longer.  This keeps
    /// error context informative while preventing large or sensitive response
    /// bodies from bloating logs or leaking into error reports.
    const BODY_LOG_LIMIT: usize = 120;

    fn truncate_body(body: &str) -> String {
        if body.len() <= Self::BODY_LOG_LIMIT {
            body.to_owned()
        } else {
            // Walk back to a valid UTF-8 char boundary so we never produce a
            // string that ends mid-codepoint.
            let mut end = Self::BODY_LOG_LIMIT;
            while !body.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}\u{2026}", &body[..end])
        }
    }

    /// Parse a GitHub API releases response into our internal `Release` format.
    /// The GitHub API uses `browser_download_url` as the download URL and
    /// encodes the hash as `digest: "sha256:HASH"` rather than a bare `sha256` field.
    fn set_releases_from_gh_json(&mut self, json: &str) -> anyhow::Result<()> {
        let gh_releases: Vec<GhApiRelease> =
            serde_json::from_str(json).context(format_context!(
                "Failed to parse GitHub API JSON response \
                 (expected a top-level array of \
                 {{tag_name, prerelease, assets[{{browser_download_url, name, digest}}]}} \
                 objects); body prefix: {}",
                Self::truncate_body(json)
            ))?;

        self.releases = gh_releases
            .into_iter()
            .map(|gh_release| {
                let assets = gh_release
                    .assets
                    .into_iter()
                    .filter_map(|gh_asset| {
                        // Extract the bare hex hash from "sha256:HASH", skipping assets
                        // that carry no recognisable digest.
                        let sha256: Arc<str> = gh_asset
                            .digest
                            .as_deref()
                            .and_then(|d| d.strip_prefix("sha256:"))
                            .map(Arc::from)?;

                        // Determine which platform this asset targets by matching
                        // the platform string against the asset name.
                        let platform = platform::Platform::get_supported_platforms()
                            .into_iter()
                            .find(|p| gh_asset.name.contains(p.to_string().as_str()))?;

                        Some((
                            platform,
                            Asset {
                                url: gh_asset.browser_download_url,
                                name: gh_asset.name,
                                sha256,
                            },
                        ))
                    })
                    .collect();

                Release {
                    tag_name: gh_release.tag_name,
                    prerelease: gh_release.prerelease,
                    assets,
                }
            })
            .collect();

        self.sort_releases_desc();
        Ok(())
    }

    fn save_to_store(&self) -> anyhow::Result<()> {
        let save_path = self.path_to_store.join(VERSION_FILE_NAME);
        let json = serde_json::to_string_pretty(&self.releases).context(format_context!(
            "Internal error: failed to serialize releases"
        ))?;
        std::fs::write(&save_path, json).context(format_context!(
            "Failed to write releases to file {}",
            save_path.display()
        ))?;
        Ok(())
    }

    /// Return the best matching release.
    ///
    /// `self.releases` is always kept sorted newest-first by `sort_releases_desc`,
    /// so `.first()` / `.find()` here reliably return the *maximum* version rather
    /// than whichever entry happened to appear first in the source JSON.
    ///
    /// * `tag = Some(t)` – exact match by tag name (order-independent).
    /// * `tag = None, include_prerelease = true` – newest release of any kind.
    /// * `tag = None, include_prerelease = false` – newest stable (non-prerelease) release.
    pub fn find_release(&self, tag: Option<&str>, include_prerelease: bool) -> Option<&Release> {
        if let Some(tag) = tag {
            self.releases
                .iter()
                .find(|release| release.tag_name.as_ref() == tag)
        } else if include_prerelease {
            self.releases.first()
        } else {
            self.releases.iter().find(|release| !release.prerelease)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_progress() -> console::Progress {
        console::Progress::new(console::Console::new_null(), "", None, None)
    }

    fn make_release(tag: &str, prerelease: bool) -> Release {
        Release {
            tag_name: tag.into(),
            prerelease,
            assets: HashMap::new(),
        }
    }

    fn make_manifest(releases: Vec<Release>) -> Manifest {
        Manifest {
            path_to_store: std::path::Path::new("/tmp").into(),
            releases,
        }
    }

    // -------------------------------------------------------
    // sort_releases_desc
    // -------------------------------------------------------

    #[test]
    fn test_sort_already_newest_first() {
        let mut m = make_manifest(vec![
            make_release("v0.15.3", false),
            make_release("v0.15.2", false),
            make_release("v0.15.1", false),
        ]);
        m.sort_releases_desc();
        let tags: Vec<&str> = m.releases.iter().map(|r| r.tag_name.as_ref()).collect();
        assert_eq!(tags, ["v0.15.3", "v0.15.2", "v0.15.1"]);
    }

    #[test]
    fn test_sort_oldest_first_becomes_newest_first() {
        let mut m = make_manifest(vec![
            make_release("v0.15.1", false),
            make_release("v0.15.2", false),
            make_release("v0.15.3", false),
        ]);
        m.sort_releases_desc();
        let tags: Vec<&str> = m.releases.iter().map(|r| r.tag_name.as_ref()).collect();
        assert_eq!(tags, ["v0.15.3", "v0.15.2", "v0.15.1"]);
    }

    #[test]
    fn test_sort_shuffled_mixed_stable_and_prerelease() {
        // semver: 1.2.0 > 1.1.0 > 1.1.0-beta.1 > 1.0.0 > 0.9.5
        // (pre-release has lower precedence than the release it annotates)
        let mut m = make_manifest(vec![
            make_release("v1.0.0", false),
            make_release("v1.2.0", false),
            make_release("v0.9.5", false),
            make_release("v1.1.0-beta.1", true),
            make_release("v1.1.0", false),
        ]);
        m.sort_releases_desc();
        let tags: Vec<&str> = m.releases.iter().map(|r| r.tag_name.as_ref()).collect();
        assert_eq!(
            tags,
            ["v1.2.0", "v1.1.0", "v1.1.0-beta.1", "v1.0.0", "v0.9.5"]
        );
    }

    #[test]
    fn test_sort_non_semver_tags_placed_after_valid_semver() {
        let mut m = make_manifest(vec![
            make_release("nightly", false),
            make_release("v0.15.0", false),
            make_release("latest", false),
        ]);
        m.sort_releases_desc();
        let tags: Vec<&str> = m.releases.iter().map(|r| r.tag_name.as_ref()).collect();
        // v0.15.0 is the only valid semver; non-semver entries follow (lex desc).
        assert_eq!(tags[0], "v0.15.0");
        // "nightly" and "latest" come after; lex-desc order: 'n' > 'l'
        assert_eq!(tags[1..], ["nightly", "latest"]);
    }

    // -------------------------------------------------------
    // find_release – relies on sorted order
    // -------------------------------------------------------

    /// Core regression: without sorting, find_release(None, false) on an
    /// oldest-first list would return the *oldest* stable release, not the newest.
    #[test]
    fn test_find_release_returns_newest_stable_regardless_of_input_order() {
        // Input intentionally oldest-first (as a generic HTTPS server might serve).
        let mut m = make_manifest(vec![
            make_release("v0.15.1", false),
            make_release("v0.15.3-alpha.1", true),
            make_release("v0.15.2", false),
            make_release("v0.15.3", false),
        ]);
        m.sort_releases_desc();
        let release = m.find_release(None, false);
        assert_eq!(release.map(|r| r.tag_name.as_ref()), Some("v0.15.3"));
    }

    #[test]
    fn test_find_release_newest_including_prerelease() {
        let mut m = make_manifest(vec![
            make_release("v0.15.1", false),
            make_release("v0.15.3-alpha.1", true),
            make_release("v0.15.2", false),
        ]);
        m.sort_releases_desc();
        // v0.15.3-alpha.1 > v0.15.2 in semver, so it should be first overall.
        let release = m.find_release(None, true);
        assert_eq!(
            release.map(|r| r.tag_name.as_ref()),
            Some("v0.15.3-alpha.1")
        );
    }

    #[test]
    fn test_find_release_by_exact_tag() {
        let mut m = make_manifest(vec![
            make_release("v0.15.1", false),
            make_release("v0.15.2", false),
            make_release("v0.15.3", false),
        ]);
        m.sort_releases_desc();
        let release = m.find_release(Some("v0.15.2"), false);
        assert_eq!(release.map(|r| r.tag_name.as_ref()), Some("v0.15.2"));
    }

    #[test]
    fn test_find_release_unknown_tag_returns_none() {
        let mut m = make_manifest(vec![
            make_release("v0.15.1", false),
            make_release("v0.15.2", false),
        ]);
        m.sort_releases_desc();
        assert!(m.find_release(Some("v0.99.0"), false).is_none());
    }

    #[test]
    fn test_find_release_stable_skips_prerelease() {
        let mut m = make_manifest(vec![
            make_release("v0.15.3-beta.1", true),
            make_release("v0.15.2", false),
            make_release("v0.15.1", false),
        ]);
        m.sort_releases_desc();
        // Newest stable should be v0.15.2, not the prerelease v0.15.3-beta.1.
        let release = m.find_release(None, false);
        assert_eq!(release.map(|r| r.tag_name.as_ref()), Some("v0.15.2"));
    }

    #[test]
    fn test_find_release_empty_manifest() {
        let m = make_manifest(vec![]);
        assert!(m.find_release(None, false).is_none());
        assert!(m.find_release(None, true).is_none());
        assert!(m.find_release(Some("v1.0.0"), false).is_none());
    }

    // -------------------------------------------------------
    // populate_from_url – URL scheme validation
    // -------------------------------------------------------

    /// An `http://` URL must be rejected before any network I/O is attempted.
    #[test]
    fn test_populate_from_url_rejects_http() {
        let mut m = make_manifest(vec![]);
        let result = m.populate_from_url(
            &mut make_progress(),
            "http://example.com/manifest.json",
            &HashMap::new(),
        );
        let err = result.unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("HTTPS") || msg.contains("https"),
            "expected HTTPS-related error, got: {msg}"
        );
    }

    /// Any non-HTTPS scheme (here FTP) must also be rejected.
    #[test]
    fn test_populate_from_url_rejects_ftp() {
        let mut m = make_manifest(vec![]);
        let result = m.populate_from_url(
            &mut make_progress(),
            "ftp://files.example.com/manifest.json",
            &HashMap::new(),
        );
        let err = result.unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("HTTPS") || msg.contains("https"),
            "expected HTTPS-related error, got: {msg}"
        );
    }

    // -------------------------------------------------------
    // truncate_body
    // -------------------------------------------------------

    #[test]
    fn test_truncate_body_short_string_is_unchanged() {
        let short = "[{\"tag_name\": \"v1.0.0\"}]";
        assert_eq!(Manifest::truncate_body(short), short);
    }

    #[test]
    fn test_truncate_body_long_string_is_truncated() {
        // Build a body that is clearly longer than BODY_LOG_LIMIT.
        let long: String = "x".repeat(Manifest::BODY_LOG_LIMIT * 2);
        let result = Manifest::truncate_body(&long);
        // Must be shorter than the original.
        assert!(
            result.len() < long.len(),
            "expected truncation, got length {}",
            result.len()
        );
        // Must end with the ellipsis character.
        assert!(
            result.ends_with('\u{2026}'),
            "expected trailing ellipsis, got: {result:?}"
        );
        // The non-ellipsis content must not exceed the limit.
        let content_len = result.trim_end_matches('\u{2026}').len();
        assert!(
            content_len <= Manifest::BODY_LOG_LIMIT,
            "content ({content_len} bytes) exceeds BODY_LOG_LIMIT ({})",
            Manifest::BODY_LOG_LIMIT
        );
    }

    #[test]
    fn test_truncate_body_exactly_at_limit_is_unchanged() {
        let exact: String = "y".repeat(Manifest::BODY_LOG_LIMIT);
        let result = Manifest::truncate_body(&exact);
        assert_eq!(result, exact);
    }

    #[test]
    fn test_set_releases_from_json_error_does_not_contain_full_body() {
        let mut m = make_manifest(vec![]);
        // Craft a body that is much larger than BODY_LOG_LIMIT and is *not* valid JSON.
        let garbage: String = format!("NOT_JSON_{}", "z".repeat(Manifest::BODY_LOG_LIMIT * 3));
        let err = m.set_releases_from_json(&garbage).unwrap_err();
        let msg = format!("{err:?}");
        // The error must mention truncation (trailing ellipsis) rather than
        // dumping the entire body.
        assert!(
            !msg.contains(&garbage),
            "full body must not appear in the error message"
        );
        assert!(
            msg.contains('\u{2026}'),
            "error message should contain a truncation ellipsis; got: {msg}"
        );
    }

    /// A completely unparseable string must be rejected with a clear error.
    #[test]
    fn test_populate_from_url_rejects_invalid_url() {
        let mut m = make_manifest(vec![]);
        let result = m.populate_from_url(&mut make_progress(), "not-a-url", &HashMap::new());
        assert!(result.is_err(), "expected an invalid URL to be rejected");
        let msg = format!("{:?}", result.unwrap_err());
        assert!(
            msg.contains("Invalid manifest URL"),
            "expected parse-error message, got: {msg}"
        );
    }
}

impl Manifest {
    pub fn get_store_path_to_release(
        &self,
        console: console::Console,
        asset: &Asset,
    ) -> Option<Arc<std::path::Path>> {
        match http_archive::HttpArchive::url_to_relative_path(&asset.url, &None) {
            Ok(relative_path) => {
                let path_in_store = self.path_to_store.join(relative_path);
                Some(path_in_store.into())
            }
            Err(err) => {
                logger(console.clone())
                    .error(format!("Failed to convert URL to relative path: {err}").as_str());
                None
            }
        }
    }

    pub fn get_store_path_to_store_binary(
        &self,
        console: console::Console,
        asset: &Asset,
    ) -> Option<Arc<std::path::Path>> {
        let store_path_to_release = self.get_store_path_to_release(console, asset);

        store_path_to_release.map(|store_path| {
            store_path
                .join(format!("{}.zip_files", asset.sha256))
                .join("spaces")
                .into()
        })
    }

    pub fn get_tools_path_to_binary(&self, tag: &str) -> Arc<std::path::Path> {
        ws::get_spaces_tools_path_to_sysroot_bin(&self.path_to_store)
            .join(format!("spaces-{tag}"))
            .into()
    }

    pub fn create_hard_links_to_tools(&self, console: console::Console) -> anyhow::Result<()> {
        let Some(current_platform) = platform::Platform::get_platform() else {
            logger(console).debug("Internal error: unknown platform");
            return Ok(());
        };

        for release in &self.releases {
            let Some(asset) = release.assets.get(&current_platform) else {
                logger(console.clone()).debug(
                    format!("Not linking. No binary for platform {current_platform}").as_str(),
                );
                continue;
            };

            let binary_path = self.get_tools_path_to_binary(&release.tag_name);
            if binary_path.exists() {
                logger(console.clone()).trace(
                    format!("Not linking {} already exists", binary_path.display()).as_str(),
                );
                continue;
            }

            if let Some(source_path) = self.get_store_path_to_store_binary(console.clone(), asset) {
                if source_path.exists() {
                    logger(console.clone()).debug(
                        format!(
                            "Creating hard link from {} to {}",
                            source_path.display(),
                            binary_path.display()
                        )
                        .as_str(),
                    );
                    std::fs::hard_link(&source_path, &binary_path).context(format_context!(
                        "failed to link {} to {}",
                        source_path.display(),
                        binary_path.display()
                    ))?;
                } else {
                    logger(console.clone()).debug(
                        format!("Not linking {} does not exist", source_path.display()).as_str(),
                    );
                }
            }
        }

        Ok(())
    }

    pub fn sync_release_to_store(
        &self,
        console: console::Console,
        progress_bar: &mut console::Progress,
        release: &Release,
    ) -> anyhow::Result<Arc<std::path::Path>> {
        let current_platform = platform::Platform::get_platform()
            .context(format_context!("Internal Error: Unknown Platform"))?;

        let asset = release
            .assets
            .get(&current_platform)
            .context(format_context!(
                "No asset found for the current platform for {}",
                release.tag_name
            ))?;

        if let Some(path) = self.get_store_path_to_release(console.clone(), asset) {
            if path.exists() {
                logger(console.clone()).info(format!("store path: {}", path.display()).as_str());
            } else {
                let archive = http_archive::Archive {
                    url: asset.url.clone(),
                    sha256: asset.sha256.clone(),
                    link: http_archive::ArchiveLink::Hard,
                    ..Default::default()
                };

                let http_archive = http_archive::HttpArchive::new(
                    &self.path_to_store.to_string_lossy(),
                    format!("spaces-{}", release.tag_name).as_str(),
                    &archive,
                    &ws::get_spaces_tools_path_to_sysroot_bin(&self.path_to_store)
                        .to_string_lossy(),
                )
                .context(format_context!(
                    "Failed to create http_archive to download spaces {}",
                    release.tag_name
                ))?;

                progress_bar.set_message(&format!("downloading {}", release.tag_name));
                http_archive.sync(console.clone()).context(format_context!(
                    "Failed to download spaces {}",
                    release.tag_name
                ))?;
            }

            let relative_path =
                http_archive::HttpArchive::url_to_relative_path(&asset.url, &None).context(
                    format_context!("Failed to get relative path for {}", asset.url),
                )?;

            let mut manifest_store = store::Store::new_from_store_path(&self.path_to_store)
                .context(format_context!("Failed to load store manifest"))?;
            manifest_store
                .add_entry(std::path::Path::new(&relative_path))
                .context(format_context!("Failed to add version to store manifest"))?;
            manifest_store
                .save(&self.path_to_store)
                .context(format_context!("Failed to save store manifest"))?;

            let binary_path = self.get_tools_path_to_binary(release.tag_name.as_ref());
            if !binary_path.exists() {
                self.create_hard_links_to_tools(console.clone())
                    .context(format_context!("Failed to update tools to store links"))?;
            }

            if binary_path.exists() {
                Ok(binary_path)
            } else {
                Err(format_error!(
                    "Internal error: tools binary is not found: {}",
                    binary_path.display()
                ))
            }
        } else {
            Err(format_error!(
                "Failed to determine store path for {}",
                asset.url
            ))
        }
    }
}
