use crate::{context, manifest};
use std::sync::Arc;

pub struct Ledger {
    manifest: manifest::Ledger,
    full_path_to_ledger: String,
}

impl Ledger {
    pub fn new(context: Arc<context::Context>) -> anyhow::Result<Self> {
        let full_path_to_ledger = context.get_bare_store_path("spaces_ledger.toml");
        let manifest = manifest::Ledger::new(&full_path_to_ledger)
            .ok()
            .unwrap_or_default();
        Ok(Self {
            manifest,
            full_path_to_ledger,
        })
    }

    pub fn update(
        &mut self,
        full_path_to_workspace: &str,
        workspace: &manifest::Workspace,
    ) -> anyhow::Result<()> {
        if let Some(entry) = self.manifest.workspaces.get_mut(full_path_to_workspace) {
            *entry = workspace.clone();
        } else {
            self.manifest
                .workspaces
                .insert(full_path_to_workspace.to_string(), workspace.clone());
        }

        Ok(())
    }

    pub fn show_status(&self, context: Arc<context::Context>) -> anyhow::Result<()> {
        for (full_path_to_workspace, workspace) in self.manifest.workspaces.iter() {
            if let Ok(mut printer) = context.printer.write() {
                let path = std::path::Path::new(full_path_to_workspace);

                let display_path = path.display().to_string();

                if path.exists() {
                    printer.info(display_path.as_str(), &workspace)?;
                } else {
                    printer.info(display_path.as_str(), &"Needs Cleanup")?;
                }
            }
        }

        Ok(())
    }
}

impl Drop for Ledger {
    fn drop(&mut self) {
        self.manifest
            .save(&self.full_path_to_ledger)
            .unwrap_or_else(|_| panic!("Failed to save ledger file at {}", self.full_path_to_ledger));
    }
}
