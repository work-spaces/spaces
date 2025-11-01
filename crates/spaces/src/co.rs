use crate::{runner, singleton, tools, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

fn co_logger(printer: &mut printer::Printer) -> logger::Logger {
    logger::Logger::new_printer(printer, "co".into())
}

pub const CO_FILE_NAME: &str = "co.spaces.toml";

fn handle_new_branch(new_branch: Vec<Arc<str>>) {
    // Add any new branches specified by the command line
    let mut new_branches = singleton::get_new_branches();
    new_branches.extend(new_branch);
    singleton::set_new_branches(new_branches);
}

fn set_workspace_env(env: Vec<Arc<str>>) -> anyhow::Result<()> {
    for env_pair in env.iter() {
        let parts = env_pair.split_once('=');
        if parts.is_none() {
            return Err(format_error!(
                "Invalid env format: {env_pair}.\n Use `--env=VAR=VALUE`"
            ));
        }
    }

    singleton::set_args_env(env).context(format_context!(
        "while setting environment variables for checkout rules"
    ))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn checkout_repo(
    printer: &mut printer::Printer,
    name: Arc<str>,
    rule_name: Option<Arc<str>>,
    url: Arc<str>,
    rev: Arc<str>,
    clone: Option<git::Clone>,
    env: Vec<Arc<str>>,
    new_branch: Vec<Arc<str>>,
    create_lock_file: bool,
    force_install_tools: bool,
) -> anyhow::Result<()> {
    set_workspace_env(env).context(format_context!("While checking out repo"))?;
    let clone = clone.unwrap_or(git::Clone::Default);

    // get the repo name from the url
    let repo_name = if let Some(rule_name) = rule_name {
        rule_name
    } else {
        let repo_name = url
            .split('/')
            .next_back()
            .context(format_context!("URL is mal-formed {url}"))?;

        repo_name.strip_suffix(".git").unwrap_or(repo_name).into()
    };

    // remove the .git extension

    let script: Arc<str> = format!(
        r#"
checkout.add_repo(
rule = {{
"name": "{repo_name}"
}},
repo = {{
"url": "{url}",
"rev": "{rev}",
"checkout": "Revision",
"clone": "{clone}"
}}
)
"#
    )
    .into();

    tools::install_tools(printer, force_install_tools)
        .context(format_context!("while installing tools"))?;

    //sanitize the new branches with a //checkout: prefix
    let mut new_branches = new_branch;
    for branch in new_branches.iter_mut() {
        if !branch.starts_with("//") {
            *branch = format!("//checkout:{branch}").into();
        }
    }

    co_logger(printer).debug(format!("Adding branches {new_branches:?}").as_str());
    handle_new_branch(new_branches);

    runner::checkout(
        printer,
        name,
        vec![],
        Some(script),
        create_lock_file.into(),
        false,
    )
    .context(format_context!("during runner checkout"))?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn checkout_workflow(
    printer: &mut printer::Printer,
    name: Arc<str>,
    env: Vec<Arc<str>>,
    new_branch: Vec<Arc<str>>,
    script: Vec<Arc<str>>,
    workflow: Option<Arc<str>>,
    wf: Option<Arc<str>>,
    create_lock_file: bool,
    force_install_tools: bool,
    keep_workspace_on_failure: bool,
) -> anyhow::Result<()> {
    let mut script_inputs: Vec<Arc<str>> = vec![];
    script_inputs.extend(script.clone());

    if wf.is_some() && workflow.is_some() {
        return Err(format_error!("Cannot use both --workflow and --wf"));
    }

    set_workspace_env(env).context(format_context!("While checking out workflow"))?;

    if let Some(workflow) = workflow.or(wf) {
        let parts: Vec<_> = workflow.split(':').collect();
        if parts.len() != 2 {
            return Err(format_error!(
                "Invalid workflow format: {}.\n Use --workflow=<directory>:<script>,<script>,...",
                workflow
            ));
        }
        let directory = parts[0];

        let inputs: Vec<_> = parts[1].split(',').collect();
        let mut scripts: Vec<Arc<str>> = vec![];

        let is_workspace_toml = if inputs.len() == 1 {
            let dev_flow = workflows::try_workflows(directory, inputs[0])
                .context(format_context!("Failed to parse workflows"))?;
            if let Some(dev_flow) = dev_flow {
                scripts.extend(dev_flow.checkout_scripts);
                singleton::set_new_branches(dev_flow.new_branches);
                true
            } else {
                false
            }
        } else {
            false
        };

        if !is_workspace_toml {
            scripts.extend(inputs.iter().map(|s| (*s).into()));
        }

        for script in scripts {
            let short_path = format!("{directory}/{script}");
            let long_path = format!("{directory}/{script}.spaces.star");
            if !std::path::Path::new(long_path.as_str()).exists()
                && !std::path::Path::new(short_path.as_str()).exists()
            {
                return Err(format_error!(
                    "Script file not found: {}/{}",
                    directory,
                    script
                ));
            }

            script_inputs.push(format!("{directory}/{script}").into());
        }
    }

    handle_new_branch(new_branch);

    for script_path in script_inputs.iter() {
        let script_as_path = std::path::Path::new(script_path.as_ref());
        if let Some(file_name) = script_as_path.file_name() {
            let file_name = file_name.to_string_lossy();
            if file_name == "env" || file_name == workspace::ENV_FILE_NAME {
                return Err(format_error!("`env.spaces.star` is a reserved script name",));
            }
        }
    }

    tools::install_tools(printer, force_install_tools)
        .context(format_context!("while installing tools"))?;

    runner::checkout(
        printer,
        name,
        script_inputs,
        None,
        create_lock_file.into(),
        keep_workspace_on_failure,
    )
    .context(format_context!("during runner checkout"))?;

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutWorkflow {
    pub workflow: Option<Arc<str>>,
    pub script: Option<Vec<Arc<str>>>,
    pub env: Option<Vec<Arc<str>>>,
    #[serde(alias = "new-branch")]
    pub new_branch: Option<Vec<Arc<str>>>,
    #[serde(alias = "create-lock-file")]
    pub create_lock_file: Option<bool>,
}

impl CheckoutWorkflow {
    fn checkout(self, printer: &mut printer::Printer, name: Arc<str>) -> anyhow::Result<()> {
        checkout_workflow(
            printer,
            name,
            self.env.unwrap_or_default(),
            self.new_branch.unwrap_or_default(),
            self.script.unwrap_or_default(),
            self.workflow,
            None,
            self.create_lock_file.unwrap_or_default(),
            false,
            false,
        )
        .context(format_context!("in CheckoutWorkflow"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutRepo {
    pub url: Arc<str>,
    #[serde(alias = "rule-name")]
    pub rule_name: Option<Arc<str>>,
    pub rev: Arc<str>,
    #[serde(alias = "new-branch")]
    pub new_branch: Option<Vec<Arc<str>>>,
    pub clone: Option<git::Clone>,
    pub env: Option<Vec<Arc<str>>>,
    #[serde(alias = "create-lock-file")]
    pub create_lock_file: Option<bool>,
}

impl CheckoutRepo {
    fn checkout(self, printer: &mut printer::Printer, name: Arc<str>) -> anyhow::Result<()> {
        checkout_repo(
            printer,
            name,
            self.rule_name,
            self.url,
            self.rev,
            self.clone,
            self.env.unwrap_or_default(),
            self.new_branch.unwrap_or_default(),
            self.create_lock_file.unwrap_or_default(),
            false,
        )
        .context(format_context!("in CheckoutRepo"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Checkout {
    Workflow(CheckoutWorkflow),
    Repo(CheckoutRepo),
}

impl Checkout {
    pub fn load() -> anyhow::Result<HashMap<Arc<str>, Self>> {
        let contents = std::fs::read_to_string(CO_FILE_NAME).context(format_context!(
            "Failed to open {}. This file must exist in the current directory to use `co`",
            CO_FILE_NAME
        ))?;
        let checkout = toml::from_str(&contents).context(format_context!(
            "Failed to parse toml file {}",
            CO_FILE_NAME
        ))?;
        Ok(checkout)
    }

    pub fn checkout(self, printer: &mut printer::Printer, name: Arc<str>) -> anyhow::Result<()> {
        match self {
            Checkout::Workflow(workflow) => workflow.checkout(printer, name),
            Checkout::Repo(repo) => repo.checkout(printer, name),
        }
        .context(format_context!("during repo checkout"))?;
        Ok(())
    }
}
