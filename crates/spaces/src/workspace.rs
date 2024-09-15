use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::RwLock;

pub const WORKSPACE_FILE_NAME: &str = "spaces.workspace.star";
pub const SPACES_MODULE_NAME: &str = "spaces.star";
pub const WORKSPACE_FILE_HEADER: &str = r#"
"""
Spaces Workspace file
"""
"#;

struct State {
    workspace_path: Option<String>,
    workspace_log_folder: Option<String>,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(RwLock::new(State {
        workspace_path: None,
        workspace_log_folder: None,
    }));
    STATE.get()
}

pub fn get_workspace_log_file(rule_name: &str) -> anyhow::Result<String> {
    let state = get_state().read().unwrap();
    let log_folder = state
        .workspace_log_folder
        .as_ref()
        .ok_or(format_error!("Internal Error: No workspace path"))?;

    let rule_name = rule_name.replace('/', "_");
    let rule_name = rule_name.replace(':', "_");

    Ok(format!("{log_folder}/{rule_name}.log"))
}


pub fn set_workspace_path(path: String) -> anyhow::Result<()> {
    let mut state = get_state().write().unwrap();
    let date = chrono::Local::now();
    let log_folder = format!("{path}/spaces_logs/logs_{}", date.format("%Y%m%d-%H-%M-%S"));
    std::fs::create_dir_all(log_folder.as_str())
        .context(format_context!("Failed to create log folder {log_folder}"))?;
    state.workspace_log_folder = Some(log_folder);
    state.workspace_path = Some(path);

    Ok(())
}


pub fn get_workspace_path() -> Option<String> {
    let state = get_state().read().unwrap();
    state.workspace_path.clone()
}

pub fn get_workspace_io_path() -> anyhow::Result<String> {
    if let Some(workspace_path) = get_workspace_path() {
        let state = get_state().read().unwrap();
        std::fs::create_dir_all(format!("{}/build", workspace_path).as_str())
            .context(format_context!("Failed to create io.spaces directory"))?;
        Ok(format!("{}/build/io.spaces", workspace_path))
    } else {
        Err(format_error!("Internal Error: No workspace path"))
    }

}


#[derive(Debug)]
pub struct Workspace {
    pub absolute_path: String,
    pub modules: Vec<(String, String)>,
}

impl Workspace {
    fn find_workspace_root(current_working_directory: &str) -> anyhow::Result<String> {
        let mut current_directory = current_working_directory;
        loop {
            let workspace_path = format!("{}/{}", current_directory, WORKSPACE_FILE_NAME);
            if std::path::Path::new(workspace_path.as_str()).exists() {
                return Ok(current_directory.to_string());
            }
            let parent_directory = std::path::Path::new(current_directory).parent();
            if parent_directory.is_none() {
                return Err(format_error!(
                    "Failed to find {} in any parent directory",
                    WORKSPACE_FILE_NAME
                ));
            }
            current_directory = parent_directory.unwrap().to_str().unwrap();
        }
    }

    pub fn new(
        mut progress: printer::MultiProgressBar,
        current_working_directory: &str,
    ) -> anyhow::Result<Self> {
        // search the current directory and all parent directories for the workspace file
        let absolute_path = Self::find_workspace_root(current_working_directory)
            .context(format_context!("While searching for workspace root"))?;

        // walkdir and find all spaces.star files in the workspace
        let walkdir: Vec<_> = walkdir::WalkDir::new(absolute_path.as_str())
            .into_iter()
            .collect();

        progress.set_prefix("scanning workspace");
        progress.set_total(walkdir.len() as u64);

        let workspace_content = std::fs::read_to_string(format!("{}/{}", absolute_path, WORKSPACE_FILE_NAME))
            .context(format_context!("Failed to read workspace file"))?;

        let mut modules = vec![(WORKSPACE_FILE_NAME.to_string(), workspace_content)];
        for entry in walkdir {
            progress.increment(1);
            if let Ok(entry) = entry.context(format_context!("While walking directory")) {
                if entry.file_type().is_file() && entry.file_name() == SPACES_MODULE_NAME {
                    let path = entry.path().to_string_lossy().to_string();
                    let content = std::fs::read_to_string(path.as_str())
                        .context(format_context!("Failed to read file {}", path))?;

                    if let Some(path) = path.strip_prefix(format!("{}/", absolute_path).as_str()) {
                        modules.push((path.to_owned(), content));
                    }
                }
            }
        }

        Ok(Self {
            absolute_path,
            modules,
        })
    }
}
