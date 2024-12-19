use crate::{builtins, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;

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
    let store_path = workspace::get_checkout_store_path();
    let spaces_tools = workspace::get_spaces_tools_path(store_path.as_str());

    if let Some(archive) = archive.as_ref() {
        let mut http_archive =
            http_archive::HttpArchive::new(store_path.as_str(), "unused", archive, "no tools path")
                .context(format_context!("Failed to create http archive"))?;

        http_archive.allow_gh_for_download(false);

        let progress_bar = if http_archive.is_download_required() {
            let progress_bar = multi_progress.add_progress(name, Some(200), Some("Complete"));
            let progress_bar = http_archive
                .sync(progress_bar)
                .context(format_context!("Failed to sync http archive"))?;

            http_archive
                .create_links(progress_bar, spaces_tools.as_str(), "unused")
                .context(format_context!("Failed to create links"))?;
            None
        } else {
            multi_progress.printer.log(
                printer::Level::Debug,
                format!("Skipping download of {name}").as_str(),
            )?;
            if is_force_link {
                Some(multi_progress.add_progress(name, Some(200), Some("Complete")))
            } else {
                None
            }
            
        };

        if let Some(progress_bar) = progress_bar {
                http_archive
                .create_links(progress_bar, spaces_tools.as_str(), "unused")
                .context(format_context!("Failed to create links"))?;
            
        }

    } else {
        multi_progress.printer.log(
            printer::Level::Debug,
            format!("{name} not available for {this_platform}").as_str(),
        )?;
    }

    Ok(())
}

pub fn install_tools(printer: &mut printer::Printer, 
    is_force_link: bool) -> anyhow::Result<()> {
   
    // install gh in the store bin if it does not exist
    let store_path = workspace::get_checkout_store_path();
    let store_sysroot_bin = workspace::get_spaces_tools_path(store_path.as_str());
    std::fs::create_dir_all(store_sysroot_bin.as_str()).context(format_context!(
        "Failed to create directory {store_sysroot_bin}"
    ))?;

    let gh_json = include_str!("tools/gh.json");
    let gh: builtins::checkout::PlatformArchive = serde_json::from_str(gh_json)
        .context(format_context!("Failed to parse gh json"))?;


    let cargo_binstall_json = include_str!("tools/cargo-binstall.json");
    let cargo_binstall: builtins::checkout::PlatformArchive = serde_json::from_str(cargo_binstall_json)
        .context(format_context!("Failed to parse cargo-binstall json"))?;
  

    let oras_json = include_str!("tools/oras.json");
    let oras: builtins::checkout::PlatformArchive = serde_json::from_str(oras_json)
        .context(format_context!("Failed to parse oras json"))?;

    let tools = vec![("gh", gh), ("cargo_binstall", cargo_binstall), ("oras", oras)];

    let mut multi_progress = printer::MultiProgress::new(printer);

    for (name, tool) in tools {
        multi_progress.printer.log(printer::Level::Debug, format!("dowload and install {name}").as_str())?;
        download_and_install(&mut multi_progress,  name, tool, is_force_link)
            .context(format_context!("Failed to download and install tools"))?;
    }

    Ok(())
}
