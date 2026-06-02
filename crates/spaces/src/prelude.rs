use anyhow::Context;
use anyhow_source_location::format_context;
use include_dir::{Dir, include_dir};
use std::sync::Arc;
use utils::{logger, ws};

use crate::workspace;

const PRELUDE_OUTPUT_PATH: &str = "@star/prelude";

static EMBEDDED_PRELUDE_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/assets/prelude");

pub fn get_embedded_prelude_content(relative_path: &str) -> anyhow::Result<Option<Arc<str>>> {
    let normalized_relative_path = relative_path.replace('\\', "/");

    if normalized_relative_path.is_empty() {
        return Ok(None);
    }

    let file = match EMBEDDED_PRELUDE_DIR.get_file(normalized_relative_path.as_str()) {
        Some(file) => file,
        None => return Ok(None),
    };

    let content = std::str::from_utf8(file.contents()).context(format_context!(
        "Embedded prelude file is not valid UTF-8: {}",
        normalized_relative_path
    ))?;

    Ok(Some(content.to_owned().into()))
}

fn logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "prelude".into())
}

fn collect_embedded_prelude_files(
    dir: &Dir<'_>,
    files: &mut Vec<(Arc<str>, Arc<str>)>,
) -> anyhow::Result<()> {
    for file in dir.files() {
        let relative_path: Arc<str> = file.path().to_string_lossy().replace('\\', "/").into();

        let content = std::str::from_utf8(file.contents()).context(format_context!(
            "Prelude file is not valid UTF-8: {}",
            relative_path
        ))?;

        files.push((relative_path, content.to_owned().into()));
    }

    for child in dir.dirs() {
        collect_embedded_prelude_files(child, files)?;
    }

    Ok(())
}

fn save_asset(workspace_path: &str, destination: &str, content: &str) -> anyhow::Result<()> {
    let output_path = std::path::Path::new(workspace_path).join(destination);

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).context(format_context!(
            "Failed to create parent directories for prelude asset {}",
            output_path.display()
        ))?;
    }

    std::fs::write(&output_path, content).context(format_context!(
        "Failed to write prelude asset {}",
        output_path.display()
    ))?;

    Ok(())
}

pub fn generate_checkout_prelude(
    console: console::Console,
    workspace: workspace::WorkspaceArc,
) -> anyhow::Result<()> {
    let log = logger(console);

    let mut embedded_files = Vec::new();
    collect_embedded_prelude_files(&EMBEDDED_PRELUDE_DIR, &mut embedded_files)?;
    embedded_files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut workspace_write = workspace.write();
    let workspace_path = workspace_write.get_absolute_path();
    let previous_checkout = workspace_write.settings.clone_existing_checkout();

    for (relative_path, content) in embedded_files {
        let destination: Arc<str> = format!("{PRELUDE_OUTPUT_PATH}/{relative_path}").into();

        workspace_write.add_checkout_asset(destination.clone(), content.clone());

        if previous_checkout.is_asset_modified(destination.clone()) {
            log.warning(format!("Prelude file {} is modified. Not updating", destination).as_str());
            continue;
        }

        save_asset(
            workspace_path.as_ref(),
            destination.as_ref(),
            content.as_ref(),
        )
        .context(format_context!(
            "Failed to save prelude asset {}",
            destination
        ))?;

        workspace_write
            .settings
            .json
            .assets
            .insert(destination, ws::Asset::new_contents(content.as_ref()));
    }

    Ok(())
}
