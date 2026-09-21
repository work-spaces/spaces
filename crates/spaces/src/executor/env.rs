use crate::workspace;

use anyhow_source_location::format_error;
use serde::{Deserialize, Serialize};
use utils::{environment, logger};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateEnv {
    pub environment: environment::AnyEnvironment,
}

impl UpdateEnv {
    pub fn execute(
        &self,
        progress: &mut console::Progress,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let console = progress.console.clone();
        logger::Logger::new(console, name.into())
            .debug(format!("Update env {name}: {:?}", self).as_str());
        workspace
            .write()
            .update_env(self.environment.clone())
            .map_err(|err| format_error!("failed to update env\n{err:?}"))?;
        Ok(())
    }
}
