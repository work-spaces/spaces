use anyhow::Context;
use anyhow_source_location::format_context;
use clap::ValueEnum as _;
use console::{Console, style};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

const MANIFEST_FILE_NAME: &str = "features.spaces.json";

/// Available features that can be toggled
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    strum::Display,
    clap::ValueEnum,
    enum_map::Enum,
)]
#[strum(serialize_all = "snake_case")]
pub enum Feature {
    ModuleCache,
    DeprecationWarnings,
}

impl Feature {
    /// Get the environment variable name for this feature
    pub fn env_var_name(&self) -> String {
        format!("SPACES_ENV_{}", self.to_string().to_uppercase())
    }

    pub fn into_kebab_case(self) -> Arc<str> {
        self.to_string().replace("_", "-").into()
    }
}

/// Source of a feature's configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum FeatureSource {
    /// Set via environment variable
    Environment,
    /// Set in the manifest file
    Manifest,
    /// Using default value
    Default,
}

/// Features manifest containing the enabled/disabled state of features.
///
/// Backed by a `HashMap<String, bool>` so that adding or removing a variant
/// from `Feature` never causes a deserialization error: unknown keys are simply
/// ignored, and absent keys fall back to the default (disabled).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Features {
    /// Map of feature names (snake_case) to their explicitly-set enabled state.
    /// A key that is absent means "not set in manifest – use default".
    #[serde(default)]
    features: HashMap<String, bool>,
}

impl Features {
    /// Create a new empty features manifest
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the features manifest from the store path
    pub fn new_from_json(path_to_store: &Path) -> anyhow::Result<Self> {
        let path = path_to_store.join(MANIFEST_FILE_NAME);
        if path.exists() {
            let contents = std::fs::read_to_string(&path)
                .context(format_context!("Failed to read file: {}", path.display()))?;
            let manifest: Self = serde_json::from_str(&contents).context(format_context!(
                "Failed to deserialize JSON: {}",
                path.display()
            ))?;
            Ok(manifest)
        } else {
            Ok(Self::new())
        }
    }

    /// Save the features manifest to the store path
    pub fn save(&self, path_to_store: &Path) -> anyhow::Result<()> {
        // Ensure the directory exists
        std::fs::create_dir_all(path_to_store).context(format_context!(
            "Failed to create directory: {}",
            path_to_store.display()
        ))?;

        let path = path_to_store.join(MANIFEST_FILE_NAME);
        let contents = serde_json::to_string_pretty(self).context(format_context!(
            "Failed to serialize JSON: {}",
            path.display()
        ))?;
        std::fs::write(&path, contents)
            .context(format_context!("Failed to write file: {}", path.display()))?;
        Ok(())
    }

    /// Check if a feature is enabled
    ///
    /// Priority order:
    /// 1. Environment variable (SPACES_ENV_<FEATURE>)
    /// 2. Manifest file setting
    /// 3. Default (false)
    pub fn is_enabled(&self, feature: Feature) -> bool {
        // Check environment variable first
        if let Ok(env_value) = std::env::var(feature.env_var_name()) {
            return env_value.to_uppercase() == "ON";
        }

        // Check manifest, default to false if not set
        self.features
            .get(&feature.to_string())
            .copied()
            .unwrap_or(false)
    }

    /// Enable a feature in the manifest
    pub fn enable(&mut self, feature: Feature) {
        self.features.insert(feature.to_string(), true);
    }

    /// Disable a feature in the manifest
    pub fn disable(&mut self, feature: Feature) {
        self.features.insert(feature.to_string(), false);
    }

