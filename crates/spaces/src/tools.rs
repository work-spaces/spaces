use crate::{rules, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;

fn download_and_install(
    multi_progress: &mut printer::MultiProgress,
    name: &str,
    platform_archive: rules::checkout::PlatformArchive,
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
    let bare_store = workspace::get_store_path();
    let spaces_tools = workspace::get_spaces_tools_path();

    if let Some(archive) = archive.as_ref() {
        let mut http_archive =
            http_archive::HttpArchive::new(bare_store.as_str(), "unused", archive)
                .context(format_context!("Failed to create http archive"))?;

        http_archive.allow_gh_for_download(false);

        if http_archive.is_download_required() {
            let progress_bar = multi_progress.add_progress(name, Some(200), Some("Complete"));
            let progress_bar = http_archive
                .sync(progress_bar)
                .context(format_context!("Failed to sync http archive"))?;

            http_archive
                .create_links(progress_bar, spaces_tools.as_str(), "unused")
                .context(format_context!("Failed to create links"))?;
        }
    }

    Ok(())
}

pub fn install_tools(printer: &mut printer::Printer) -> anyhow::Result<()> {
    let gh_includes = Some(vec!["**/bin/gh".to_string()]);
    let gh_prefix = Some("sysroot".to_string());

    let gh = rules::checkout::PlatformArchive {
        macos_aarch64: Some(http_archive::Archive {
            url: "https://github.com/cli/cli/releases/download/v2.53.0/gh_2.53.0_macOS_arm64.zip"
                .to_string(),
            sha256: "d9a6a358292d26f35287f7dc4bb0fe2eae1bb8deea3ac6957644987fadd2af4d".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: gh_includes.clone(),
            excludes: None,
            strip_prefix: Some("gh_2.53.0_macOS_arm64".to_string()),
            add_prefix: gh_prefix.clone(),
        }),
        macos_x86_64: Some(http_archive::Archive {
            url: "https://github.com/cli/cli/releases/download/v2.53.0/gh_2.53.0_macOS_amd64.zip"
                .to_string(),
            sha256: "9319b54b12ae3d03cc129e20cae7a78101d864c6c52eeb19f184fc868df74a85".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: gh_includes.clone(),
            excludes: None,
            strip_prefix: Some("gh_2.53.0_macOS_amd64".to_string()),
            add_prefix: gh_prefix.clone(),
        }),
        linux_aarch64: Some(http_archive::Archive {
            url:
                "https://github.com/cli/cli/releases/download/v2.53.0/gh_2.53.0_linux_arm64.tar.gz"
                    .to_string(),
            sha256: "22c4254025ef5acd7e5406a0eade879e868204861fcb3cd51a95a20cda5d221a".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: gh_includes.clone(),
            excludes: None,
            strip_prefix: Some("gh_2.53.0_linux_arm64".to_string()),
            add_prefix: gh_prefix.clone(),
        }),
        linux_x86_64: Some(http_archive::Archive {
            url:
                "https://github.com/cli/cli/releases/download/v2.53.0/gh_2.53.0_linux_amd64.tar.gz"
                    .to_string(),
            sha256: "ed2caf962730e0f593a2b6cae42a9b827b8a9c8bdd6efb56eae7feec38bdd0c6".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: gh_includes.clone(),
            excludes: None,
            strip_prefix: Some("gh_2.53.0_linux_amd64".to_string()),
            add_prefix: gh_prefix.clone(),
        }),
        windows_x86_64: Some(http_archive::Archive {
            url: "https://github.com/cli/cli/releases/download/v2.53.0/gh_2.53.0_windows_amd64.zip"
                .to_string(),
            sha256: "f23f3268eef9ec4f4a91a79dee510d5d1ab11234f0d6256491cdbb566502db96".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: gh_includes.clone(),
            excludes: None,
            strip_prefix: Some("gh_2.53.0_windows_amd64".to_string()),
            add_prefix: gh_prefix.clone(),
        }),
        windows_aarch64: Some(http_archive::Archive {
            url: "https://github.com/cli/cli/releases/download/v2.53.0/gh_2.53.0_windows_arm64.zip"
                .to_string(),
            sha256: "7503fa8a1bf8b114405a05c1b7c83e273edf3d589bfc6c91d4a4bd521377f0cb".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: gh_includes.clone(),
            excludes: None,
            strip_prefix: Some("gh_2.53.0_windows_arm64".to_string()),
            add_prefix: gh_prefix.clone(),
        }),
    };

    // install gh in the store bin if it does not exist
    let store_sysroot_bin = workspace::get_spaces_tools_path();
    std::fs::create_dir_all(store_sysroot_bin.as_str()).context(format_context!(
        "Failed to create directory {store_sysroot_bin}"
    ))?;

    let cargo_binstall_prefix = Some("sysroot/bin".to_string());

    let cargo_binstall = rules::checkout::PlatformArchive {
        macos_aarch64: Some(http_archive::Archive {
            url: "https://github.com/cargo-bins/cargo-binstall/releases/download/v1.10.9/cargo-binstall-aarch64-apple-darwin.zip".to_string(),
            sha256: "18fe179cad3c90f21da0b983483452c94b910bce9ec05bd53ba9409157aa68f0".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: None,
            excludes: None,
            strip_prefix: None,
            add_prefix: cargo_binstall_prefix.clone(),
        }),
        macos_x86_64: Some(http_archive::Archive {
            url: "https://github.com/cargo-bins/cargo-binstall/releases/download/v1.10.9/cargo-binstall-x86_64-apple-darwin.zip"
                .to_string(),
            sha256: "ee7ffbad9416dc03d1c666017a12d0425508ce44bef6173389ccac309f5b097f".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: None,
            excludes: None,
            strip_prefix: None,
            add_prefix: cargo_binstall_prefix.clone(),
        }),
        linux_aarch64: Some(http_archive::Archive {
            url: "https://github.com/cargo-bins/cargo-binstall/releases/download/v1.10.9/cargo-binstall-aarch64-unknown-linux-gnu.tgz".to_string(),
            sha256: "f7902fb1797b984abbdf07a8ad3f7f0f7d75259f5b66fb85ca3d7f097a345d86".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: None,
            excludes: None,
            strip_prefix: None,
            add_prefix: cargo_binstall_prefix.clone(),
        }),
        linux_x86_64: Some(http_archive::Archive {
            url: "https://github.com/cargo-bins/cargo-binstall/releases/download/v1.10.9/cargo-binstall-x86_64-unknown-linux-gnu.tgz".to_string(),
            sha256: "a12d62ffe88cbe4a0db82bf7287c10ae8fd920e57a53fb6714ad208060782a2b".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: None,
            excludes: None,
            strip_prefix: None,
            add_prefix: cargo_binstall_prefix.clone(),
        }),
        windows_aarch64: Some(http_archive::Archive {
            url: "https://github.com/cargo-bins/cargo-binstall/releases/download/v1.10.9/cargo-binstall-aarch64-pc-windows-msvc.zip".to_string(),
            sha256: "c712771b1ea1443374725039021a46860466c074e6cf7131c7b642252513dada".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: None,
            excludes: None,
            strip_prefix: None,
            add_prefix: cargo_binstall_prefix.clone(),
        }),
        windows_x86_64: Some(http_archive::Archive {
            url: "https://github.com/cargo-bins/cargo-binstall/releases/download/v1.10.9/cargo-binstall-x86_64-pc-windows-msvc.zip".to_string(),
            sha256: "4da50466ee54a154e6990989cb06e978b2863023673dea6448ab6a0177e78375".to_string(),
            link: http_archive::ArchiveLink::Hard,
            includes: None,
            excludes: None,
            strip_prefix: None,
            add_prefix: cargo_binstall_prefix.clone(),
        }),
    };

    let tools = vec![("gh", gh), ("cargo_binstall", cargo_binstall)];

    let mut multi_progress = printer::MultiProgress::new(printer);

    for (name, tool) in tools {
        download_and_install(&mut multi_progress, name, tool)
            .context(format_context!("Failed to download and install tools"))?;
    }

    Ok(())
}
