use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::workspace;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub workspaces: HashMap<String, String>,
}

impl Manifest {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)
            .context(format_context!("Failed to read ledger file {path}"))?;
        let result: Manifest = toml::from_str(&contents)
            .context(format_context!("Failed to parse ledger file {path}"))?;
        Ok(result)
    }

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let contents = toml::to_string(&self).context("Failed to serialize ledger")?;
        std::fs::write(path, contents)
            .context(format_context!("Failed to save ledger file {path}"))?;
        Ok(())
    }
}

pub struct Ledger {
    manifest: Manifest,
    full_path_to_ledger: String,
}

impl Ledger {
    pub fn new() -> anyhow::Result<Self> {
        let full_path_to_ledger = format!("{}/spaces_ledger.toml", workspace::get_store_path());
        let manifest = Manifest::new(&full_path_to_ledger).ok().unwrap_or_default();
        Ok(Self {
            manifest,
            full_path_to_ledger,
        })
    }

    #[allow(dead_code)]
    pub fn update(
        &mut self,
        full_path_to_workspace: &str,
        workspace: String,
    ) -> anyhow::Result<()> {
        if let Some(entry) = self.manifest.workspaces.get_mut(full_path_to_workspace) {
            entry.clone_from(&workspace);
        } else {
            self.manifest
                .workspaces
                .insert(full_path_to_workspace.to_string(), workspace.clone());
        }

        Ok(())
    }

    pub fn show_status(&self) -> anyhow::Result<()> {
        let mut printer = printer::Printer::new_stdout();
        for (full_path_to_workspace, workspace) in self.manifest.workspaces.iter() {
            let path = std::path::Path::new(full_path_to_workspace);
            let display_path = path.display().to_string();
            if path.exists() {
                printer.info(display_path.as_str(), &workspace)?;
            } else {
                printer.info(display_path.as_str(), &"Needs Cleanup")?;
            }
        }

        Ok(())
    }
}

impl Drop for Ledger {
    fn drop(&mut self) {
        self.manifest
            .save(&self.full_path_to_ledger)
            .unwrap_or_else(|_| {
                panic!("Failed to save ledger file at {}", self.full_path_to_ledger)
            });
    }
}
