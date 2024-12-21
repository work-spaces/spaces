use crate::workspace;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateEnv {
    pub environment: environment::Environment,
}

impl UpdateEnv {
    pub fn execute(
        &self,
        mut progress: printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        logger::Logger::new_progress(&mut progress, name.into()).debug(
            format!("Update env {name}: {:?}", &self).as_str(),
        );
        workspace.write().update_env(self.environment.clone())
            .context(format_context!("failed to update env"))?;
        Ok(())
    }
}
