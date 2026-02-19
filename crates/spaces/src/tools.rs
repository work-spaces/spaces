use crate::{builtins, singleton, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use utils::{ci, http_archive, logger, platform, ws};

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

fn tools_logger(printer: &mut printer::Printer) -> logger::Logger<'_> {
    logger::Logger::new_printer(printer, "tools".into())
}

fn download_and_install(
    multi_progress: &mut printer::MultiProgress,
    name: &str,
    platform_archive: builtins::checkout::PlatformArchive,
    is_force_link: bool,
) -> anyhow::Result<()> {
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
        let mut http_archive =
            http_archive::HttpArchive::new(store_path.as_ref(), "unused", archive, "no tools path")
                .context(format_context!("Failed to create http archive"))?;

        http_archive.allow_gh_for_download(false);
        let mut links = std::collections::HashSet::new();

        let progress_bar = if http_archive.is_download_required() {
            let progress_bar = multi_progress.add_progress(name, Some(200), Some("Complete"));
            let progress_bar = http_archive
                .sync(progress_bar)
                .context(format_context!("Failed to sync http archive"))?;

            http_archive
                .create_links(progress_bar, spaces_tools.as_ref(), "unused", &mut links)
                .context(format_context!("Failed to create links"))?;
            None
        } else {
            tools_logger(multi_progress.printer)
                .debug(format!("Skipping download of {name}").as_str());
            if is_force_link {
                Some(multi_progress.add_progress(name, Some(200), Some("Complete")))
            } else {
                None
            }
        };

        if let Some(progress_bar) = progress_bar {
            http_archive
                .create_links(progress_bar, spaces_tools.as_ref(), "unused", &mut links)
                .context(format_context!("Failed to create links"))?;
        }
    } else {
        tools_logger(multi_progress.printer)
            .debug(format!("{name} not available for {this_platform}").as_str());
    }

    Ok(())
}

pub fn handle_command(printer: &mut printer::Printer, command: Command) -> anyhow::Result<()> {
    let is_ci = singleton::get_is_ci().into();

    let group =
        ci::GithubLogGroup::new_group(printer, is_ci, format!("Spaces Tools {command}").as_str())?;
    let result = match command {
        Command::List {} => list_tools(printer),
        Command::Install {} => install_tools(printer, true),
        Command::CleanupCheckouts { age, dry_run } => cleanup_checkouts(printer, age, dry_run),
    };

    group.end_group(printer, is_ci)?;

    result
}

pub fn list_tools(printer: &mut printer::Printer) -> anyhow::Result<()> {
    let store_path = ws::get_checkout_store_path_as_path();
    tools_logger(printer).info(
        format!(
            "Path: {}",
            ws::get_spaces_tools_path_to_sysroot_bin(&store_path).display()
        )
        .as_str(),
    );
    tools_logger(printer).info("- builtin: info.get_path_to_spaces_tools()");
    tools_logger(printer).info("Tools:");

    for (name, _json) in TOOLS {
        tools_logger(printer).info(format!("- {name}").as_str());
    }

    Ok(())
}

fn cleanup_checkouts(
    printer: &mut printer::Printer,
    age: u16,
    is_dry_run: bool,
) -> anyhow::Result<()> {
    // get dirs in current dir
    tools_logger(printer).info("Scanning for workspaces");
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
                tools_logger(printer).info(format!("{}:", entry.path().display(),).as_str());
                tools_logger(printer)
                    .info(format!("  - Age: {} days", entry_age.get_current_age()).as_str());
                if is_dry_run {
                    tools_logger(printer).info("  - Ready to remove (dry-run)");
                } else {
                    tools_logger(printer).info("  - Removing");
                    /*
                    std::fs::remove_dir_all(entry.path()).context(format_context!(
                        "Failed to delete {}",
                        entry.path().display()
                    ))?;
                    */
                }
            }
        }
    }

    Ok(())
}

pub fn install_tools(printer: &mut printer::Printer, is_force_link: bool) -> anyhow::Result<()> {
    // install gh in the store bin if it does not exist
    let store_path = ws::get_checkout_store_path();
    let store_sysroot_bin = ws::get_spaces_tools_path(store_path.as_ref());
    std::fs::create_dir_all(store_sysroot_bin.as_ref()).context(format_context!(
        "Failed to create directory {store_sysroot_bin}"
    ))?;

    let mut multi_progress = printer::MultiProgress::new(printer);

    for (name, json) in TOOLS {
        tools_logger(multi_progress.printer).debug(format!("dowload and install {name}").as_str());
        let tool: builtins::checkout::PlatformArchive =
            serde_json::from_str(json).context(format_context!("Failed to parse oras json"))?;
        download_and_install(&mut multi_progress, name, tool, is_force_link)
            .context(format_context!("Failed to download and install tools"))?;
    }

    Ok(())
}
