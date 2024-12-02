use crate::executor;
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub enum DependencyType {
    Build,
    Runtime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub semver: String,
    pub dependency_type: DependencyType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    rule: String,         // rule that will build this capsule
    domain: String,       // domain of the capsule
    owner: String,        // owner of the capsule
    repo: String,         // repo of the capsule
    version: String,      // Version of the capsule
    prefix: String,       // --prefix location where the capsule is available when installed
    is_relocatable: bool, // whether the capsule is relocatable
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleCheckoutInfo {
    pub digest: String,  // The workspace digest
    pub info: Vec<Info>, // List of capsules that are available to build
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstallLocation {
    Workspace,
    Store,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capsule {
    pub required: Vec<Dependency>,
    pub scripts: Vec<String>, // list of starlark scripts to execute
    pub install_location: InstallLocation,
    pub semver: String, //semantic version required by caller
}

impl Capsule {
    pub fn execute(
        &self,
        name: &str,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        // create add_workflow.spaces.star - pass it as the first script

        let mut args = vec![
            "--hide-progress-bars".to_string(),
            "--verbosity=debug".to_string(),
            "checkout".to_string(),
        ];

        args.extend(self.scripts.iter().map(|e| format!("--script={e}")));
        args.push(format!("--name={name}"));

        // run spaces checkout in @workflows using name
        let spaces_checkout = executor::exec::Exec {
            command: "spaces".to_string(),
            args: Some(args),
            working_directory: Some("@workflows".to_string()),
            env: None,
            redirect_stdout: None,
            expect: None,
        };

        let checkout_name = format!("{}_checkout", name);
        spaces_checkout
            .execute(&checkout_name, progress)
            .context(format_context!("Failed to checkout workflow {name}"))?;

        // check capsule_checkout_info.spaces.json for a valid CapsuleCheckoutInfo struct

        // ensure the semver is satisfied for this item

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleRun {
    working_directory: String,
}

impl CapsuleRun {
    pub fn execute(
        &self,
        name: &str,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let args = vec![
            "--hide-progress-bars".to_string(),
            "--verbosity=debug".to_string(),
            "run".to_string(),
        ];

        // run spaces checkout in @workflows using name
        let spaces_run = executor::exec::Exec {
            command: "spaces".to_string(),
            args: Some(args),
            working_directory: Some(self.working_directory.clone()),
            env: None,
            redirect_stdout: None,
            expect: None,
        };

        let run_name = format!("{}_run", name);
        spaces_run
            .execute(&run_name, progress)
            .context(format_context!("Failed to checkout workflow {name}"))?;

        Ok(())
    }
}
