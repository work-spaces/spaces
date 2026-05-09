#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IsCi {
    No,
    Yes,
}

impl From<bool> for IsCi {
    fn from(value: bool) -> Self {
        if value { IsCi::Yes } else { IsCi::No }
    }
}

pub fn is_github_actions() -> bool {
    std::env::var("GITHUB_ACTIONS")
        .map(|e| e == "true")
        .unwrap_or(false)
}

pub struct GithubLogGroup {
    is_github: bool,
    is_ci: IsCi,
    console: console::Console,
}

impl GithubLogGroup {
    pub fn new_group(
        console: console::Console,
        is_ci: IsCi,
        group_name: &str,
    ) -> anyhow::Result<Self> {
        let is_github = is_github_actions();

        if is_github && is_ci == IsCi::Yes {
            console.raw(format!("::group::{group_name}\n").as_str())?;
        }

        Ok(GithubLogGroup {
            is_github,
            is_ci,
            console,
        })
    }

    fn end_group(&self) -> anyhow::Result<()> {
        if self.is_github && self.is_ci == IsCi::Yes {
            self.console.raw("::endgroup::\n")?;
        }
        Ok(())
    }
}

impl Drop for GithubLogGroup {
    fn drop(&mut self) {
        let _ = self.end_group();
    }
}
