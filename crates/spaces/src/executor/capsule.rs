use crate::{executor, state_lock, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};


fn get_process_group_id() -> String {
    if let Ok(process_group_id) = std::env::var(workspace::SPACES_PROCESS_GROUP_ENV_VAR) {
        process_group_id
    } else {
        // create process ID from system time
        format!("{}", chrono::Utc::now().timestamp())
    }
}

fn get_capsule_digest(capsules_path: &str, scripts: &Vec<String>) -> anyhow::Result<String> {
    let mut modules = Vec::new();
    for script in scripts {
        let mut effective_script = script.clone();
        if !effective_script.ends_with(".spaces.star") {
            effective_script.push_str(".spaces.star");
        }
        let script_path = format!("{}/{}", capsules_path, effective_script);

        let current_working_directory = workspace::get_current_working_directory().context(
            format_context!("Failed to get current working when reading {script_path}"),
        )?;

        let content = std::fs::read_to_string(script_path.as_str()).context(format_context!(
            "Failed to read script {} from {}",
            script_path,
            current_working_directory
        ))?;
        modules.push((script.clone(), content));
    }
    Ok(workspace::calculate_digest(&modules))
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub enum DependencyType {
    Build,
    Runtime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Descriptor {
    pub domain: String, // domain of the capsule
    pub owner: String,  // owner of the capsule
    pub repo: String,   // repo of the capsule
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    descriptor: Descriptor, // descriptor of the capsule
    version: String,        // Version of the capsule
    prefix: String,         // --prefix location where the capsule is available when installed
}

// capsules.spaces.json
type InfoFile = Vec<Info>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleCheckoutInfo {
    pub digest: String,  // The workspace digest
    pub info: Vec<Info>, // List of capsules that are available to build
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleCompleteInfo {
    pub digest: String, // The workspace digest
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CapsuleRunStatus {
    AlreadyStarted,
    StartNow,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapsuleRunInfo {
    pub process_group_id: String, // The workspace digest
    pub digest: String,
    pub is_locked: bool,
}

impl CapsuleRunInfo {
    fn get_run_info_path(&self) -> String {
        format!(
            "{}/capsules/run/{}.json",
            workspace::get_store_path(),
            self.digest
        )
    }

    fn lock(
        &mut self,
        capsules_path: &str,
        scripts: &Vec<String>,
    ) -> anyhow::Result<CapsuleRunStatus> {
        self.process_group_id = get_process_group_id();
        self.digest = get_capsule_digest(capsules_path, scripts)
            .context(format_context!("Failed to get capsule digest"))?;

        let run_info_dir = format!("{}/capsules/run", workspace::get_store_path());

        std::fs::create_dir_all(run_info_dir.as_str())
            .context(format_context!("Failed to create {run_info_dir}"))?;

        let capsule_run_info_path = self.get_run_info_path();

        match std::fs::OpenOptions::new()
            .write(true) // Open for writing
            .create_new(true) // Create only if it does NOT exist
            .open(capsule_run_info_path.as_str())
        {
            Ok(file) => {
                serde_json::to_writer(file, &self)
                    .context(format_context!("Failed to write {capsule_run_info_path}"))?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let contents = std::fs::read_to_string(capsule_run_info_path.as_str())
                    .context(format_context!("Failed to read {capsule_run_info_path}"))?;
                let existing_info: CapsuleRunInfo =
                    serde_json::from_str(&contents).context(format_context!(
                        "failed to parse {capsule_run_info_path} - delete the file and try again"
                    ))?;

                if existing_info.process_group_id == self.process_group_id {
                    return Ok(CapsuleRunStatus::AlreadyStarted);
                } else {
                    let capsule_run_info_string = serde_json::to_string(&self)
                        .context(format_context!("Failed to serialize capsule run info"))?;

                    // over write the file
                    std::fs::write(capsule_run_info_path.as_str(), capsule_run_info_string)
                        .context(format_context!(
                            "Failed to create file {capsule_run_info_path}"
                        ))?;
                }
            }
            Err(err) => {
                return Err(format_error!(
                    "Failed to create file '{}': {err:?} - delete the file and try again",
                    capsule_run_info_path
                ));
            }
        }
        self.is_locked = true;
        Ok(CapsuleRunStatus::StartNow)
    }

    fn unlock(&mut self) -> anyhow::Result<()> {
        let capsule_run_info_path = self.get_run_info_path();
        std::fs::remove_file(capsule_run_info_path.as_str())
            .context(format_context!("Failed to remove {capsule_run_info_path}"))?;
        Ok(())
    }

    fn wait(&self, progress: &mut printer::MultiProgressBar) -> anyhow::Result<()> {
        let path = self.get_run_info_path();
        progress.set_message("Capsule already started, waiting for it to finish");
        let capsule_run_info_path = std::path::Path::new(&path);
        let mut log_count = 0;
        while capsule_run_info_path.exists() {
            let contents = std::fs::read_to_string(capsule_run_info_path)
                .context(format_context!("Failed to read {path}"))?;

            let lock_info: CapsuleRunInfo = serde_json::from_str(&contents).context(
                format_context!("failed to parse {path} - delete the file and try again"),
            )?;

            if lock_info.process_group_id != self.process_group_id {
                progress.log(
                    printer::Level::Message,
                    format!("Capsule {} is no longer running, unlocking", self.digest).as_str(),
                );
                return Ok(());
            }

            progress.increment(1);
            std::thread::sleep(std::time::Duration::from_millis(500));
            log_count += 1;
            if log_count == 10 {
                progress.log(
                    printer::Level::Debug,
                    format!("Still waiting for capsule to finish at {}", path).as_str(),
                );
                log_count = 0;
            }
        }
        Ok(())
    }
}

impl Drop for CapsuleRunInfo {
    fn drop(&mut self) {
        if !self.is_locked {
            return;
        }

        let _ = self.unlock();
    }
}

#[derive(Debug)]
struct State {
    info_file: InfoFile,
}

static STATE: state::InitCell<state_lock::StateLock<State>> = state::InitCell::new();

fn get_state() -> &'static state_lock::StateLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }

    let info_file_path = workspace::SPACES_CAPSULES_INFO_NAME;

    let info_file = if std::path::Path::new(info_file_path).exists() {
        let info_file = load_file_info(".");
        match info_file {
            Ok(info) => info,
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    STATE.set(state_lock::StateLock::new(State { info_file }));
    STATE.get()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capsule {
    pub scripts: Vec<String>,      // list of starlark scripts to execute
    pub prefix: Option<String>, // --prefix location where the capsule should be installed in the sysroot (default is none)
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

fn get_spaces_command() -> anyhow::Result<String> {
    let spaces_exec = which::which("spaces").context("Failed to find spaces executable")?;
    Ok(spaces_exec.to_string_lossy().to_string())
}

fn get_spaces_env() -> anyhow::Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    let workspace_env = workspace::get_env();
    env.insert(
        workspace::SPACES_PROCESS_GROUP_ENV_VAR.to_string(),
        get_process_group_id(),
    );

    env.extend(
        workspace_env
            .get_inherited_vars()
            .context(format_context!("Failed to get inherited vars"))?,
    );

    Ok(env)
}

impl Capsule {
    fn checkout_capsule(
        &self,
        name: &str,
        spaces_command: String,
        workspace_name: &str,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let mut args = vec![
            "--hide-progress-bars".to_string(),
            "--verbosity=debug".to_string(),
            "checkout".to_string(),
        ];

        args.extend(self.scripts.iter().map(|e| format!("--script={e}")));
        args.push(format!("--name={workspace_name}"));

        std::fs::create_dir_all(workspace::SPACES_CAPSULES_NAME)
            .context(format_context!("Failed to create @capsules"))?;

        let mut env = HashMap::new();
        env.insert(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        );

        env.extend(get_spaces_env().context(format_context!("Failed to get spaces env"))?);

        // run spaces checkout in @capsules using name
        let spaces_checkout = executor::exec::Exec {
            command: spaces_command,
            args: Some(args),
            working_directory: Some(workspace::SPACES_CAPSULES_NAME.to_string()),
            env: Some(env),
            redirect_stdout: None,
            expect: None,
        };

        let checkout_name = format!("{}_checkout", name);
        spaces_checkout
            .execute(&checkout_name, progress)
            .context(format_context!("Failed to checkout workflow {name}"))?;

        Ok(())
    }

    fn run_capsule(
        &self,
        name: &str,
        spaces_command: String,
        workspace_path: String,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        let args = vec![
            "--hide-progress-bars".to_string(),
            "--verbosity=debug".to_string(),
            "run".to_string(),
        ];

        // run spaces checkout in @capsules using name
        let spaces_run = executor::exec::Exec {
            command: spaces_command,
            args: Some(args),
            working_directory: Some(workspace_path),
            env: Some(get_spaces_env().context(format_context!("Failed to get spaces env"))?),
            redirect_stdout: None,
            expect: None,
        };

        let run_name = format!("{}_run", name);
        spaces_run
            .execute(&run_name, progress)
            .context(format_context!("Failed to checkout workflow {name}"))?;

        Ok(())
    }

    fn hard_link_capsule_to_workspace(
        &self,
        capsule_prefix: &str,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        if let Some(prefix) = self.prefix.as_ref() {
            // walkdir on capsule prefix - hard link files to the workspace
            let walker = walkdir::WalkDir::new(capsule_prefix);
            let walker_list: Vec<_> = walker.into_iter().collect();

            progress.set_total(walker_list.len() as u64);

            progress.log(
                printer::Level::Info,
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
                    let prefix_path = std::path::Path::new(prefix);
                    let destination_path = prefix_path.join(relative_path);

                    if let Some(parent) = destination_path.parent() {
                        std::fs::create_dir_all(parent).context(format_context!(
                            "Failed to create parent directory {:?}",
                            parent
                        ))?;
                    }

                    let destination = destination_path.to_string_lossy().to_string();
                    let source = source_path.to_string_lossy().to_string();
                    progress.log(
                        printer::Level::Trace,
                        format!("Hard linking {:?} to {:?}", source, destination).as_str(),
                    );

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
        name: &str,
        progress: &mut printer::MultiProgressBar,
    ) -> anyhow::Result<()> {
        // create add_workflow.spaces.star - pass it as the first script
        let spaces_command =
            get_spaces_command().context(format_context!("While executing capsule run"))?;
        let workspace_name = name.replace(':', "_");

        let mut capsule_run_info = CapsuleRunInfo::default();
        let workspace_path = format!("{}/{}", workspace::SPACES_CAPSULES_NAME, workspace_name);

        progress.log(
            printer::Level::Info,
            format!("Executing spaces capsule in {workspace_path}").as_str(),
        );

        let run_status = capsule_run_info
            .lock(workspace::SPACES_CAPSULES_NAME, &self.scripts)
            .context(format_context!("Failed to lock capsule"))?;

        progress.log(
            printer::Level::Message,
            format!(
                "Capsule run status for {} is {:?}",
                capsule_run_info.digest, run_status
            )
            .as_str(),
        );

        self.checkout_capsule(name, spaces_command.clone(), &workspace_name, progress)
            .context(format_context!("Failed to checkout capsule {name}"))?;

        // check capsules.spaces.json for a valid CapsuleCheckoutInfo struct
        let capsule_info = {
            let mut state = get_state().write();

            let capsule_info = load_file_info(&workspace_path)
                .context(format_context!("Failed to load capsules.spaces.json"))?;

            state.info_file.clone_from(&capsule_info);
            capsule_info
        };

        if run_status == CapsuleRunStatus::StartNow {
            progress.log(
                printer::Level::Info,
                format!("`spaces run` for capsule {}", capsule_run_info.digest).as_str(),
            );

            self.run_capsule(name, spaces_command, workspace_path, progress)
                .context(format_context!("Failed to run capsule {name}"))?;

            progress.log(
                printer::Level::Message,
                format!("Ready to unlock capsule {}", capsule_run_info.digest).as_str(),
            );

            let capsule_complete_info = CapsuleCompleteInfo {
                digest: capsule_run_info.digest.clone(),
            };

            let capsule_info_json = serde_json::to_string_pretty(&capsule_complete_info)
                .context(format_context!("Failed to serialize capsule info"))?;

            progress.log(
                printer::Level::Debug,
                format!("Updating capsule info {}", capsule_run_info.digest).as_str(),
            );

            for entry in capsule_info.iter() {
                let file_path = format!("{}/{}.json", entry.prefix, capsule_run_info.digest);
                std::fs::write(file_path.as_str(), capsule_info_json.as_str()).context(
                    format_context!("Failed to write capsule info to {file_path}"),
                )?;
            }

        } else {
            progress.log(
                printer::Level::Info,
                format!("waiting for capsule {}", capsule_run_info.digest).as_str(),
            );

            capsule_run_info
                .wait(progress)
                .context(format_context!("Failed to wait for capsule to finish"))?;
        }

        if self.prefix.is_some() {
            let mut capsule_prefix = HashSet::new();
            for entry in capsule_info.iter() {
                capsule_prefix.insert(entry.prefix.clone());
            }

            for prefix in capsule_prefix {
                self.hard_link_capsule_to_workspace(&prefix, progress)
                    .context(format_context!("Failed to hard link capsule to workspace"))?;
            }
        }

        progress.log(
            printer::Level::Message,
            format!("Now unlocking {}", capsule_run_info.digest).as_str(),
        );

        Ok(())
    }
}
