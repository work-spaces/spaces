use super::manifest::Manifest;
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub const VERSION_CONFIG_FILE_NAME: &str = "version.spaces.toml";
const VERSION_CONFIG_PATH_ENV_NAME: &str = "SPACES_ENV_VERSION_CONFIG_PATH";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub manifest_url: Arc<str>,
    #[serde(default)]
    pub headers: HashMap<Arc<str>, Arc<str>>,
    #[serde(default)]
    pub env: HashMap<Arc<str>, Arc<str>>,
}

#[derive(Clone, Debug)]
pub struct ResolvedConfig {
    manifest_url: Arc<str>,
    headers: HashMap<Arc<str>, Arc<str>>,
}

impl ResolvedConfig {
    pub fn manifest_url(&self) -> &Arc<str> {
        &self.manifest_url
    }

    pub fn headers(&self) -> &HashMap<Arc<str>, Arc<str>> {
        &self.headers
    }
}

impl Config {
    pub fn new_from_toml(path_to_store: &std::path::Path) -> anyhow::Result<Option<Self>> {
        let explicit_config_path = std::env::var(VERSION_CONFIG_PATH_ENV_NAME).ok();
        let config_path = if let Some(path) = explicit_config_path.as_ref() {
            let trimmed = path.trim().to_string();
            if trimmed.is_empty() {
                return Err(format_error!(
                    "{} is set but empty",
                    VERSION_CONFIG_PATH_ENV_NAME
                ));
            }
            std::path::PathBuf::from(trimmed)
        } else {
            path_to_store.join(VERSION_CONFIG_FILE_NAME)
        };

        if !config_path.exists() {
            if explicit_config_path.is_some() {
                return Err(format_error!(
                    "{} points to a missing file: {}",
                    VERSION_CONFIG_PATH_ENV_NAME,
                    config_path.display()
                ));
            }
            return Ok(None);
        }

        let contents = std::fs::read_to_string(&config_path).context(format_context!(
            "Failed to read version configuration from {}",
            config_path.display()
        ))?;

        let config: Config = toml::from_str(&contents).context(format_context!(
            "Failed to parse version configuration from {}",
            config_path.display()
        ))?;

        Ok(Some(config))
    }

    pub fn resolve(&self) -> anyhow::Result<ResolvedConfig> {
        if self.manifest_url.trim().is_empty() {
            return Err(format_error!("manifest_url cannot be empty"));
        }

        let mut token_values: Vec<(Arc<str>, String)> = self
            .env
            .iter()
            .map(|(token, env_name)| {
                let value = std::env::var(env_name.as_ref()).map_err(|_| {
                    format_error!(
                        "Environment variable '{}' (for token '{}') is not set",
                        env_name,
                        token
                    )
                })?;
                Ok::<(Arc<str>, String), anyhow::Error>((token.clone(), value))
            })
            .collect::<anyhow::Result<Vec<(Arc<str>, String)>>>()?;

        token_values.sort_by_key(|b| std::cmp::Reverse(b.0.len()));

        let manifest_url: Arc<str> =
            Self::replace_tokens(self.manifest_url.as_ref(), &token_values).into();

        let mut headers = HashMap::new();
        for (key, value) in &self.headers {
            let resolved_key: Arc<str> = Self::replace_tokens(key.as_ref(), &token_values).into();
            let resolved_value: Arc<str> =
                Self::replace_tokens(value.as_ref(), &token_values).into();
            headers.insert(resolved_key, resolved_value);
        }

        Ok(ResolvedConfig {
            manifest_url,
            headers,
        })
    }

    pub fn populate_manifest(
        &self,
        manifest: &mut Manifest,
        progress_bar: &mut console::Progress,
    ) -> anyhow::Result<()> {
        let resolved = self
            .resolve()
            .context(format_context!("Failed to resolve version config"))?;

        manifest
            .populate_from_url(
                progress_bar,
                resolved.manifest_url().as_ref(),
                resolved.headers(),
            )
            .context(format_context!(
                "Failed to populate manifest from {}",
                resolved.manifest_url()
            ))
    }

