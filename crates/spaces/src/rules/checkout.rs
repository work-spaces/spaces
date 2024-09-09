use crate::{executor, info, rules};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use std::collections::HashSet;
use std::vec;

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
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] repo: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let repo: git::Repo = serde_json::from_value(repo.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let worktree_path = info::get_workspace_absolute_path()
            .context(format_context!("Internal error: workspace path not set"))?;

        let mut state = rules::get_state().write().unwrap();
        let checkout = repo.get_checkout();
        state.tasks.insert(
            name.to_string(),
            rules::Task::new(
                name,
                rules::Phase::Checkout,
                Vec::new(),
                HashSet::new(),
                HashSet::new(),
                executor::Task::Git(executor::git::Git {
                    url: repo.url,
                    spaces_key: name.to_string(),
                    worktree_path,
                    checkout,
                }),
            ),
        );
        Ok(NoneType)
    }

    fn add_platform_archive(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] platforms: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
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

        add_http_archive(name, platform_archive)
            .context(format_context!("Failed to add archive"))?;

        Ok(NoneType)
    }

    fn add_archive(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] archive: starlark::values::Value,
        // includes, excludes, strip_prefix
    ) -> anyhow::Result<NoneType> {
        let archive: http_archive::Archive = serde_json::from_value(archive.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        add_http_archive(name, Some(archive)).context(format_context!("Failed to add archive"))?;

        Ok(NoneType)
    }

    fn add_asset(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let add_asset: executor::asset::AddAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse archive arguments"))?;

        let mut state = rules::get_state().write().unwrap();

        state.tasks.insert(
            name.to_string(),
            rules::Task::new(
                name,
                rules::Phase::PostCheckout,
                Vec::new(),
                HashSet::new(),
                HashSet::new(),
                executor::Task::AddAsset(add_asset),
            ),
        );
        Ok(NoneType)
    }

    fn update_asset(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        // support JSON, yaml, and toml
        let update_asset: executor::asset::UpdateAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse archive arguments"))?;

        let mut state = rules::get_state().write().unwrap();

        state.tasks.insert(
            name.to_string(),
            rules::Task::new(
                name,
                rules::Phase::PostCheckout,
                Vec::new(),
                HashSet::new(),
                HashSet::new(),
                executor::Task::UpdateAsset(update_asset),
            ),
        );

        Ok(NoneType)
    }

    fn update_env(
        #[starlark(require = named)] name: &str,
        #[starlark(require = named)] env: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        // support JSON, yaml, and toml
        let update_env: executor::env::UpdateEnv = serde_json::from_value(env.to_json_value()?)
            .context(format_context!("Failed to parse archive arguments"))?;

        let mut state = rules::get_state().write().unwrap();

        state.tasks.insert(
            name.to_string(),
            rules::Task::new(
                name,
                rules::Phase::PostCheckout,
                Vec::new(),
                HashSet::new(),
                HashSet::new(),
                executor::Task::UpdateEnv(update_env),
            ),
        );

        Ok(NoneType)
    }
}

fn add_http_archive(
    name: &str,
    archive_option: Option<http_archive::Archive>,
) -> anyhow::Result<()> {
    if let Some(archive) = archive_option {
        let mut state = rules::get_state().write().unwrap();

        let sync_name = format!("{}_sync", name);

        //create a target that waits for all downloads
        //then create links based on all downloads being complete

        let http_archive = http_archive::HttpArchive::new(&info::get_store_path(), name, &archive)
            .context(format_context!("Failed to create http_archive {}", name))?;

        state.tasks.insert(
            sync_name.clone(),
            rules::Task::new(
                sync_name.as_str(),
                rules::Phase::Checkout,
                Vec::new(),
                HashSet::new(),
                HashSet::new(),
                executor::Task::HttpArchiveSync(executor::http_archive::HttpArchiveSync {
                    http_archive: http_archive.clone(),
                }),
            ),
        );

        state.tasks.insert(
            name.to_string(),
            rules::Task::new(
                name,
                rules::Phase::PostCheckout,
                vec![sync_name],
                HashSet::new(),
                HashSet::new(),
                executor::Task::HttpArchiveCreateLinks(
                    executor::http_archive::HttpArchiveCreateLinks {
                        http_archive: http_archive.clone(),
                    },
                ),
            ),
        );
    }
    Ok(())
}
