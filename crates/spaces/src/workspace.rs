use anyhow::Context;
use anyhow_source_location::{format_context, format_error};

pub const WORKSPACE_FILE_NAME: &str = "spaces.workspace.star";
pub const SPACES_MODULE_NAME: &str = "spaces.star";
pub const WORKSPACE_FILE_HEADER: &str = r#"
"""
Spaces Workspace file
"""
"#;

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
