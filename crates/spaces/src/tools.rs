use crate::{builtins, singleton, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use utils::{ci, http_archive, logger, platform, store, ws};

#[derive(Debug, clap::Subcommand, Clone, strum::Display)]
pub enum Command {
    /// Lists the available internal tools.
    List {},
    /// Install internal tools if they are not already installed.
    Install {},
    /// Cleans up old workspaces in the current directory.
    CleanupCheckouts {
        /// Minimum age of workspaces to clean up.
        #[arg(long)]
        age: u16,
        /// Dry run mode - do not deletecle.
        #[arg(long)]
        dry_run: bool,
    },
}

const CARGO_BINSTALL_JSON: &str = include_str!("tools/cargo-binstall.json");
const ORAS_JSON: &str = include_str!("tools/oras.json");
const GH_JSON: &str = include_str!("tools/gh.json");
const TOOLS: &[(&str, &str)] = &[
    ("gh", GH_JSON),
    ("cargo_binstall", CARGO_BINSTALL_JSON),
    ("oras", ORAS_JSON),
];

fn tools_logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "tools".into())
}

fn download_and_install(
    console: console::Console,
    name: &str,
    platform_archive: builtins::checkout::PlatformArchive,
    _is_force_link: bool,
) -> anyhow::Result<Option<String>> {
    let logger = tools_logger(console.clone());
    let this_platform =
        platform::Platform::get_platform().context(format_context!("Failed to get platform"))?;
    let archive = match this_platform {
        platform::Platform::MacosX86_64 => platform_archive.macos_x86_64,
        platform::Platform::MacosAarch64 => platform_archive.macos_aarch64,
        platform::Platform::LinuxX86_64 => platform_archive.linux_x86_64,
        platform::Platform::LinuxAarch64 => platform_archive.linux_aarch64,
        platform::Platform::WindowsX86_64 => platform_archive.windows_x86_64,
        platform::Platform::WindowsAarch64 => platform_archive.windows_aarch64,
    };
    let store_path = ws::get_checkout_store_path();
    let spaces_tools = ws::get_spaces_tools_path(store_path.as_ref());

    if let Some(archive) = archive.as_ref() {
        let relative_path = http_archive::HttpArchive::url_to_relative_path(
            archive.url.as_ref(),
            &archive.filename,
        )
        .context(format_context!("Failed to get relative path for archive"))?;

        let mut http_archive =
            http_archive::HttpArchive::new(store_path.as_ref(), "unused", archive, "no tools path")
                .context(format_context!("Failed to create http archive"))?;

        http_archive.allow_gh_for_download(false);
        let mut links = std::collections::HashSet::new();

        if http_archive.is_download_required() {
            http_archive
                .sync(console.clone())
                .context(format_context!("Failed to sync http archive"))?;

            http_archive
                .create_links(console.clone(), spaces_tools.as_ref(), "unused", &mut links)
                .context(format_context!("Failed to create links"))?;
        } else {
            logger.debug(format!("Skipping download of {name}").as_str());
        };

        http_archive
            .create_links(console.clone(), spaces_tools.as_ref(), "unused", &mut links)
            .context(format_context!("Failed to create links"))?;

        return Ok(Some(relative_path));
    } else {
        logger.debug(format!("{name} not available for {this_platform}").as_str());
    }

    Ok(None)
}

pub fn handle_command(console: console::Console, command: Command) -> anyhow::Result<()> {
    let is_ci = singleton::get_is_ci().into();

    let group = ci::GithubLogGroup::new_group(
        console.clone(),
        is_ci,
        format!("Spaces Tools {command}").as_str(),
    )?;
    let result = match command {
        Command::List {} => list_tools(console.clone()),
        Command::Install {} => install_tools(console.clone(), true),
        Command::CleanupCheckouts { age, dry_run } => {
            cleanup_checkouts(console.clone(), age, dry_run)
        }
    };

    group.end_group(console, is_ci)?;

    result
}

pub fn list_tools(console: console::Console) -> anyhow::Result<()> {
    let logger = tools_logger(console.clone());
    let store_path = ws::get_checkout_store_path_as_path();
    logger.info(
        format!(
            "Path: {}",
            ws::get_spaces_tools_path_to_sysroot_bin(&store_path).display()
        )
        .as_str(),
    );
    logger.info("- builtin: info.get_path_to_spaces_tools()");
    logger.info("Tools:");

    for (name, _json) in TOOLS {
        logger.info(format!("- {name}").as_str());
    }

    Ok(())
}

fn cleanup_checkouts(console: console::Console, age: u16, is_dry_run: bool) -> anyhow::Result<()> {
    let logger = tools_logger(console.clone());
    // get dirs in current dir
    logger.info("Scanning for workspaces");
    let read_dir =
        std::fs::read_dir(".").context(format_context!("Failed to read current directory"))?;

    for entry in read_dir.filter_map(|e| match e {
        Ok(entry) => {
            if entry.path().is_dir() {
                Some(entry)
            } else {
                None
            }
        }
        Err(_) => None,
    }) {
        if let Some(entry_age) = workspace::get_age(&entry.path()) {
            let current_age_in_days = entry_age.get_current_age();
            if current_age_in_days > age as u128 {
                logger.info(format!("{}:", entry.path().display(),).as_str());
                logger.info(format!("  - Age: {} days", entry_age.get_current_age()).as_str());
                if is_dry_run {
                    logger.info("  - Ready to remove (dry-run)");
                } else {
                    logger.info("  - Removing");
                    std::fs::remove_dir_all(entry.path()).context(format_context!(
                        "Failed to delete {}",
                        entry.path().display()
                    ))?;
                }
            }
        }
    }

    Ok(())
}

pub fn install_tools(console: console::Console, is_force_link: bool) -> anyhow::Result<()> {
    let logger = tools_logger(console.clone());
    // install gh in the store bin if it does not exist
    let store_path = ws::get_checkout_store_path();
    let store_sysroot_bin = ws::get_spaces_tools_path(store_path.as_ref());
    std::fs::create_dir_all(store_sysroot_bin.as_ref()).context(format_context!(
        "Failed to create directory {store_sysroot_bin}"
    ))?;

    // Load the store manifest to track installed tools
    let mut manifest_store =
        store::Store::new_from_store_path(std::path::Path::new(store_path.as_ref())).context(
            format_context!("Failed to load store manifest from {}", store_path),
        )?;

    for (name, json) in TOOLS {
        logger.debug(format!("dowload and install {name}").as_str());
        let tool: builtins::checkout::PlatformArchive =
            serde_json::from_str(json).context(format_context!("Failed to parse oras json"))?;

        if let Some(relative_path) =
            download_and_install(console.clone(), name, tool, is_force_link)
                .context(format_context!("Failed to download and install tools"))?
        {
            manifest_store
                .add_entry(std::path::Path::new(&relative_path))
                .context(format_context!("Failed to add tool to store manifest"))?;
        }
    }

    // Save the updated store manifest
    manifest_store
        .save(std::path::Path::new(store_path.as_ref()))
        .context(format_context!("Failed to save store manifest"))?;

    Ok(())
}
