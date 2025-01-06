use crate::{executor, workspace};
use anyhow::Context;
use anyhow_source_location::format_context;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn logger<'a>(progress: &'a mut printer::MultiProgressBar, name: &str) -> logger::Logger<'a> {
    logger::Logger::new_progress(progress, name.into())
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub enum DependencyType {
    Build,
    Runtime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Descriptor {
    pub domain: Arc<str>, // domain of the capsule
    pub owner: Arc<str>,  // owner of the capsule
    pub repo: Arc<str>,   // repo of the capsule
}

impl Descriptor {
    pub fn to_string(&self) -> Arc<str> {
        format!("{}-{}-{}", self.domain, self.owner, self.repo).into()
    }

    fn get_relative_path(&self, digest: &str) -> Arc<str> {
        format!(
            "{}/{}",
            self.to_string(),
            workspace::get_short_digest(digest)
        )
        .into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    descriptor: Descriptor, // descriptor of the capsule
    version: Arc<str>,      // Version of the capsule
    prefix: Arc<str>,       // --prefix location where the capsule is available when installed
}

// capsules.spaces.json
type InfoFile = Vec<Info>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleCheckoutInfo {
    pub digest: Arc<str>, // The workspace digest
    pub info: Vec<Info>,  // List of capsules that are available to build
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleCompleteInfo {
    pub digest: Arc<str>, // The workspace digest
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CapsuleRunStatus {
    AlreadyStarted,
    StartNow,
}

#[derive(Debug)]
pub struct CapsuleRunInfo {
    workspace: workspace::WorkspaceArc,
    lock_file: lock::FileLock,
    digest: Arc<str>,
    descriptor: Descriptor,
    spaces_command: Arc<str>,
}

impl CapsuleRunInfo {
    fn get_capsule_digest(workflows_path: &str, scripts: &[Arc<str>]) -> anyhow::Result<Arc<str>> {
        let mut modules = Vec::new();
        for script in scripts {
            let mut effective_script = script.to_string();
            if !effective_script.ends_with(".spaces.star") {
                effective_script.push_str(".spaces.star");
            }
            let script_path = format!("{}/{}", workflows_path, effective_script);

            let current_working_directory = workspace::get_current_working_directory().context(
                format_context!("Failed to get current working when reading {script_path}"),
            )?;

            let content: Arc<str> = std::fs::read_to_string(script_path.as_str())
                .context(format_context!(
                    "Failed to read script {} from {}",
                    script_path,
                    current_working_directory
                ))?
                .into();
            modules.push((script.to_owned(), content));
        }
        Ok(workspace::calculate_digest(modules.as_slice()))
    }

    fn logger<'a>(&self, progress: &'a mut printer::MultiProgressBar) -> logger::Logger<'a> {
        logger::Logger::new_progress(
            progress,
            self.descriptor
                .get_relative_path(self.digest.as_ref())
                .clone(),
        )
    }

    fn new(
        workspace: workspace::WorkspaceArc,
        descriptor: Descriptor,
        scripts: &[Arc<str>],
    ) -> anyhow::Result<CapsuleRunInfo> {
        let workflows_path = workspace.read().get_path_to_workflows();

        let digest = Self::get_capsule_digest(workflows_path.as_ref(), scripts).context(
            format_context!("Failed to get digest for capsule with scripts: {workflows_path}",),
        )?;

        let status_path = workspace.read().get_path_to_capsule_store_status();
        let lock_file_path = format!("{status_path}/{digest}.json");

        Ok(CapsuleRunInfo {
            workspace,
            digest: digest.clone(),
            descriptor,
            lock_file: lock::FileLock::new(lock_file_path.into()),
            spaces_command: get_spaces_command()
                .context(format_context!("While looking up spaces command"))?,
        })
    }

    fn get_short_digest(&self) -> Arc<str> {
        workspace::get_short_digest(self.digest.as_ref())
    }

    fn get_path(&self, base: Arc<str>) -> Arc<str> {
        format!(
            "{base}/{}",
            self.descriptor
                .get_relative_path(self.get_short_digest().as_ref())
        )
        .into()
    }

    fn get_workspace_name(&self) -> Arc<str> {
        self.get_short_digest()
    }

    #[allow(dead_code)]
    fn get_path_to_capsule_status(&self) -> Arc<str> {
        let base = self.workspace.read().get_path_to_capsule_store_status();
        self.get_path(base)
    }

    fn get_path_to_capsule_workspace(&self) -> Arc<str> {
        let base = self.workspace.read().get_path_to_capsule_store_workspaces();
        self.get_path(base)
    }

    fn get_path_to_capsule_sysroot(&self) -> Arc<str> {
        let base = self.workspace.read().get_path_to_capsule_store_sysroot();
        self.get_path(base)
    }

    fn get_path_to_capsule_workspaces(&self) -> Arc<str> {
        let base = self.workspace.read().get_path_to_capsule_store_workspaces();
        format!("{base}/{}", self.descriptor.to_string()).into()
    }

    fn get_path_to_capsule_worflows(&self) -> Arc<str> {
        self.workspace.read().get_path_to_workflows()
    }

    fn try_lock(&mut self) -> anyhow::Result<CapsuleRunStatus> {
        match self
            .lock_file
            .try_lock()
            .context(format_context!("Failed to lock capsule"))?
        {
            lock::LockStatus::Busy => Ok(CapsuleRunStatus::AlreadyStarted),
            lock::LockStatus::Locked => Ok(CapsuleRunStatus::StartNow),
        }
    }

    fn wait(&self, progress: &mut printer::MultiProgressBar) -> anyhow::Result<()> {
        progress.set_message("Capsule already started, waiting for it to finish");
        self.lock_file
            .wait(progress)
            .context(format_context!("Failed to wait for capsule to finish"))?;

        Ok(())
    }
}

#[derive(Debug)]
struct State {
    info_file: InfoFile,
}

static STATE: state::InitCell<lock::StateLock<State>> = state::InitCell::new();

fn get_state() -> &'static lock::StateLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    let info_file_path = workspace::SPACES_CAPSULES_INFO_NAME;

    let info_file = if std::path::Path::new(info_file_path).exists() {
        load_file_info(".").unwrap_or_default()
    } else {
        Vec::new()
    };

    STATE.set(lock::StateLock::new(State { info_file }));
    STATE.get()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capsule {
    pub descriptor: Descriptor,           // descriptor of the capsule
    pub scripts: Vec<Arc<str>>,           // list of starlark scripts to execute
    pub globs: Option<HashSet<Arc<str>>>, // list of globs to hardlink to the prefix
    pub prefix: Option<Arc<str>>, // --prefix location where the capsule should be installed in the sysroot (default is none)
}

fn load_file_info(capsule_workspace_path: &str) -> anyhow::Result<InfoFile> {
    let file_path = format!(
        "{}/{}",
        capsule_workspace_path,
        workspace::SPACES_CAPSULES_INFO_NAME
    );

    let current_working_directory = workspace::get_current_working_directory().context(
        format_context!("Failed to get current working directory when loading {file_path}"),
    )?;

    let file = std::fs::File::open(file_path.as_str()).context(format_context!(
        "Failed to open {file_path} with CWD {current_working_directory}"
    ))?;
    let info: InfoFile =
        serde_json::from_reader(file).context(format_context!("Failed to parse {file_path}"))?;
    Ok(info)
}

fn get_spaces_command() -> anyhow::Result<Arc<str>> {
    let spaces_exec = which::which("spaces").context("Failed to find spaces executable")?;
    Ok(spaces_exec.to_string_lossy().into())
}

fn get_spaces_env(
    workspace: workspace::WorkspaceArc,
) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
    let mut env: HashMap<Arc<str>, Arc<str>> = HashMap::new();
    let workspace_env = workspace.read().get_env();
    env.insert(
        lock::get_process_group_id_env_name().into(),
        lock::get_process_group_id(),
    );

    env.extend(
        workspace_env
            .get_inherited_vars()
            .context(format_context!("Failed to get inherited vars"))?,
    );

    env.insert(
        workspace::SPACES_ENV_CAPSULE_WORKFLOWS.into(),
        workspace.read().get_path_to_workflows(),
    );

    Ok(env)
}

impl Capsule {
    fn checkout_capsule(
        &self,
        progress: &mut printer::MultiProgressBar,
        capsule_run_info: &CapsuleRunInfo,
    ) -> anyhow::Result<()> {
        let mut args: Vec<Arc<str>> = vec![
            "--hide-progress-bars".into(),
            "--verbosity=debug".into(),
            "checkout".into(),
        ];

        let workflows_path = capsule_run_info.get_path_to_capsule_worflows();

        args.extend(
            self.scripts
                .iter()
                .map(|e| format!("--script={workflows_path}/{e}").into()),
        );
        args.push(format!("--name={}", capsule_run_info.get_workspace_name()).into());

        let capsule_workspaces_path = capsule_run_info.get_path_to_capsule_workspaces();

        capsule_run_info
            .logger(progress)
            .message(format!("Checking out capsule in {capsule_workspaces_path}").as_str());

        std::fs::create_dir_all(capsule_workspaces_path.as_ref()).context(format_context!(
            "Failed to create {capsule_workspaces_path}"
        ))?;

        let mut env = HashMap::new();
        env.insert(
            "PATH".into(),
            std::env::var("PATH").unwrap_or_default().into(),
        );

        env.extend(
            get_spaces_env(capsule_run_info.workspace.clone())
                .context(format_context!("Failed to get spaces env"))?,
        );

        // run spaces checkout in SPACES_CAPSULES_NAME using name
        let spaces_checkout = executor::exec::Exec {
            command: capsule_run_info.spaces_command.clone(),
            args: Some(args),
            working_directory: Some(capsule_workspaces_path.clone()),
            env: Some(env),
            redirect_stdout: None,
            expect: None,
        };

        let checkout_name = format!("{}_checkout", capsule_run_info.get_workspace_name());
        spaces_checkout
            .execute(progress, capsule_run_info.workspace.clone(), &checkout_name)
            .context(format_context!(
                "Failed to checkout workflow {}",
                capsule_run_info.get_workspace_name()
            ))?;

        Ok(())
    }

    fn run_capsule(
        &self,
        progress: &mut printer::MultiProgressBar,
        capsule_run_info: &CapsuleRunInfo,
    ) -> anyhow::Result<()> {
        let args = vec![
            "--hide-progress-bars".into(),
            "--verbosity=debug".into(),
            "run".into(),
        ];

        let workspace_path = capsule_run_info.get_path_to_capsule_workspace();

        // run spaces checkout in SPACES_CAPSULES_NAME using name
        let spaces_run = executor::exec::Exec {
            command: capsule_run_info.spaces_command.clone(),
            args: Some(args),
            working_directory: Some(workspace_path),
            env: Some(
                get_spaces_env(capsule_run_info.workspace.clone())
                    .context(format_context!("Failed to get spaces env"))?,
            ),
            redirect_stdout: None,
            expect: None,
        };

        let run_name = format!("{}_run", capsule_run_info.get_workspace_name());
        spaces_run
            .execute(progress, capsule_run_info.workspace.clone(), &run_name)
            .context(format_context!(
                "Failed to checkout workflow {}",
                capsule_run_info.get_workspace_name()
            ))?;

        Ok(())
    }

    fn hard_link_capsule_to_workspace(
        &self,
        progress: &mut printer::MultiProgressBar,
        capsule_prefix: &str,
    ) -> anyhow::Result<()> {
        if let Some(prefix) = self.prefix.as_ref() {
            // walkdir on capsule prefix - hard link files to the workspace
            let walker = walkdir::WalkDir::new(capsule_prefix);
            let walker_list: Vec<_> = walker.into_iter().collect();

            progress.set_total(walker_list.len() as u64);

            logger(progress, capsule_prefix).info(
                format!("Hard linking capsule prefix {capsule_prefix} to workspace path {prefix}, {} items", walker_list.len())
                    .as_str(),
            );

            for entry in walker_list {
                if let Ok(entry) = entry {
                    if entry.file_type().is_dir() {
                        continue;
                    }

                    let source_path = entry.path();

                    let relative_path =
                        source_path
                            .strip_prefix(capsule_prefix)
                            .context(format_context!(
                                "Failed to strip prefix {:?} from {:?}",
                                capsule_prefix,
                                source_path
                            ))?;

                    if let Some(globs) = self.globs.as_ref() {
                        if !changes::glob::match_globs(
                            globs,
                            relative_path.to_string_lossy().as_ref(),
                        ) {
                            logger(progress, capsule_prefix).debug(
                                format!("Skipping {relative_path:?} because of globs").as_str(),
                            );
                            continue;
                        }
                    }
                    let prefix_path = std::path::Path::new(prefix.as_ref());
                    let destination_path = prefix_path.join(relative_path);

                    if let Some(parent) = destination_path.parent() {
                        std::fs::create_dir_all(parent).context(format_context!(
                            "Failed to create parent directory {:?}",
                            parent
                        ))?;
                    }

                    let destination = destination_path.to_string_lossy().to_string();
                    let source = source_path.to_string_lossy().to_string();
                    logger(progress, capsule_prefix)
                        .trace(format!("Hard linking {:?} to {:?}", source, destination).as_str());

                    http_archive::HttpArchive::create_hard_link(
                        destination.clone(),
                        source.clone(),
                    )
                    .context(format_context!(
                        "Failed to create hard link from {:?} to {:?}",
                        source,
                        destination
                    ))?;
                }
                progress.increment(1);
            }
        }

        Ok(())
    }

    pub fn execute(
        &self,
        progress: &mut printer::MultiProgressBar,
        workspace: workspace::WorkspaceArc,
        name: &str,
    ) -> anyhow::Result<()> {
        let mut capsule_run_info = CapsuleRunInfo::new(
            workspace.clone(),
            self.descriptor.clone(),
            &self.scripts,
        )
        .context(format_context!("Failed to create capsule run info"))?;
        let capsule_workspace_path = capsule_run_info.get_path_to_capsule_workspace();

        logger(progress, name)
            .info(format!("Executing spaces capsule in {capsule_workspace_path}").as_str());

        let run_status = capsule_run_info
            .try_lock()
            .context(format_context!("Failed to lock capsule"))?;

        logger(progress, name).message(
            format!(
                "Capsule run status for {} is {:?}",
                capsule_run_info.get_short_digest(),
                run_status
            )
            .as_str(),
        );

        let capsule_sysroot_prefix = capsule_run_info.get_path_to_capsule_sysroot();

        // check if capsule already exists the store
        let capsule_json_file_path = format!(
            "{capsule_sysroot_prefix}/{}.json",
            capsule_run_info.get_short_digest()
        );

        logger(progress, name)
            .debug(format!("Does capsule JSON file exist? {}", capsule_json_file_path).as_str());

        if !std::path::Path::new(capsule_json_file_path.as_str()).exists() {
            self.checkout_capsule(progress, &capsule_run_info)
                .context(format_context!("Failed to checkout capsule {name}"))?;

            // check capsules.spaces.json for a valid CapsuleCheckoutInfo struct
            let capsule_info = {
                let mut state = get_state().write();

                let capsule_info = load_file_info(&capsule_workspace_path).context(
                    format_context!("Failed to load {}", workspace::SPACES_CAPSULES_INFO_NAME),
                )?;

                state.info_file.clone_from(&capsule_info);
                capsule_info
            };

            if run_status == CapsuleRunStatus::StartNow {
                logger(progress, name).info(
                    format!(
                        "`spaces run` for capsule {}",
                        capsule_run_info.get_short_digest()
                    )
                    .as_str(),
                );

                self.run_capsule(progress, &capsule_run_info)
                    .context(format_context!("Failed to run capsule {name}"))?;

                logger(progress, name).message(
                    format!(
                        "Ready to unlock capsule {}",
                        capsule_run_info.get_short_digest()
                    )
                    .as_str(),
                );

                let capsule_complete_info = CapsuleCompleteInfo {
                    digest: capsule_run_info.digest.clone(),
                };

                let capsule_info_json = serde_json::to_string_pretty(&capsule_complete_info)
                    .context(format_context!("Failed to serialize capsule info"))?;

                logger(progress, name)
                    .debug(format!("Updating capsule info {}", capsule_run_info.digest).as_str());

                for entry in capsule_info.iter() {
                    let file_path = format!(
                        "{}/{}.json",
                        entry.prefix,
                        capsule_run_info.get_short_digest()
                    );

                    std::fs::write(file_path.as_str(), capsule_info_json.as_str()).context(
                        format_context!("Failed to write capsule info to {file_path}"),
                    )?;
                }
            } else {
                logger(progress, name).info(
                    format!(
                        "waiting for capsule {}",
                        capsule_run_info.get_short_digest()
                    )
                    .as_str(),
                );

                capsule_run_info
                    .wait(progress)
                    .context(format_context!("Failed to wait for capsule to finish"))?;
            }
        } else {
            logger(progress, name).info(
                format!(
                    "{} is already in spaces store",
                    capsule_run_info.get_short_digest()
                )
                .as_str(),
            );
        }

        self.hard_link_capsule_to_workspace(progress, capsule_sysroot_prefix.as_ref())
            .context(format_context!("Failed to hard link capsule to workspace"))?;

        logger(progress, name)
            .message(format!("Now unlocking {}", capsule_run_info.get_short_digest()).as_str());

        Ok(())
    }
}
