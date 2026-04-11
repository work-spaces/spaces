use crate::logger;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

pub type FilterCallback = Box<dyn Fn(&std::path::Path) -> bool>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UseCowSemantics {
    No,
    Yes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, strum::Display)]
pub enum LinkType {
    None,
    #[default]
    Hard,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MakeReadOnly {
    No,
    Yes,
}

struct LinkState {}

static LINK_STATE: state::InitCell<RwLock<LinkState>> = state::InitCell::new();

fn get_link_state() -> &'static RwLock<LinkState> {
    if let Some(state) = LINK_STATE.try_get() {
        return state;
    }

    LINK_STATE.set(RwLock::new(LinkState {}));
    LINK_STATE.get()
}

pub fn apply_soft_links(
    soft_links: Vec<(std::path::PathBuf, std::path::PathBuf)>,
) -> anyhow::Result<()> {
    for (original, link) in soft_links {
        symlink::symlink_file(&original, &link).context(format_context!(
            "failed to create symlink {original:?} -> {link:?}"
        ))?;
    }
    Ok(())
}

pub fn create_links_from_directory(
    source_dir: &std::path::Path,
    dest_dir: &std::path::Path,
    make_read_only: MakeReadOnly,
    link_type: LinkType,
) -> anyhow::Result<()> {
    let mut soft_links = Vec::new();
    for entry in walkdir::WalkDir::new(source_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let relative = entry
            .path()
            .strip_prefix(source_dir)
            .context(format_context!(
                "internal error: path not prefixed by source dir"
            ))?;

        if entry.file_type().is_dir() {
            let target_dir = dest_dir.join(relative);
            std::fs::create_dir_all(&target_dir)
                .context(format_context!("failed to create directory {target_dir:?}"))?;
            continue;
        }

        let target = dest_dir.join(relative).to_string_lossy().to_string();
        let source = entry.path().to_string_lossy().to_string();
        create_link(
            target.clone(),
            source.clone(),
            make_read_only.clone(),
            Some(&mut soft_links),
            link_type.clone(),
        )
        .context(format_context!("hard link {target} -> {source}"))?;
    }
    apply_soft_links(soft_links)
}

pub fn create_link(
    target_path: String,
    source: String,
    make_read_only: MakeReadOnly,
    soft_links: Option<&mut Vec<(std::path::PathBuf, std::path::PathBuf)>>,
    link_type: LinkType,
) -> anyhow::Result<()> {
    let target = std::path::Path::new(target_path.as_str());
    let original = std::path::Path::new(source.as_str());

    // Hold the mutex to ensure operations are atomic
    #[allow(clippy::readonly_write_lock)]
    let _state = get_link_state().write().unwrap();

    if !original.is_dir() && make_read_only == MakeReadOnly::Yes {
        let original_metadata = std::fs::metadata(original)
            .context(format_context!("Failed to get metadata for {original:?}"))?;

        let mut read_only_permissions = original_metadata.permissions();
        read_only_permissions.set_readonly(true);

        std::fs::set_permissions(original, read_only_permissions).context(format_context!(
            "Failed to set permissions for {original:?}"
        ))?;
    }

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).context(format_context!("{target_path} -> {source}"))?;
    }

    let _ = std::fs::remove_file(target);

    if original.is_symlink() {
        let link = std::fs::read_link(original)
            .context(format_context!("failed to read symlink {original:?}"))?;

        if let Some(soft_links) = soft_links {
            soft_links.push((link, target.into()));
        } else {
            symlink::symlink_file(link.clone(), target).context(format_context!(
                "failed to create symlink {original:?} -> {link:?}"
            ))?;
        }

        return Ok(());
    }

    if link_type == LinkType::None {
        return Ok(());
    }

    if link_type == LinkType::Hard {
        std::fs::hard_link(original, target).context(format_context!(
            "If you get 'Operation Not Permitted' on mac try enabling 'Full Disk Access' for the terminal"))?;
    } else {
        reflink_copy::reflink_or_copy(original, target).context(format_context!(
            "If you get 'Operation Not Permitted' on mac try enabling 'Full Disk Access' for the terminal"))?;

        let target_metadata = std::fs::metadata(target)
            .context(format_context!("Failed to get metadata for {target:?}"))?;

        let mut read_write_permissions = target_metadata.permissions();

        #[allow(clippy::permissions_set_readonly_false)]
        read_write_permissions.set_readonly(false);

        std::fs::set_permissions(target, read_write_permissions).context(format_context!(
            "Failed to set permissions for {}",
            target.display()
        ))?;
    }

    Ok(())
}

pub fn copy_with_cow_semantics(
    progress: &mut console::Progress,
    source: &str,
    destination: &str,
    use_cow_semantics: UseCowSemantics,
    filter: Option<FilterCallback>,
) -> anyhow::Result<()> {
    let all_files = walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| filter.as_ref().is_none_or(|f| f(entry.path())))
        .collect::<Vec<_>>();
    progress.update_progress(0, all_files.len() as u64);

    let mut items_copied = 0;
    for entry in all_files {
        if entry.file_type().is_file() {
            let relative_path = entry.path().strip_prefix(source).context(format_context!(
                "Internal Error: path not prefixed by source"
            ))?;
            progress.set_message(relative_path.display().to_string().as_str());
            let destination_path = std::path::Path::new(destination).join(relative_path);
            if let Some(parent) = destination_path.parent() {
                std::fs::create_dir_all(parent)
                    .context(format_context!("Failed to create parent {parent:?}"))?;
            }
            if use_cow_semantics == UseCowSemantics::Yes {
                let copied = reflink_copy::reflink_or_copy(entry.path(), destination_path.clone())
                    .context(format_context!(
                        "Failed to reflink {} to {}",
                        entry.path().display(),
                        destination_path.display()
                    ))?;
                if copied.is_some() {
                    items_copied += 1;
                }
            } else {
                std::fs::copy(entry.path(), &destination_path).context(format_context!(
                    "Failed to copy {} to {}",
                    entry.path().display(),
                    destination_path.display()
                ))?;
            }
        }

        progress.increment_progress();
    }

    if items_copied > 0 {
        logger::Logger::new(progress.console.clone(), "copy".into()).info(
            format!(
            "{items_copied} items were copied rather than ref-linked using copy-on-write semantics"
        )
            .as_str(),
        );
    }

    progress.update_progress(0, 200);

    Ok(())
}
