use crate::{runner, singleton, task, tools, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::Arc;
use utils::{ci, git, logger, workflows};

fn co_logger(console: console::Console) -> logger::Logger {
    logger::Logger::new(console, "co".into())
}

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

fn set_workspace_store(store: Vec<Arc<str>>) -> anyhow::Result<()> {
    for store_pair in store.iter() {
        let parts = store_pair.split_once('=');
        if parts.is_none() {
            return Err(format_error!(
                "Invalid store format: {store_pair}.\n Use `--store=KEY=VALUE`"
            ));
        }
    }

    singleton::set_args_store(store).context(format_context!(
        "while setting store values for checkout rules"
    ))?;

    Ok(())
}

pub fn checkout_repo(
    console: console::Console,
    name: Arc<str>,
    repo_args: utils::co::CheckoutRepoArgs,
    args: utils::co::CheckoutArgs,
) -> anyhow::Result<()> {
    let clone = repo_args.clone.unwrap_or(git::Clone::Default);
    let command_docstring =
        utils::co::build_checkout_command_docstring(&name, clone, &repo_args, &args);

    set_workspace_env(args.env).context(format_context!("While checking out repo"))?;
    set_workspace_store(args.store).context(format_context!("While checking out repo"))?;
    singleton::set_args_locks(args.lock).context(format_context!("While checking out repo"))?;

    // get the repo name from the url
    let repo_name = if let Some(rule_name) = repo_args.rule_name {
        rule_name
    } else {
        let repo_name = repo_args
            .url
            .split('/')
            .next_back()
            .context(format_context!("URL is mal-formed {}", repo_args.url))?;

        repo_name.strip_suffix(".git").unwrap_or(repo_name).into()
    };

    let url = &repo_args.url;
    let rev = &repo_args.rev;

    let script: Arc<str> = format!(
        r#"{command_docstring}
checkout.add_repo(
    rule = {{
        "name": "{repo_name}"
    }},
    repo = {{
        "url": "{url}",
        "rev": "{rev}",
        "checkout": "Revision",
        "clone": "{clone}"
    }})
"#
    )
    .into();

    tools::install_tools(console.clone(), args.force_install_tools)
        .context(format_context!("while installing tools"))?;

    co_logger(console.clone()).debug(format!("Adding branches {:?}", args.new_branch).as_str());
    handle_new_branch(args.new_branch);

    runner::checkout(
        console.clone(),
        name,
        vec![],
        Some(script),
        args.create_lock_file.into(),
        args.keep_workspace_on_failure,
    )
    .context(format_context!("during runner checkout"))?;

    Ok(())
}

pub fn checkout_workflow(
    console: console::Console,
    name: Arc<str>,
    workflow_args: utils::co::CheckoutWorkflowArgs,
    args: utils::co::CheckoutArgs,
) -> anyhow::Result<()> {
    singleton::set_execution_phase(task::Phase::Run);

    let mut script_inputs: Vec<Arc<str>> = vec![];
    script_inputs.extend(workflow_args.script.clone());

    if workflow_args.wf.is_some() && workflow_args.workflow.is_some() {
        return Err(format_error!("Cannot use both --workflow and --wf"));
    }

    set_workspace_env(args.env).context(format_context!("While checking out workflow"))?;
    set_workspace_store(args.store).context(format_context!("While checking out workflow"))?;
    singleton::set_args_locks(args.lock).context(format_context!("While checking out workflow"))?;

    if let Some(workflow) = workflow_args.workflow.or(workflow_args.wf) {
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

    handle_new_branch(args.new_branch);

    for script_path in script_inputs.iter() {
        let script_as_path = std::path::Path::new(script_path.as_ref());
        if let Some(file_name) = script_as_path.file_name() {
            let file_name = file_name.to_string_lossy();
            if file_name == "env" || file_name == workspace::ENV_FILE_NAME {
                return Err(format_error!(
                    "`{}` is a reserved script name",
                    workspace::ENV_FILE_NAME
                ));
            }
        }
    }

    tools::install_tools(console.clone(), args.force_install_tools)
        .context(format_context!("while installing tools"))?;

    runner::checkout(
        console.clone(),
        name,
        script_inputs,
        None,
        args.create_lock_file.into(),
        args.keep_workspace_on_failure,
    )
    .context(format_context!("during runner checkout"))?;

    Ok(())
}

fn checkout_co_workflow(
    console: console::Console,
    co: utils::co::CheckoutWorkflow,
    name: Arc<str>,
    keep_workspace_on_failure: bool,
    lock: Vec<Arc<str>>,
) -> anyhow::Result<()> {
    let is_ci: ci::IsCi = singleton::get_is_ci().into();
    let _group = ci::GithubLogGroup::new_group(
        console.clone(),
        is_ci,
        format!("Spaces Checkout Workflow {name}").as_str(),
    )?;
    if let Some(toml_store) = co.store {
        singleton::set_args_store_from_toml(toml_store)
            .context(format_context!("while setting toml store values"))?;
    }
    let result = checkout_workflow(
        console.clone(),
        name,
        utils::co::CheckoutWorkflowArgs {
            script: co.script.unwrap_or_default(),
            workflow: co.workflow,
            wf: None,
        },
        utils::co::CheckoutArgs {
            env: co.env.unwrap_or_default(),
            store: vec![],
            store_for_docstring: None,
            new_branch: co.new_branch.unwrap_or_default(),
            create_lock_file: co.create_lock_file.unwrap_or_default(),
            force_install_tools: false,
            keep_workspace_on_failure,
            lock,
        },
    );
    result.context(format_context!("in CheckoutWorkflow"))?;
    Ok(())
}

fn checkout_co_repo(
    console: console::Console,
    co: utils::co::CheckoutRepo,
    name: Arc<str>,
    keep_workspace_on_failure: bool,
    lock: Vec<Arc<str>>,
) -> anyhow::Result<()> {
    let is_ci: ci::IsCi = singleton::get_is_ci().into();
    let _group = ci::GithubLogGroup::new_group(
        console.clone(),
        is_ci,
        format!("Spaces Checkout Repo {}", co.url).as_str(),
    )?;
    let store_for_docstring = co.store.as_ref().map(|store| {
        let mut values: Vec<Arc<str>> = store
            .iter()
            .map(|(key, value)| {
                let rendered_value = match value {
                    toml::Value::String(str_value) => str_value.clone(),
                    _ => value.to_string(),
                };
                format!("{key}={rendered_value}").into()
            })
            .collect();
        values.sort();
        values
    });
    if let Some(toml_store) = co.store {
        singleton::set_args_store_from_toml(toml_store)
            .context(format_context!("while setting toml store values"))?;
    }
    let result = checkout_repo(
        console.clone(),
        name,
        utils::co::CheckoutRepoArgs {
            rule_name: co.rule_name,
            url: co.url,
            rev: co.rev,
            clone: co.clone,
        },
        utils::co::CheckoutArgs {
            env: co.env.unwrap_or_default(),
            store: vec![],
            store_for_docstring,
            new_branch: co.new_branch.unwrap_or_default(),
            create_lock_file: co.create_lock_file.unwrap_or_default(),
            force_install_tools: false,
            keep_workspace_on_failure,
            lock,
        },
    );
    result.context(format_context!("in CheckoutRepo"))?;
    Ok(())
}

pub fn checkout_co(
    co: utils::co::Checkout,
    console: console::Console,
    name: Arc<str>,
    keep_workspace_on_failure: bool,
    lock: Vec<Arc<str>>,
) -> anyhow::Result<()> {
    let result = match co {
        utils::co::Checkout::Workflow(workflow) => checkout_co_workflow(
            console.clone(),
            workflow,
            name,
            keep_workspace_on_failure,
            lock,
        ),
        utils::co::Checkout::Repo(repo) => {
            checkout_co_repo(console.clone(), repo, name, keep_workspace_on_failure, lock)
        }
    };
    result.context(format_context!("during co checkout"))?;
    Ok(())
}
