use anyhow::Context;
use anyhow_source_location::format_context;
use console::{Console, style};
use serde::{Deserialize, Serialize};
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

    fn into_kebab_case(self) -> Arc<str> {
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

/// Features manifest containing the enabled/disabled state of features
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Features {
    /// Map of feature names to their enabled state
    features: enum_map::EnumMap<Feature, bool>,
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

        // Check manifest
        self.features[feature]
    }

    /// Enable a feature in the manifest
    pub fn enable(&mut self, feature: Feature) {
        self.features[feature] = true;
    }

    /// Disable a feature in the manifest
    pub fn disable(&mut self, feature: Feature) {
        self.features[feature] = false;
    }

    /// Get the status of a feature and its source
    fn get_status_with_source(&self, feature: Feature) -> (bool, FeatureSource) {
        // Check environment variable first
        if let Ok(env_value) = std::env::var(feature.env_var_name()) {
            let enabled = env_value.to_uppercase() == "ON";
            return (enabled, FeatureSource::Environment);
        }

        // Check manifest
        let enabled = self.features[feature];
        if enabled {
            (enabled, FeatureSource::Manifest)
        } else {
            // Default
            (false, FeatureSource::Default)
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
    /// Show information about the featurs
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
                    format!("Enabled feature: {}", feature),
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
                    format!("Disabled feature: {}", feature),
                );
                console.info("Status", styled_message)?;
                Ok(())
            }
            FeaturesCommand::Info => {
                let features = Features::new_from_json(store_path)?;

                let title = style::StyledContent::new(console::total_style(), "Feature Status:");
                console.raw(format!("{}\n", title))?;
                console.raw("---------------\n")?;

                for (feature, _enabled) in &features.features {
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

    #[test]
    fn test_manifest_default() {
        // Clean up any env vars that might be set by other parallel tests
        unsafe {
            env::remove_var("SPACES_ENV_RULE_CACHE");
            env::remove_var("SPACES_ENV_MODULE_CACHE");
            env::remove_var("SPACES_ENV_DEPRECATION_WARNINGS");
        }

        let manifest = Features::new();
        assert!(!manifest.is_enabled(Feature::ModuleCache));
        assert!(!manifest.is_enabled(Feature::DeprecationWarnings));
    }

    #[test]
    fn test_manifest_enable_disable() {
        // Clean up any env vars that might be set by other parallel tests
        unsafe { env::remove_var("SPACES_ENV_MODULE_CACHE") };

        let mut manifest = Features::new();

        manifest.enable(Feature::ModuleCache);
        assert!(manifest.is_enabled(Feature::ModuleCache));

        manifest.disable(Feature::ModuleCache);
        assert!(!manifest.is_enabled(Feature::ModuleCache));
    }

    #[test]
    fn test_env_var_override() {
        // Clean up first to ensure clean state
        unsafe { env::remove_var("SPACES_ENV_MODULE_CACHE") };

        let manifest = Features::new();

        // Set environment variable
        unsafe { env::set_var("SPACES_ENV_MODULE_CACHE", "ON") };
        assert!(manifest.is_enabled(Feature::ModuleCache));

        unsafe { env::set_var("SPACES_ENV_MODULE_CACHE", "OFF") };
        assert!(!manifest.is_enabled(Feature::ModuleCache));

        // Clean up
        unsafe { env::remove_var("SPACES_ENV_MODULE_CACHE") };
    }

    #[test]
    fn test_env_var_priority() {
        // Clean up first to ensure clean state
        unsafe { env::remove_var("SPACES_ENV_MODULE_CACHE") };

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
}
