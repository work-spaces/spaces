use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::RwLock;

pub const ENV_FILE_NAME: &str = "env.spaces.star";
pub const SPACES_MODULE_NAME: &str = "spaces.star";
pub const SPACES_CHECKOUT_NAME: &str = "checkout.star";
pub const SPACES_STDIN_NAME: &str = "stdin.star";
pub const WORKSPACE_FILE_HEADER: &str = r#"
"""
Spaces Workspace file
"""
"#;

struct State {
    absolute_path: String,
    log_directory: String,
}

static STATE: state::InitCell<RwLock<State>> = state::InitCell::new();

pub fn is_rules_module(path: &str) -> bool {
    path.ends_with(SPACES_MODULE_NAME)
}

pub fn is_checkout_script(path: &str) -> bool {
    path.ends_with(SPACES_CHECKOUT_NAME)
}   

pub fn get_store_path() -> String {
    if let Ok(spaces_home) = std::env::var("SPACES_HOME") {
        return format!("{}/.spaces/store", spaces_home);
    }
    if let Ok(Some(home_path)) = homedir::my_home() {
        return format!("{}/.spaces/store", home_path.to_string_lossy());
    }
    panic!("Failed to get home directory");
}

pub fn get_spaces_tools_path() -> String {
    format!("{}/spaces_tools", get_store_path())
}

pub fn get_cargo_binstall_root() -> String {
    format!("{}/cargo_binstall_bin_dir", get_spaces_tools_path())
}

fn get_unique() -> anyhow::Result<String> {
    let duration_since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context(format_context!("No system time"))?;
    let duration_since_epoch_string = format!("{}", duration_since_epoch.as_nanos());
    let unique_sha256 = sha256::digest(duration_since_epoch_string.as_bytes());
    Ok(unique_sha256.as_str()[0..4].to_string())
}

fn get_state() -> &'static RwLock<State> {
    if let Some(state) = STATE.try_get() {
        return state;
    }
    STATE.set(RwLock::new(State {
        absolute_path: "".to_string(),
        log_directory: "spaces_logs".to_string(),
    }));
    STATE.get()
}

pub fn get_log_file(rule_name: &str) -> String {
    let state = get_state().read().unwrap();
    let rule_name = rule_name.replace('/', "_");
    let rule_name = rule_name.replace(':', "_");
    format!("{}/{rule_name}.log", state.log_directory)
}

pub fn build_directory() -> &'static str {
    "build"
}

pub fn get_io_path() -> &'static str {
    "build/io.spaces"
}

pub fn absolute_path() -> String {
    get_state().read().unwrap().absolute_path.clone()
}

#[derive(Debug)]
pub struct Workspace {
    pub modules: Vec<(String, String)>,
}

impl Workspace {
    fn find_workspace_root(current_working_directory: &str) -> anyhow::Result<String> {
        let mut current_directory = current_working_directory.to_owned();
        loop {
            let workspace_path = format!("{}/{}", current_directory, ENV_FILE_NAME);
            if std::path::Path::new(workspace_path.as_str()).exists() {
                return Ok(current_directory.to_string());
            }
            let parent_directory = std::path::Path::new(current_directory.as_str()).parent();
            if parent_directory.is_none() {
                return Err(format_error!(
                    "Failed to find {} in any parent directory",
                    ENV_FILE_NAME
                ));
            }
            current_directory = parent_directory.unwrap().to_string_lossy().to_string();
        }
    }

    pub fn new(mut progress: printer::MultiProgressBar) -> anyhow::Result<Self> {
        let date = chrono::Local::now();

        let current_working_directory = std::env::current_dir()
            .context(format_context!("Failed to get current working directory"))?
            .to_string_lossy()
            .to_string();

        // search the current directory and all parent directories for the workspace file
        let absolute_path = Self::find_workspace_root(current_working_directory.as_str())
            .context(format_context!("While searching for workspace root"))?;

        // walkdir and find all spaces.star files in the workspace
        let walkdir: Vec<_> = walkdir::WalkDir::new(absolute_path.as_str())
            .into_iter()
            .collect();

        progress.set_total(walkdir.len() as u64);

        let env_content =
            std::fs::read_to_string(format!("{}/{}", absolute_path, ENV_FILE_NAME))
                .context(format_context!("Failed to read workspace file"))?;

        let mut modules = vec![(ENV_FILE_NAME.to_string(), env_content)];
        for entry in walkdir {
            progress.increment(1);
            if let Ok(entry) = entry.context(format_context!("While walking directory")) {
                if entry.file_type().is_file()
                    && is_rules_module(entry.file_name().to_string_lossy().as_ref())
                {
                    let path = entry.path().to_string_lossy().to_string();
                    let content = std::fs::read_to_string(path.as_str())
                        .context(format_context!("Failed to read file {}", path))?;

                    if let Some(path) = path.strip_prefix(format!("{}/", absolute_path).as_str()) {
                        modules.push((path.to_owned(), content));
                    }
                }
            }
        }

        std::env::set_current_dir(std::path::Path::new(absolute_path.as_str())).context(
            format_context!("Failed to set current directory to {absolute_path}"),
        )?;

        let mut state = get_state().write().unwrap();

        state.log_directory = format!("spaces_logs/logs_{}", date.format("%Y%m%d-%H-%M-%S"));

        std::fs::create_dir_all(state.log_directory.as_str()).context(format_context!(
            "Failed to create log folder {}",
            state.log_directory
        ))?;

        std::fs::create_dir_all(build_directory())
            .context(format_context!("Failed to create build directory"))?;

        state.absolute_path = absolute_path;

        #[allow(unused)]
        let unique = get_unique().context(format_context!("failed to get unique marker"))?;

        Ok(Self { modules })
    }
}
