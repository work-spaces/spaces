use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use crate::{environment, workspace};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateEnv {
    pub environment: environment::Environment,
}

impl UpdateEnv {
    pub fn execute(
        &self,
        name: &str,
        mut progress: printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        progress.log(
            printer::Level::Debug,
            format!("Update env {name}: {:?}", &self).as_str(),
        );
        workspace::update_env(self.environment.clone()).context(format_context!("failed to update env"))?;
        Ok(())
    }
}
