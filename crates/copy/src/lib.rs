use anyhow::Context;
use anyhow_source_location::format_context;

pub fn copy_with_cow_semantics(
    progress: &mut printer::MultiProgressBar,
    source: &str,
    destination: &str,
) -> anyhow::Result<()> {
    let all_files = walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(|e| if let Ok(e) = e { Some(e) } else { None })
        .collect::<Vec<_>>();
    progress.set_total(all_files.len() as u64);

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
            reflink_copy::reflink_or_copy(entry.path(), destination_path.clone()).context(
                format_context!(
                    "Failed to reflink {} to {}",
                    entry.path().display(),
                    destination_path.display()
                ),
            )?;
        }

        progress.increment(1);
    }
    progress.set_total(200);

    Ok(())
}
