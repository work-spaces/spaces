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

        Ok(GithubLogGroup { is_github })
    }

    pub fn end_group(&self, console: console::Console, is_ci: IsCi) -> anyhow::Result<()> {
        if self.is_github && is_ci == IsCi::Yes {
            console.raw("::endgroup::\n")?;
        }
        Ok(())
    }
}