    /// Get the status of a feature and its source
    fn get_status_with_source(&self, feature: Feature) -> (bool, FeatureSource) {
        // Check environment variable first
        if let Ok(env_value) = std::env::var(feature.env_var_name()) {
            let enabled = env_value.to_uppercase() == "ON";
            return (enabled, FeatureSource::Environment);
        }

        // Check manifest
        match self.features.get(&feature.to_string()).copied() {
            Some(enabled) => (enabled, FeatureSource::Manifest),
            None => (false, FeatureSource::Default),
        }
    }
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum FeaturesCommand {
    /// Enable a feature
    Enable {
        /// Feature to enable
        feature: Feature,
    },
    /// Disable a feature
    Disable {
        /// Feature to disable
        feature: Feature,
    },
    /// Show information about the features
    Info,
}

impl FeaturesCommand {
    /// Execute the features command
    pub fn execute(&self, console: &Console, store_path: &Path) -> anyhow::Result<()> {
        match self {
            FeaturesCommand::Enable { feature } => {
                let mut features = Features::new_from_json(store_path)?;
                features.enable(*feature);
                features
                    .save(store_path)
                    .context(format_context!("while saving features to store"))?;
                let styled_message = style::StyledContent::new(
                    console::name_style(),
                    format!("Enabled feature: {}", feature.into_kebab_case()),
                );
                console.info("Status", styled_message)?;
                Ok(())
            }
            FeaturesCommand::Disable { feature } => {
                let mut features = Features::new_from_json(store_path)?;
                features.disable(*feature);
                features
                    .save(store_path)
                    .context(format_context!("while saving features to store"))?;
                let styled_message = style::StyledContent::new(
                    console::name_style(),
                    format!("Disabled feature: {}", feature.into_kebab_case()),
                );
                console.info("Status", styled_message)?;
                Ok(())
            }
            FeaturesCommand::Info => {
                let features = Features::new_from_json(store_path)?;

                let title = style::StyledContent::new(console::total_style(), "Feature Status:");
                console.raw(format!("{}\n", title))?;
                console.raw("---------------\n")?;

                // Iterate over all currently-known variants so the output is
                // always complete, regardless of what is stored on disk.
                for &feature in Feature::value_variants() {
                    let (enabled, source) = features.get_status_with_source(feature);
                    let feature_name = style::StyledContent::new(
                        console::name_style(),
                        format!("{}", feature.into_kebab_case()),
                    );

                    let status_style = if enabled {
                        console::name_style()
                    } else {
                        console::key_style()
                    };
                    let status = if enabled { "ON" } else { "OFF" };
                    let status_styled = style::StyledContent::new(status_style, status);

                    let source_styled =
                        style::StyledContent::new(console::keyword_style(), format!("{}", source));

                    console.raw(format!(
                        "  {} - {} ({})\n",
                        feature_name, status_styled, source_styled
                    ))?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_feature_env_var_name() {
        assert_eq!(
            Feature::ModuleCache.env_var_name(),
            "SPACES_ENV_MODULE_CACHE"
        );
        assert_eq!(
            Feature::DeprecationWarnings.env_var_name(),
            "SPACES_ENV_DEPRECATION_WARNINGS"
        );
    }

    /// Consolidated test for all functionality that manipulates environment variables.
    /// This ensures tests can run in parallel without interference from env var mutations.
    #[test]
    fn test_features_with_env_vars() {
        // Clean up all env vars at the start
        unsafe {
            env::remove_var("SPACES_ENV_RULE_CACHE");
            env::remove_var("SPACES_ENV_MODULE_CACHE");
            env::remove_var("SPACES_ENV_DEPRECATION_WARNINGS");
        }

        // Test 1: Manifest default behavior
        {
            let manifest = Features::new();
            assert!(!manifest.is_enabled(Feature::ModuleCache));
            assert!(!manifest.is_enabled(Feature::DeprecationWarnings));
        }

        // Test 2: Manifest enable/disable
        {
            let mut manifest = Features::new();

            manifest.enable(Feature::ModuleCache);
            assert!(manifest.is_enabled(Feature::ModuleCache));

            manifest.disable(Feature::ModuleCache);
            assert!(!manifest.is_enabled(Feature::ModuleCache));
        }

        // Test 3: Environment variable override
        {
            let manifest = Features::new();

            // Set environment variable
            unsafe { env::set_var("SPACES_ENV_MODULE_CACHE", "ON") };
            assert!(manifest.is_enabled(Feature::ModuleCache));

            unsafe { env::set_var("SPACES_ENV_MODULE_CACHE", "OFF") };
            assert!(!manifest.is_enabled(Feature::ModuleCache));

            // Clean up
            unsafe { env::remove_var("SPACES_ENV_MODULE_CACHE") };
        }

        // Test 4: Environment variable priority
        {
            let mut manifest = Features::new();
            manifest.enable(Feature::ModuleCache);

            // Manifest says enabled
            assert!(manifest.is_enabled(Feature::ModuleCache));

            // Environment variable overrides
            unsafe { env::set_var("SPACES_ENV_MODULE_CACHE", "OFF") };
            assert!(!manifest.is_enabled(Feature::ModuleCache));

            // Clean up
            unsafe { env::remove_var("SPACES_ENV_MODULE_CACHE") };
        }

        // Test 5: Feature source tracking
        {
            let mut features = Features::new();

            // Test 5.1: Unset feature should report Default source
            let (enabled, source) = features.get_status_with_source(Feature::ModuleCache);
            assert!(!enabled);
            assert_eq!(source, FeatureSource::Default);

            // Test 5.2: Explicitly disabled feature should report Manifest source
            features.disable(Feature::ModuleCache);
            let (enabled, source) = features.get_status_with_source(Feature::ModuleCache);
            assert!(!enabled);
            assert_eq!(source, FeatureSource::Manifest);

            // Test 5.3: Explicitly enabled feature should report Manifest source
            features.enable(Feature::ModuleCache);
            let (enabled, source) = features.get_status_with_source(Feature::ModuleCache);
            assert!(enabled);
            assert_eq!(source, FeatureSource::Manifest);

            // Test 5.4: Environment variable should report Environment source
            unsafe { env::set_var("SPACES_ENV_MODULE_CACHE", "ON") };
            let (enabled, source) = features.get_status_with_source(Feature::ModuleCache);
            assert!(enabled);
            assert_eq!(source, FeatureSource::Environment);

            // Clean up
            unsafe { env::remove_var("SPACES_ENV_MODULE_CACHE") };
        }

        // Test 6: JSON serialization with explicit disable
        {
            let temp_dir = tempfile::tempdir().unwrap();

            // Create features with one enabled and one explicitly disabled
            let mut features = Features::new();
            features.enable(Feature::ModuleCache);
            features.disable(Feature::DeprecationWarnings);

            // Save to disk
            features.save(temp_dir.path()).unwrap();

            // Load from disk
            let loaded_features = Features::new_from_json(temp_dir.path()).unwrap();

            // Verify enabled feature reports Manifest source
            let (enabled, source) = loaded_features.get_status_with_source(Feature::ModuleCache);
            assert!(enabled);
            assert_eq!(source, FeatureSource::Manifest);

            // Verify explicitly disabled feature reports Manifest source (not Default)
            let (enabled, source) =
                loaded_features.get_status_with_source(Feature::DeprecationWarnings);
            assert!(!enabled);
            assert_eq!(source, FeatureSource::Manifest);
            // temp_dir dropped here → automatic cleanup
        }

        // Test 7: Unknown keys in JSON are silently ignored (simulates a removed Feature variant)
        {
            let temp_dir = tempfile::tempdir().unwrap();

            // Write a JSON file that contains a feature name that no longer exists
            let json = r#"{"features":{"module_cache":true,"removed_old_feature":true}}"#;
            let path = temp_dir.path().join("features.spaces.json");
            std::fs::write(&path, json).unwrap();

            // Loading must succeed and known features must be correct
            let loaded = Features::new_from_json(temp_dir.path()).unwrap();
            assert!(loaded.is_enabled(Feature::ModuleCache));
            assert!(!loaded.is_enabled(Feature::DeprecationWarnings));
            // temp_dir dropped here → automatic cleanup
        }

        // Final cleanup of all env vars
        unsafe {
            env::remove_var("SPACES_ENV_RULE_CACHE");
            env::remove_var("SPACES_ENV_MODULE_CACHE");
            env::remove_var("SPACES_ENV_DEPRECATION_WARNINGS");
        }
    }
}