    fn replace_tokens(input: &str, token_values: &[(Arc<str>, String)]) -> String {
        let mut result = input.to_string();
        for (token, value) in token_values {
            result = result.replace(token.as_ref(), value.as_str());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    /// Build a `Config` directly without touching the file system.
    fn make_config(
        manifest_url: &str,
        headers: Vec<(&str, &str)>,
        env_mappings: Vec<(&str, &str)>,
    ) -> Config {
        Config {
            manifest_url: manifest_url.into(),
            headers: headers
                .into_iter()
                .map(|(k, v)| (Arc::from(k), Arc::from(v)))
                .collect(),
            env: env_mappings
                .into_iter()
                .map(|(token, env_name)| (Arc::from(token), Arc::from(env_name)))
                .collect(),
        }
    }

    // -------------------------------------------------------
    // manifest_url validation (no env vars needed)
    // -------------------------------------------------------

    #[test]
    fn test_resolve_rejects_empty_manifest_url() {
        let config = make_config("", vec![], vec![]);
        let err = config.resolve().unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("manifest_url"),
            "expected manifest_url in error, got: {msg}"
        );
    }

    #[test]
    fn test_resolve_rejects_whitespace_only_manifest_url() {
        let config = make_config("   ", vec![], vec![]);
        let err = config.resolve().unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("manifest_url"),
            "expected manifest_url in error, got: {msg}"
        );
    }

    // -------------------------------------------------------
    // Token substitution and missing env vars
    //
    // All tests that read or write environment variables are
    // consolidated here so that they cannot interfere with each
    // other when the test harness runs tests in parallel.
    // -------------------------------------------------------

