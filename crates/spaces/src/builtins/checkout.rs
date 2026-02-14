use crate::executor::asset;
use crate::workspace::WorkspaceArc;
use crate::{executor, rules, singleton, task};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use serde::{Deserialize, Serialize};
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;
use std::sync::Arc;
use utils::{changes, environment, git, http_archive, platform, rule};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PlatformArchive {
    #[serde(rename = "macos-x86_64")]
    pub macos_x86_64: Option<http_archive::Archive>,
    #[serde(rename = "macos-aarch64")]
    pub macos_aarch64: Option<http_archive::Archive>,
    #[serde(rename = "windows-x86_64")]
    pub windows_aarch64: Option<http_archive::Archive>,
    #[serde(rename = "windows-aarch64")]
    pub windows_x86_64: Option<http_archive::Archive>,
    #[serde(rename = "linux-x86_64")]
    pub linux_x86_64: Option<http_archive::Archive>,
    #[serde(rename = "linux-aarch64")]
    pub linux_aarch64: Option<http_archive::Archive>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct CargoBin {
    #[serde(rename = "crate")]
    crate_: String,
    bins: Vec<String>,
    version: String,
}

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Abort script evaluation with a message.
    ///
    /// ```python
    /// checkout.abort("Failed to do something")
    /// ```
    ///
    /// # Arguments
    /// * `message`: Abort message to show the user.
    ///
    fn abort(message: &str) -> anyhow::Result<NoneType> {
        Err(format_error!("Checkout Aborting: {}", message))
    }

    /// Adds a target to organize dependencies.
    ///
    /// ```python
    /// checkout.add_target(
    ///     rule = {"name": "my_rule", "deps": ["my_other_rule"]},
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` containing the rule definition (e.g., `name`, `deps`, `platforms`, `type`, and `help`).
    fn add_target(
        #[starlark(require = named)] rule: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add target rule"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::Target,
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    /// Adds a process to execute during checkout.
    ///
    /// ```python
    /// checkout.add_exec(
    ///     rule = {"name": "my_rule", "deps": ["my_other_rule"]},
    ///     exec = {"command": "ls", "arguments": ["-l"]}
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` containing the rule definition.
    /// * `exec`: A `dict` containing the execution details.
    fn add_exec(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] exec: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for exec rule"))?;

        if rule.inputs.is_some() {
            return Err(format_error!(
                "Cannot specify inputs for checkout.add_exec()"
            ));
        }

        let mut exec: executor::exec::Exec = serde_json::from_value(exec.to_json_value()?)
            .context(format_context!("bad options for exec"))?;

        if let Some(working_directory) = exec.working_directory.as_mut() {
            *working_directory = rules::get_sanitized_working_directory(working_directory.clone());
        }

        if let Some(redirect_stdout) = exec.redirect_stdout.as_mut() {
            *redirect_stdout = format!("build/{redirect_stdout}").into();
        }

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::Exec(exec),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    /// Adds a git repository to the workspace.
    ///
    /// ```python
    /// checkout.add_repo(
    ///     # the rule name is also the path in the workspace where the clone will be
    ///     rule = { "name": "spaces" },
    ///     repo = {
    ///         "url": "[https://github.com/work-spaces/spaces](https://github.com/work-spaces/spaces)",
    ///         "rev": "main",
    ///         "checkout": "Revision",
    ///         "clone": "Default",
    ///         "is_evaluate_spaces_modules": True
    ///     }
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: rule definition containing
    /// * `repo`: repository details containing
    ///
    fn add_repo(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] repo: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for repo rule"))?;

        let repo: git::Repo = serde_json::from_value(repo.to_json_value()?)
            .context(format_context!("bad options for repo"))?;

        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;

        let worktree_path = if let Some(directory) = repo.working_directory.as_ref() {
            directory.clone()
        } else {
            let workspace = workspace_arc.read();
            workspace.get_absolute_path()
        };

        if let Some(clone_type) = repo.clone.as_ref() {
            if *clone_type == git::Clone::Worktree && repo.sparse_checkout.is_some() {
                return Err(format_error!(
                    "Sparse checkout is not supported with Worktree clone"
                ));
            }
        }

        let checkout = repo.get_checkout();
        let spaces_key = rule.name.clone();
        let rule_name = rule.name.clone();
        let url = repo.url.trim_end_matches('/');
        let url: Arc<str> = url.strip_suffix(".git").unwrap_or(url).into();

        add_git_url_to_workspace_store_queue(
            workspace_arc.clone(),
            url.as_ref(),
            if repo.is_cow_semantics() { "cow/" } else { "" },
        )
        .context(format_context!("during checkout add repo"))?;

        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::Git(executor::git::Git {
                url,
                spaces_key,
                worktree_path,
                checkout,
                clone: repo.clone.unwrap_or(git::Clone::Default),
                is_evaluate_spaces_modules: repo.is_evaluate_spaces_modules.unwrap_or(true),
                sparse_checkout: repo.sparse_checkout,
                working_directory: repo.working_directory,
            }),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    /// Adds a binary crate using cargo-binstall.
    ///
    /// ```python
    /// checkout.add_cargo_bin(
    ///     rule = {"name": "probe-rs-tools"},
    ///     cargo_bin = {
    ///         "crate": "probe-rs-tools",
    ///         "version": "0.24.0",
    ///         "bins": ["probe-rs", "cargo-embed", "cargo-flash"]
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `cargo_bin`: A `dict` of crate details containing `crate` (`str`), `version` (`str`), and `bins` (`list`).
    fn add_cargo_bin(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] cargo_bin: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let cargo_bin: CargoBin = serde_json::from_value(cargo_bin.to_json_value()?)
            .context(format_context!("bad options for cargo_bin"))?;

        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for cargo_bin rule"))?;

        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;
        let workspace = workspace_arc.read();

        let cargo_binstall_dir = workspace.get_cargo_binstall_root();

        let output_directory = format!("{}/{}", cargo_binstall_dir, cargo_bin.version);

        std::fs::create_dir_all(output_directory.as_str()).context(format_context!(
            "Failed to create directory {output_directory}"
        ))?;

        let hard_link_rule = rule.clone();

        let cargo_binstall_path = format!(
            "{}/sysroot/bin/cargo-binstall",
            workspace.get_spaces_tools_path()
        );

        let exec = executor::exec::Exec {
            command: cargo_binstall_path.into(),
            args: Some(vec![
                format!("--version={}", cargo_bin.version).into(),
                format!("--root={output_directory}").into(),
                "--no-confirm".into(),
                cargo_bin.crate_.into(),
            ]),
            env: None,
            working_directory: None,
            redirect_stdout: None,
            expect: None,
            log_level: None,
            timeout: None,
        };

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::Exec(exec),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        for bin in cargo_bin.bins {
            let mut bin_rule = hard_link_rule.clone();
            bin_rule.name = format!("{}/{}", hard_link_rule.name, bin).into();

            // cargo install uses the root/bin install directory
            let output_file = format!("{output_directory}/bin/{bin}");

            let rule_name = hard_link_rule.name.clone();
            rules::insert_task(task::Task::new(
                bin_rule,
                task::Phase::PostCheckout,
                executor::Task::AddHardLink(asset::AddHardLink {
                    source: output_file,
                    destination: format!("sysroot/bin/{bin}"),
                }),
            ))
            .context(format_context!("Failed to insert task {rule_name}"))?;
        }

        Ok(NoneType)
    }

    /// Adds an archive to the workspace based on the platform.
    ///
    /// ```python
    /// base = {
    ///     "add_prefix": "sysroot/bin",
    ///     "strip_prefix": "target/release",
    ///     "link": "Hard",
    /// }
    ///
    /// macos_x86_64 = base | {
    ///     "url": "[https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-macos-latest-x86_64-v0.6.0-beta.13.zip](https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-macos-latest-x86_64-v0.6.0-beta.13.zip)",
    ///     "sha256": "47d325145e6f7f870426f1b123c781f89394b0458bb43f5abe2d36ac3543f7ef",
    /// }
    ///
    /// macos_aarch64 = base | {
    ///     "url": "[https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-macos-latest-aarch64-v0.6.0-beta.13.zip](https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-macos-latest-aarch64-v0.6.0-beta.13.zip)",
    ///     "sha256": "6dd972454942faa609670679c53b6876ab8e66bcfd0b583ee5a8d13c93b2e879",
    /// }
    ///
    /// linux_x86_64 = base | {
    ///     "url": "[https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-linux-gnu-x86_64-v0.6.0-beta.13.zip](https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-linux-gnu-x86_64-v0.6.0-beta.13.zip)",
    ///     "sha256": "39030124f18b338eceee09061fb305b522ada76f6a0562f9926ea0747b3ad440",
    /// }
    ///
    /// linux_aarch64 = base | {
    ///     "url": "https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-linux-gnu-aarch64-v0.6.0-beta.13.zip",
    ///     "sha256": "39030124f18b338eceee09061fb305b522ada76f6a0562f9926ea0747b3ad440",
    /// }
    ///
    /// windows_x86_64 = base | {
    ///     "url": "[https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-windows-latest-x86_64-v0.6.0-beta.13.exe](https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-windows-latest-x86_64-v0.6.0-beta.13.exe)",
    ///     "sha256": "b93dc96b2c66fcfc4aef851db2064f6e6ecb54b29968ca5174f6b892b99651c8",
    /// }
    ///
    /// windows_aarch64 = base | {
    ///     "url": "[https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-windows-latest-aarch64-v0.6.0-beta.13.exe](https://github.com/work-spaces/spaces/releases/download/v0.6.0-beta.13/spaces-windows-latest-aarch64-v0.6.0-beta.13.exe)",
    ///     "sha256": "c67c7b23897e0949843e248465d5444428fb287f89dcd45cec76dde4b2cdc6a9",
    /// }
    ///
    /// checkout.add_platform_archive(
    ///     # rule name is only the path in the workspace if add_prefix is not set
    ///     rule = {"name": "spaces"},
    ///     platforms = {
    ///         "macos-x86_64": macos_x86_64,
    ///         "macos-aarch64": macos_aarch64,
    ///         "windows-x86_64": windows_x86_64,
    ///         "windows-aarch64": windows_aarch64,
    ///         "linux-x86_64": linux_x86_64,
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `platforms`: A `dict` of platform keys (e.g., `macos-aarch64`, `linux-x86_64`) mapping to archive detail `dict`s.
    fn add_platform_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] platforms: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add platform archive rule"))?;
        //convert platforms to starlark value

        let platforms: PlatformArchive = serde_json::from_value(platforms.to_json_value()?)?;

        let platform_archive = match platform::Platform::get_platform() {
            Some(platform::Platform::MacosX86_64) => platforms.macos_x86_64,
            Some(platform::Platform::MacosAarch64) => platforms.macos_aarch64,
            Some(platform::Platform::WindowsX86_64) => platforms.windows_x86_64,
            Some(platform::Platform::WindowsAarch64) => platforms.windows_aarch64,
            Some(platform::Platform::LinuxX86_64) => platforms.linux_x86_64,
            Some(platform::Platform::LinuxAarch64) => platforms.linux_aarch64,
            _ => None,
        };

        if platform_archive.is_none() {
            return Err(format_error!(
                "Platform {} not supported by {}",
                platform::Platform::get_platform().unwrap(),
                rule.name
            ));
        }
        add_http_archive(rule, platform_archive)
            .context(format_context!("Failed to add archive"))?;

        Ok(NoneType)
    }

    /// Adds a hardlink to an executable file available on the `PATH`.
    ///
    /// ```python
    /// checkout.add_which_asset(
    ///     rule = { "name": "which_pkg_config" },
    ///     asset = {
    ///         "which": "pkg-config",
    ///         "destination": "sysroot/bin/pkg-config"
    ///     }
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `asset`: A `dict` containing `which` (`str`) and `destination` (`str`). Note: This creates system dependencies that may break workspace hermeticity.
    fn add_which_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for which asset rule"))?;

        let asset: asset::AddWhichAsset = serde_json::from_value(asset.to_json_value()?)
            .context(format_context!("Failed to parse add_which_asset arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::AddWhichAsset(asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    /// Creates a hardlink from a source to a destination path.
    ///
    /// ```python
    /// checkout.add_hard_link_asset(
    ///     rule = {
    ///         "name": "link_file",
    ///     },
    ///     asset = {
    ///         "source": "path/to/original/file.txt",
    ///         "destination": "sysroot/link/to/file.txt"
    ///     }
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition.
    /// * `asset`: A `dict` containing `source` and `destination` paths.
    fn add_hard_link_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for which asset rule"))?;

        let asset: asset::AddHardLink = serde_json::from_value(asset.to_json_value()?).context(
            format_context!("Failed to parse add_hard_link_asset arguments"),
        )?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::AddHardLink(asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    /// Creates a symbolic link from a source to a destination path.
    ///
    /// ```python
    /// checkout.add_soft_link_asset(
    ///     rule = {
    ///         "name": "symlink_file",
    ///     },
    ///     asset = {
    ///         "source": "path/to/original/file.txt",
    ///         "destination": "sysroot/symlink/to/file.txt"
    ///     }
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition.
    /// * `asset`: A `dict` containing `source` and `destination` paths for the symbolic link.

    fn add_soft_link_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for which asset rule"))?;

        let asset: asset::AddSoftLink = serde_json::from_value(asset.to_json_value()?).context(
            format_context!("Failed to parse add_soft_link_asset arguments"),
        )?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::AddSoftLink(asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    /// Adds any number of assets, with support for different asset types.
    ///
    /// ```python
    /// checkout.add_any_assets(
    ///     rule = {
    ///         "name": "add_multiple_files",
    ///     },
    ///     assets = [
    ///         { "type": "hardlink", "source": "path/to/file1.txt", "destination": "sysroot/file1.txt" },
    ///         { "type": "symlink", "source": "path/to/file2.txt", "destination": "sysroot/file2.txt" }
    ///     ]
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition.
    /// * `assets`: A `list` of asset dictionaries, where each dictionary specifies the asset's type and its properties (e.g., source and destination).
    fn add_any_assets(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] assets: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for which asset rule"))?;

        let any_assets: asset::AddAnyAssets = serde_json::from_value(assets.to_json_value()?)
            .context(format_context!("Failed to parse add_any_assets arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::AddAnyAssets(any_assets),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    /// Adds an archive to the workspace.
    ///
    /// ```python
    /// checkout.add_archive(
    ///     # the rule name is the path in the workspace where the archive will be extracted
    ///     rule = {"name": "llvm-project"},
    ///     archive = {
    ///         "url": "[https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-](https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-){}.zip".format(version),
    ///         "sha256": "27b5c7c745ead7e9147c78471b9053d4f6fc3bed94baf45f4e8295439f564bb8",
    ///         "link": "Hard",
    ///         "strip_prefix": "llvm-project-llvmorg-{}".format(version),
    ///         "add_prefix": "llvm-project",
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `archive`: A `dict` containing `url` (`str`), `sha256` (`str`), `link` (`None`|`Hard`), `globs` (`list`), `strip_prefix` (`str`), and `add_prefix` (`str`).
    fn add_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] archive: starlark::values::Value,
        // includes, excludes, strip_prefix
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add archive rule"))?;

        let archive: http_archive::Archive = serde_json::from_value(archive.to_json_value()?)
            .context(format_context!("Failed to parse add_archive arguments"))?;

        add_http_archive(rule, Some(archive)).context(format_context!("Failed to add archive"))?;
        Ok(NoneType)
    }

    fn add_oras_archive(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] oras_archive: starlark::values::Value,
        // includes, excludes, strip_prefix
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for oras rule"))?;

        let oras_archive: executor::oras::OrasArchive =
            serde_json::from_value(oras_archive.to_json_value()?)
                .context(format_context!("Failed to parse oras archive arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::OrasArchive(oras_archive),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    /// Adds a file to the workspace.
    ///
    /// ```python
    /// content = """
    /// # README
    ///
    /// This is how to use this workspace.
    /// """
    ///
    /// checkout.add_asset(
    ///     rule = {"name": "README.md"},
    ///     asset = {
    ///         "destination": "README.md",
    ///         "content": content,
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `asset`: A `dict` of asset details containing `content` (`str`) and `destination` (`str`).
    fn add_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for add asset rule"))?;

        let add_asset: executor::asset::AddAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse add_asset arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::AddAsset(add_asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
        Ok(NoneType)
    }

    /// Creates or updates an existing file containing structured data in the workspace.
    ///
    /// ```python
    /// cargo_vscode_task = {
    ///     "type": "cargo",
    ///     "problemMatcher": ["$rustc"],
    ///     "group": "build",
    /// }
    ///
    /// # Add some VS code tasks
    /// checkout.update_asset(
    ///     rule = {"name": "vscode_tasks"},
    ///     asset = {
    ///         "destination": ".vscode/tasks.json",
    ///         "format": "json",
    ///         "value": {
    ///             "tasks": [
    ///                 cargo_vscode_task | {
    ///                     "command": "build",
    ///                     "args": ["--manifest-path=spaces/Cargo.toml"],
    ///                     "label": "build:spaces",
    ///                 },
    ///                 cargo_vscode_task | {
    ///                     "command": "install",
    ///                     "args": ["--path=spaces", "--root=${userHome}/.local", "--profile=dev"],
    ///                     "label": "install_dev:spaces",
    ///                 }
    ///             ],
    ///         },
    ///     }
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `asset`: A `dict` containing `destination` (`str`), `format` (`json`|`toml`|`yaml`), and `value` (`dict`). Supports multi-rule updates to the same file if keys are unique.
    fn update_asset(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] asset: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for update asset rule"))?;
        // support JSON, yaml, and toml
        let update_asset: executor::asset::UpdateAsset =
            serde_json::from_value(asset.to_json_value()?)
                .context(format_context!("Failed to parse update_asset arguments"))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::PostCheckout,
            executor::Task::UpdateAsset(update_asset),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }

    /// Creates or updates the environment in the workspace during checkout.
    ///
    /// ```python
    /// checkout.update_env(
    ///     rule = {"name": "update_env"},
    ///     env = {
    ///         "paths": [],
    ///         "system_paths": ["/usr/bin", "/bin"],
    ///         "vars": {
    ///             "PS1": '"(spaces) $PS1"',
    ///         },
    ///         "inherited_vars": ["HOME", "SHELL", "USER"],
    ///         "optional_inherited_vars": ["TERM"],
    ///         "secret_inherited_vars": ["SSH_AUTH_SOCK"],
    ///     },
    /// )
    /// ```
    ///
    /// # Arguments
    /// * `rule`: A `dict` rule definition containing `name` (`str`), `deps` (`list`), `platforms` (`list`), `type` (`str`), and `help` (`str`).
    /// * `env`: A `dict` containing environment details. Variables are execution-phase dependent; they are available in subsequent modules during checkout and fully available during `spaces run`.
    ///     * `vars` (`dict`): Environment variables to set.
    ///     * `paths` (`list`): Paths to prepend to `PATH`.
    ///     * `system_paths` (`list`): Paths appended to the end of `PATH`.
    ///     * `inherited_vars` (`list`): Variables fixed from the calling environment at checkout.
    ///     * `secret_inherited_vars` (`list`): Variables inherited on demand with masked log values.
    fn update_env(
        #[starlark(require = named)] rule: starlark::values::Value,
        #[starlark(require = named)] env: starlark::values::Value,
    ) -> anyhow::Result<NoneType> {
        let rule: rule::Rule = serde_json::from_value(rule.to_json_value()?)
            .context(format_context!("bad options for update env rule"))?;

        // support JSON, yaml, and toml
        let environment: environment::Environment = serde_json::from_value(env.to_json_value()?)
            .context(format_context!("Failed to parse update_env arguments"))?;

        let update_env = executor::env::UpdateEnv { environment };

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::UpdateEnv(update_env),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;

        Ok(NoneType)
    }
}

fn add_git_url_to_workspace_store_queue(
    workspace_arc: WorkspaceArc,
    url: &str,
    cow: &str,
) -> anyhow::Result<()> {
    let mut workspace = workspace_arc.write();
    if let Ok((store_path, repo_name)) = git::BareRepository::url_to_relative_path_and_name(url) {
        let store_path = format!("{cow}{store_path}/{repo_name}");
        workspace
            .add_store_entry(store_path.into())
            .context(format_context!("while adding git url to store queue"))?;
    }
    Ok(())
}

fn add_http_url_to_workspace_store_queue(
    workspace_arc: WorkspaceArc,
    url: &str,
    filename: &Option<Arc<str>>,
) -> anyhow::Result<()> {
    let mut workspace = workspace_arc.write();
    if let Ok(relative_path) = http_archive::HttpArchive::url_to_relative_path(url, filename) {
        workspace
            .add_store_entry(relative_path.into())
            .context(format_context!(
                "while adding http url to workspace store queue"
            ))?;
    }
    Ok(())
}

fn add_http_archive(
    rule: rule::Rule,
    archive_option: Option<http_archive::Archive>,
) -> anyhow::Result<()> {
    if let Some(mut archive) = archive_option {
        //create a target that waits for all downloads
        //then create links based on all downloads being complete

        archive.sha256 = if archive.sha256.starts_with("http") {
            // download the sha256 file
            http_archive::download_string(&archive.sha256).context(format_context!(
                "Failed to download sha256 file {}",
                archive.sha256
            ))?
        } else {
            archive.sha256
        };

        // Validate headers
        if let Some(headers) = archive.headers.as_ref() {
            http_archive::validate_headers(headers)
                .context(format_context!("Failed to validate standard headers"))?;
        }

        let mut globs = archive.globs.unwrap_or_default();
        if let Some(includes) = archive.includes.as_ref() {
            for include in includes {
                globs.insert(format!("+{include}").into());
            }
        }

        if let Some(excludes) = archive.excludes.as_ref() {
            if globs.is_empty() {
                globs.insert("+**".into());
            }
            for exclude in excludes {
                globs.insert(format!("-{exclude}").into());
            }
        }

        if !globs.is_empty() {
            changes::glob::validate(&globs).context(format_context!("Failed to validate globs"))?;
            archive.globs = Some(globs);
        } else {
            archive.globs = None;
        }

        let workspace_arc =
            singleton::get_workspace().context(format_error!("No active workspace found"))?;

        add_http_url_to_workspace_store_queue(
            workspace_arc.clone(),
            archive.url.as_ref(),
            &archive.filename,
        )
        .context(format_context!(
            "Failed to add http url to workspace store queue"
        ))?;

        let workspace = workspace_arc.read();

        let http_archive = http_archive::HttpArchive::new(
            &workspace.get_store_path(),
            rule.name.as_ref(),
            &archive,
            format!("{}/sysroot/bin", workspace.get_spaces_tools_path()).as_str(),
        )
        .context(format_context!(
            "Failed to create http_archive {}",
            rule.name
        ))?;

        let rule_name = rule.name.clone();
        rules::insert_task(task::Task::new(
            rule,
            task::Phase::Checkout,
            executor::Task::HttpArchive(executor::http_archive::HttpArchive { http_archive }),
        ))
        .context(format_context!("Failed to insert task {rule_name}"))?;
    }
    Ok(())
}
