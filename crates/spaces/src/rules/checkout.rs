use crate::{executor, info, rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlatformArchive {
    pub macos_x86_64: Option<http_archive::Archive>,
    pub macos_aarch64: Option<http_archive::Archive>,
    pub windows_aarch64: Option<http_archive::Archive>,
    pub windows_x86_64: Option<http_archive::Archive>,
    pub linux_x86_64: Option<http_archive::Archive>,
    pub linux_aarch64: Option<http_archive::Archive>,
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    fn add_repo(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] repo: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let repo: git::Repo = serde_json::from_value(repo.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let worktree_path = workspace::get_workspace_path()
            .context(format_context!("Internal error: workspace path not set"))?;

        let mut state = rules::get_state().write().unwrap();
        let checkout = repo.get_checkout();
        let spaces_key = rule.name.clone();
        let rule_name = rule.name.clone();
        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Checkout,
                executor::Task::Git(executor::git::Git {
                    url: repo.url,
                    spaces_key,
                    worktree_path,
                    checkout,
                }),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn add_platform_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] platforms: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;
        //convert platforms to starlark value
        let platforms: PlatformArchive = serde_json::from_value(platforms.to_json_value()?)?;

        let platform_archive = match platform::Platform::get_platform() {
            Some(platform::Platform::MacosX86_64) => platforms.macos_x86_64,
            Some(platform::Platform::MacosAarch64) => platforms.macos_aarch64,
            Some(platform::Platform::WindowsX86_64) => platforms.windows_x86_64,
            Some(platform::Platform::WindowsAarch64) => platforms.windows_aarch64,
            Some(platform::Platform::LinuxX86_64) => platforms.linux_x86_64,
            Some(platform::Platform::LinuxAarch64) => platforms.linux_aarch64,
            _ => None,
        };

        add_http_archive(rule, platform_archive)
            .context(format_context!("Failed to add archive"))?;

        Ok(NoneType)
    }

    fn add_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] archive: starlark::values::Value,
        // includes, excludes, strip_prefix
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let archive: http_archive::Archive = serde_json::from_value(archive.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        add_http_archive(rule, Some(archive)).context(format_context!("Failed to add archive"))?;

        Ok(NoneType)
    }

    fn add_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let add_asset: executor::asset::AddAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse asset arguments"))?;

        let mut state = rules::get_state().write().unwrap();
        let rule_name = rule.name.clone();

        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Checkout,
                executor::Task::AddAsset(add_asset),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    fn update_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;
        // support JSON, yaml, and toml
        let update_asset: executor::asset::UpdateAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse archive arguments"))?;

        let mut state = rules::get_state().write().unwrap();
        let rule_name = rule.name.clone();

        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::PostCheckout,
                executor::Task::UpdateAsset(update_asset),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    fn update_env(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] env: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rules::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        // support JSON, yaml, and toml
        let update_env: executor::env::UpdateEnv = serde_json::from_value(env.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        let mut state = rules::get_state().write().unwrap();
        let rule_name = rule.name.clone();

        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::PostCheckout,
                executor::Task::UpdateEnv(update_env),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }
}

fn add_http_archive(
    rule: rules::Rule,
    archive_option: Option<http_archive::Archive>,
) -> anyhow::Result<()> {
    if let Some(archive) = archive_option {
        let mut state = rules::get_state().write().unwrap();

        //create a target that waits for all downloads
        //then create links based on all downloads being complete

        let http_archive =
            http_archive::HttpArchive::new(&info::get_store_path(), rule.name.as_str(), &archive)
                .context(format_context!(
                "Failed to create http_archive {}",
                rule.name
            ))?;

        let rule_name = rule.name.clone();
        state
            .insert_task(rules::Task::new(
                rule,
                rules::Phase::Checkout,
                executor::Task::HttpArchive(executor::http_archive::HttpArchive {
                    http_archive: http_archive,
                }),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
    }
    Ok(())
}