    #[test]
    fn test_resolve_with_env_vars() {
        // Env var names that are unique to this test suite.
        const ENV_A: &str = "SPACES_TEST_RESOLVE_TOKEN_A";
        const ENV_B: &str = "SPACES_TEST_RESOLVE_TOKEN_B";
        const ENV_LONG: &str = "SPACES_TEST_RESOLVE_TOKEN_LONG";

        // Start with a clean slate.
        unsafe {
            env::remove_var(ENV_A);
            env::remove_var(ENV_B);
            env::remove_var(ENV_LONG);
        }

        // --- Test 1: no tokens – URL and headers pass through unchanged ------
        {
            let config = make_config(
                "https://example.com/manifest.json",
                vec![("Authorization", "Bearer static")],
                vec![],
            );
            let resolved = config.resolve().unwrap();
            assert_eq!(
                resolved.manifest_url().as_ref(),
                "https://example.com/manifest.json"
            );
            assert_eq!(
                resolved.headers().get("Authorization").map(|v| v.as_ref()),
                Some("Bearer static")
            );
        }

        // --- Test 2: token substituted in URL --------------------------------
        {
            unsafe { env::set_var(ENV_A, "secret-token") };
            let config = make_config(
                "https://example.com/{TOKEN_A}/manifest.json",
                vec![],
                vec![("{TOKEN_A}", ENV_A)],
            );
            let resolved = config.resolve().unwrap();
            assert_eq!(
                resolved.manifest_url().as_ref(),
                "https://example.com/secret-token/manifest.json"
            );
            unsafe { env::remove_var(ENV_A) };
        }

        // --- Test 3: token substituted in header value -----------------------
        {
            unsafe { env::set_var(ENV_A, "my-api-key") };
            let config = make_config(
                "https://example.com/manifest.json",
                vec![("Authorization", "Bearer {TOKEN_A}")],
                vec![("{TOKEN_A}", ENV_A)],
            );
            let resolved = config.resolve().unwrap();
            assert_eq!(
                resolved.headers().get("Authorization").map(|v| v.as_ref()),
                Some("Bearer my-api-key")
            );
            unsafe { env::remove_var(ENV_A) };
        }

        // --- Test 4: token substituted in header key -------------------------
        {
            unsafe { env::set_var(ENV_A, "X-Custom-Header") };
            let config = make_config(
                "https://example.com/manifest.json",
                vec![("{TOKEN_A}", "static-value")],
                vec![("{TOKEN_A}", ENV_A)],
            );
            let resolved = config.resolve().unwrap();
            assert!(
                resolved.headers().contains_key("X-Custom-Header"),
                "expected resolved header key 'X-Custom-Header'"
            );
            assert_eq!(
                resolved
                    .headers()
                    .get("X-Custom-Header")
                    .map(|v| v.as_ref()),
                Some("static-value")
            );
            unsafe { env::remove_var(ENV_A) };
        }

        // --- Test 5: token substituted in both header key and value ----------
        {
            unsafe { env::set_var(ENV_A, "resolved-key") };
            unsafe { env::set_var(ENV_B, "resolved-value") };
            let config = make_config(
                "https://example.com/manifest.json",
                vec![("{TOKEN_A}", "{TOKEN_B}")],
                vec![("{TOKEN_A}", ENV_A), ("{TOKEN_B}", ENV_B)],
            );
            let resolved = config.resolve().unwrap();
            assert!(
                resolved.headers().contains_key("resolved-key"),
                "expected resolved header key 'resolved-key'"
            );
            assert_eq!(
                resolved.headers().get("resolved-key").map(|v| v.as_ref()),
                Some("resolved-value")
            );
            unsafe { env::remove_var(ENV_A) };
            unsafe { env::remove_var(ENV_B) };
        }

        // --- Test 6: missing env var – error names both var and token --------
        {
            // Guarantee the var is absent.
            unsafe { env::remove_var(ENV_A) };
            let config = make_config(
                "https://example.com/manifest.json",
                vec![],
                vec![("{TOKEN_A}", ENV_A)],
            );
            let err = config.resolve().unwrap_err();
            let msg = format!("{err:?}");
            assert!(
                msg.contains(ENV_A),
                "expected env var name '{}' in error, got: {msg}",
                ENV_A
            );
            assert!(
                msg.contains("{TOKEN_A}"),
                "expected token name '{{TOKEN_A}}' in error, got: {msg}"
            );
        }

        // --- Test 7: overlapping tokens – longer token wins ------------------
        //
        // Token `MY_TOKEN` is a raw prefix of `MY_TOKEN_EXTRA`.  Without the
        // longest-first sort, applying the short token first would corrupt the
        // longer placeholder, e.g.
        //   "MY_TOKEN_EXTRA" -> "<short>_EXTRA"  (wrong)
        // Correct behaviour:
        //   "MY_TOKEN_EXTRA" -> "<long-value>"   (right)
        {
            unsafe { env::set_var(ENV_A, "short") };
            unsafe { env::set_var(ENV_LONG, "long-value") };
            let config = make_config(
                "https://example.com/MY_TOKEN_EXTRA/manifest.json",
                vec![],
                vec![("MY_TOKEN", ENV_A), ("MY_TOKEN_EXTRA", ENV_LONG)],
            );
            let resolved = config.resolve().unwrap();
            assert_eq!(
                resolved.manifest_url().as_ref(),
                "https://example.com/long-value/manifest.json",
                "longer token should take priority over its shorter prefix"
            );
            unsafe { env::remove_var(ENV_A) };
            unsafe { env::remove_var(ENV_LONG) };
        }

        // --- Test 8: token appears multiple times – all occurrences replaced --
        {
            unsafe { env::set_var(ENV_A, "v1.0.0") };
            let config = make_config(
                "https://example.com/{TOKEN_A}/downloads/{TOKEN_A}.tar.gz",
                vec![("X-Version", "{TOKEN_A}")],
                vec![("{TOKEN_A}", ENV_A)],
            );
            let resolved = config.resolve().unwrap();
            assert_eq!(
                resolved.manifest_url().as_ref(),
                "https://example.com/v1.0.0/downloads/v1.0.0.tar.gz"
            );
            assert_eq!(
                resolved.headers().get("X-Version").map(|v| v.as_ref()),
                Some("v1.0.0")
            );
            unsafe { env::remove_var(ENV_A) };
        }
    }
}
