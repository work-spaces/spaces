use crate::{completions, evaluator, executor, rules, task, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use std::sync::Arc;
use utils::{
    ci, features, git, labels, lock, logger, logs, mtarget, rcache, shell, store, version, ws,
};

use crate::{lsp_context, singleton};
use itertools::Itertools;

pub use evaluator::IsExecuteTasks;

pub enum IsCreateLockFile {
    No,
    Yes,
}
impl From<IsCreateLockFile> for bool {
    fn from(value: IsCreateLockFile) -> bool {
        match value {
            IsCreateLockFile::No => false,
            IsCreateLockFile::Yes => true,
        }
    }
}

impl From<bool> for IsCreateLockFile {
    fn from(is_create_lock_file: bool) -> Self {
        match is_create_lock_file {
            false => IsCreateLockFile::No,
            true => IsCreateLockFile::Yes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForEachRepo {
    Repo,
    Branch,
    DirtyBranch,
    DevBranch,
}

#[derive(Debug, Clone)]
pub enum RunWorkspace {
    Target(Option<Arc<str>>, Vec<Arc<str>>),
    Script(Vec<(Arc<str>, Arc<str>)>),
}

fn get_workspace(
    console: console::Console,
    run_workspace: RunWorkspace,
    absolute_path_to_workspace: Option<Arc<str>>,
    is_clear_inputs: workspace::IsClearInputs,
    is_checkout_phase: workspace::IsCheckoutPhase,
    is_create_log_folder: workspace::IsCreateLogFolder,
) -> anyhow::Result<workspace::Workspace> {
    let checkout_scripts: Option<Vec<Arc<str>>> = match &run_workspace {
        RunWorkspace::Target(_, _) => None,
        RunWorkspace::Script(scripts) => Some(scripts.iter().map(|e| e.0.clone()).collect()),
    };

    workspace::Workspace::new(
        console.clone(),
        absolute_path_to_workspace,
        is_clear_inputs,
        checkout_scripts,
        is_checkout_phase,
        is_create_log_folder,
    )
    .context(format_context!("while running workspace"))
}

fn evaluate_environment(
    console: console::Console,
    workspace_arc: workspace::WorkspaceArc,
) -> anyhow::Result<()> {
    let workspace_modules = workspace_arc.read().modules.clone();
    let modules: Vec<_> = workspace_modules
        .iter()
        .filter_map(|(name, module)| {
            if name.as_ref() == workspace::ENV_FILE_NAME {
                Some((name.clone(), module.clone()))
            } else {
                None
            }
        })
        .collect();

    // evaluate the modules to bring in env.spaces.star
    evaluator::evaluate_starlark_modules(
        console.clone(),
        workspace_arc.clone(),
        modules.as_slice(),
        task::Phase::Inspect,
    )
    .context(format_context!("while evaluating starlark env module"))?;

    Ok(())
}

/// Check all repositories for cleanliness and perform pre-sync validations.
/// If stash is enabled, stashes dirty repos and returns their paths.
/// Returns a vector of repo paths that were stashed (empty if stash is disabled).
/// Returns an error if repos are dirty without --stash or if rebase operations would fail.
pub fn check_repos_before_sync(
    console: console::Console,
    top_progress: &mut console::Progress,
    workspace_arc: workspace::WorkspaceArc,
) -> anyhow::Result<Vec<Arc<str>>> {
    let workspace_members = workspace_arc.read().settings.json.members.clone();
    let dev_branch_rules = workspace_arc.read().settings.json.dev_branches.clone();
    let use_stash = singleton::get_sync_stash();

    let mut dev_branch_dirty = Vec::new();
    let mut branch_dirty = Vec::new();
    let mut detached_dirty = Vec::new();
    let mut stashed_repos = Vec::new();
    let mut rebase_conflicts = Vec::new();

    // First pass: check all repos for cleanliness
    for (url, member_list) in workspace_members.iter() {
        for member in member_list.iter() {
            top_progress.set_message(format!("checking {}", member.path).as_str());
            // Check if the member directory has a .git directory
            let member_git_path = std::path::Path::new(member.path.as_ref()).join(".git");
            if !member_git_path.exists() {
                continue;
            }

            let mut repo_progress = console::Progress::new(
                console.clone(),
                format!("//{}", member.path),
                None,
                Some(format!("//{}: checking repository status", member.path)),
            );

            let repo = git::Repository::new(url.clone(), member.path.clone());

            // Check if repo is dirty
            if repo.is_dirty(&mut repo_progress) {
                if use_stash {
                    // Stash the changes
                    if let Err(e) = repo.stash(&mut repo_progress) {
                        let lines = console::make_finalize_line(
                            console::FinalType::Failed,
                            None,
                            &format!("//{}: failed to stash changes: {e}", member.path),
                        );
                        repo_progress.set_finalize_lines(lines);

                        // Pop any stashes that were successfully created before this failure
                        if !stashed_repos.is_empty() {
                            let pop_result = pop_stashed_repos(
                                console.clone(),
                                workspace_arc.clone(),
                                stashed_repos.clone(),
                            );
                            if let Err(pop_err) = pop_result {
                                console.warning(
                                    "Failed to pop stashes",
                                    format!("Some stashes could not be popped: {pop_err}. You may need to manually run 'git stash pop'."),
                                )?;
                            }
                        }

                        return Err(format_error!(
                            "//{}: failed to stash changes: {e}",
                            member.path,
                        ));
                    }
                    stashed_repos.push(member.path.clone());
                    let lines = console::make_finalize_line(
                        console::FinalType::Completed,
                        None,
                        &format!("//{}: stashed uncommitted changes", member.path),
                    );
                    repo_progress.set_finalize_lines(lines);
                } else {
                    // Categorize dirty repos by their state
                    let is_dev_branch = dev_branch_rules.contains(&member.path);
                    let current_branch = repo.get_current_branch(&mut repo_progress).ok().flatten();

                    if is_dev_branch {
                        dev_branch_dirty.push(member.path.clone());
                    } else if current_branch.is_some() {
                        branch_dirty.push(member.path.clone());
                    } else {
                        detached_dirty.push(member.path.clone());
                    }

                    let lines = console::make_finalize_line(
                        console::FinalType::Failed,
                        None,
                        &format!("//{}: has uncommitted changes", member.path),
                    );
                    repo_progress.set_finalize_lines(lines);
                    continue;
                }
            }

            // If this is a dev branch repo, check if rebase would have conflicts
            if dev_branch_rules.contains(&member.path) {
                if let Ok(Some(_current_branch)) = repo.get_current_branch(&mut repo_progress) {
                    // Use the original rev from the member as the upstream branch
                    let upstream_branch = format!("origin/{}", member.rev);

                    // First fetch to ensure we have latest remote changes
                    if let Err(e) = repo.fetch_with_prune(&mut repo_progress) {
                        let lines = console::make_finalize_line(
                            console::FinalType::Failed,
                            None,
                            &format!("//{}: failed to fetch: {e}", member.path),
                        );
                        repo_progress.set_finalize_lines(lines);

                        // Pop any stashes before returning error
                        if !stashed_repos.is_empty() {
                            let pop_result = pop_stashed_repos(
                                console.clone(),
                                workspace_arc.clone(),
                                stashed_repos.clone(),
                            );
                            if let Err(pop_err) = pop_result {
                                console.warning(
                                    "Failed to pop stashes",
                                    format!("Some stashes could not be popped: {pop_err}. You may need to manually run 'git stash pop'."),
                                )?;
                            }
                        }

                        return Err(format_error!(
                            "//{}: failed to fetch updates: {e}",
                            member.path,
                        ));
                    }

                    // Check if rebase would have conflicts
                    match repo.can_rebase_without_conflicts(&mut repo_progress, &upstream_branch) {
                        Ok(true) => {
                            let lines = console::make_finalize_line(
                                console::FinalType::Completed,
                                repo_progress.elapsed(),
                                &format!("//{}: ready for rebase", member.path),
                            );
                            repo_progress.set_finalize_lines(lines);
                        }
                        Ok(false) => {
                            rebase_conflicts.push((member.path.clone(), member.rev.clone()));
                            let lines = console::make_finalize_line(
                                console::FinalType::Failed,
                                None,
                                &format!("//{}: rebase would have conflicts", member.path),
                            );
                            repo_progress.set_finalize_lines(lines);
                        }
                        Err(e) => {
                            let lines = console::make_finalize_line(
                                console::FinalType::Failed,
                                None,
                                &format!("//{} Failed to check conflicts: {}", member.path, e),
                            );
                            repo_progress.set_finalize_lines(lines);

                            // Pop any stashes before returning error
                            if !stashed_repos.is_empty() {
                                let pop_result = pop_stashed_repos(
                                    console.clone(),
                                    workspace_arc.clone(),
                                    stashed_repos.clone(),
                                );
                                if let Err(pop_err) = pop_result {
                                    console.warning(
                                        "Failed to pop stashes",
                                        format!("Some stashes could not be popped: {pop_err}. You may need to manually run 'git stash pop'."),
                                    )?;
                                }
                            }

                            return Err(format_error!(
                                "//{} Failed to check rebase conflicts: {e}",
                                member.path,
                            ));
                        }
                    }
                } else {
                    let lines = console::make_finalize_line(
                        console::FinalType::NotRequired,
                        repo_progress.elapsed(),
                        format!("//{}: not on a branch", member.path).as_str(),
                    );
                    repo_progress.set_finalize_lines(lines);
                }
            } else {
                let status_msg = if stashed_repos.contains(&member.path) {
                    format!("//{}: stashed changes", member.path)
                } else {
                    format!("//{}: clean git repo", member.path)
                };
                let lines = console::make_finalize_line(
                    console::FinalType::Completed,
                    repo_progress.elapsed(),
                    status_msg.as_str(),
                );
                repo_progress.set_finalize_lines(lines);
            }
        }
    }

    // If there are any dirty repos and stash is not enabled, report them all and fail
    let has_dirty_repos =
        !dev_branch_dirty.is_empty() || !branch_dirty.is_empty() || !detached_dirty.is_empty();
    if has_dirty_repos {
        singleton::set_evaluation_failure();

        // Emit styled error message to console
        console.emit_line(console::Line::default());

        let mut error_line = console::Line::default();
        error_line.push(console::Span::new_styled_lossy(
            console::style::StyledContent::new(
                console::keyword_style(),
                "Cannot sync:".to_string(),
            ),
        ));
        error_line.push(console::Span::new_unstyled_lossy(
            " the following repositories have uncommitted changes:".to_string(),
        ));
        console.emit_line(error_line);

        // Report dev branches
        if !dev_branch_dirty.is_empty() {
            for repo in &dev_branch_dirty {
                let mut repo_line = console::Line::default();
                repo_line.push(console::Span::new_unstyled_lossy("- ".to_string()));
                repo_line.push(console::Span::new_styled_lossy(
                    console::style::StyledContent::new(console::name_style(), format!("//{repo}")),
                ));
                repo_line.push(console::Span::new_unstyled_lossy(
                    ": [dev-branch] is dirty and not ready for rebase",
                ));
                console.emit_line(repo_line);
            }
        }

        // Report regular branches
        if !branch_dirty.is_empty() {
            for repo in &branch_dirty {
                let mut repo_line = console::Line::default();
                repo_line.push(console::Span::new_unstyled_lossy("- ".to_string()));
                repo_line.push(console::Span::new_styled_lossy(
                    console::style::StyledContent::new(console::name_style(), format!("//{repo}")),
                ));
                repo_line.push(console::Span::new_unstyled_lossy(
                    ": [branch] is dirty and not ready for pull",
                ));
                console.emit_line(repo_line);
            }
        }

        // Report detached HEAD
        if !detached_dirty.is_empty() {
            for repo in &detached_dirty {
                let mut repo_line = console::Line::default();
                repo_line.push(console::Span::new_unstyled_lossy("- ".to_string()));
                repo_line.push(console::Span::new_styled_lossy(
                    console::style::StyledContent::new(console::name_style(), format!("//{repo}")),
                ));
                repo_line.push(console::Span::new_unstyled_lossy(
                    ": [detached HEAD] is dirty and not ready for sync",
                ));
                console.emit_line(repo_line);
            }
        }

        console.emit_line(console::Line::default());
    }

    // If there are any repos with rebase conflicts, report them all and fail
    if !rebase_conflicts.is_empty() {
        singleton::set_evaluation_failure();

        // Pop stashes before returning error
        if !stashed_repos.is_empty() {
            let pop_result = pop_stashed_repos(
                console.clone(),
                workspace_arc.clone(),
                stashed_repos.clone(),
            );
            if let Err(e) = pop_result {
                console.warning(
                    "Failed to pop stashes",
                    format!("Some stashes could not be popped: {e}. You may need to manually run 'git stash pop'."),
                )?;
            }
        }

        // Emit styled error message to console
        console.emit_line(console::Line::default());

        let mut error_line = console::Line::default();
        error_line.push(console::Span::new_styled_lossy(
            console::style::StyledContent::new(
                console::keyword_style(),
                "Cannot sync:".to_string(),
            ),
        ));
        error_line.push(console::Span::new_unstyled_lossy(
            " the following dev-branch repositories would have rebase conflicts:".to_string(),
        ));
        console.emit_line(error_line);

        // Report each repository with rebase conflicts
        for (path, upstream_branch) in &rebase_conflicts {
            let mut repo_line = console::Line::default();
            repo_line.push(console::Span::new_unstyled_lossy("- ".to_string()));
            repo_line.push(console::Span::new_styled_lossy(
                console::style::StyledContent::new(console::name_style(), format!("//{}", path)),
            ));
            repo_line.push(console::Span::new_unstyled_lossy(format!(
                " (rebasing onto origin/{})",
                upstream_branch
            )));
            console.emit_line(repo_line);
        }

        console.emit_line(console::Line::default());
        let mut help_line = console::Line::default();
        help_line.push(console::Span::new_unstyled_lossy(
            "Please manually resolve conflicts by rebasing these repositories.".to_string(),
        ));
        console.emit_line(help_line);
        console.emit_line(console::Line::default());
    }

    if !rebase_conflicts.is_empty() || has_dirty_repos {
        if rebase_conflicts.is_empty() {
            console.emit_line(console::Line::default());
            let mut help_line = console::Line::default();
            help_line.push(console::Span::new_unstyled_lossy("Use ".to_string()));
            help_line.push(console::Span::new_styled_lossy(
                console::style::StyledContent::new(
                    console::name_style(),
                    "spaces sync --stash".to_string(),
                ),
            ));
            help_line.push(console::Span::new_unstyled_lossy(
                " to automatically stash/pop changes.".to_string(),
            ));
            console.emit_line(help_line);
        }

        return Err(format_error!(
            "Cannot sync: {} repositories need to be resolved manually",
            rebase_conflicts.len()
        ));
    }

    Ok(stashed_repos)
}

/// Perform rebase operations on dev-branch repositories.
pub fn rebase_dev_branches(
    console: console::Console,
    workspace_arc: workspace::WorkspaceArc,
) -> anyhow::Result<()> {
    let workspace_members = workspace_arc.read().settings.json.members.clone();
    let dev_branch_rules = workspace_arc.read().settings.json.dev_branches.clone();

    for (url, member_list) in workspace_members.iter() {
        for member in member_list.iter() {
            // Only process dev branch repos
            if !dev_branch_rules.contains(&member.path) {
                continue;
            }

            // Check if the member directory has a .git directory
            let member_git_path = std::path::Path::new(member.path.as_ref()).join(".git");
            if !member_git_path.exists() {
                continue;
            }

            let mut repo_progress = console::Progress::new(
                console.clone(),
                format!("//{}", member.path),
                None,
                Some("Rebasing dev branch".to_string()),
            );

            let repo = git::Repository::new(url.clone(), member.path.clone());

            // Get current branch to verify we're on a branch
            if let Ok(Some(_current_branch)) = repo.get_current_branch(&mut repo_progress) {
                // Use the original rev from the member as the upstream branch
                let upstream_branch = format!("origin/{}", member.rev);

                // Fetch with prune
                repo.fetch_with_prune(&mut repo_progress)
                    .context(format_context!(
                        "Failed to fetch updates for {}",
                        member.path
                    ))?;

                // Check if upstream branch exists after fetch
                let check_branch = git::execute_git_command(
                    &mut repo_progress,
                    url,
                    console::ExecuteOptions {
                        working_directory: Some(member.path.clone()),
                        arguments: vec![
                            "rev-parse".into(),
                            "--verify".into(),
                            upstream_branch.clone().into(),
                        ],
                        ..Default::default()
                    },
                );

                if check_branch.is_err() {
                    // Upstream branch doesn't exist - skip rebase
                    let lines = console::make_finalize_line(
                        console::FinalType::NotRequired,
                        repo_progress.elapsed(),
                        &format!("//{} Remote branch not found, skipping rebase", member.path),
                    );
                    repo_progress.set_finalize_lines(lines);
                    continue;
                }

                // Perform rebase
                match repo.rebase_onto(&mut repo_progress, &upstream_branch) {
                    Ok(_) => {
                        let lines = console::make_finalize_line(
                            console::FinalType::Completed,
                            repo_progress.elapsed(),
                            &format!(
                                "//{} rebased successfully on {}",
                                member.path, upstream_branch
                            ),
                        );
                        repo_progress.set_finalize_lines(lines);
                    }
                    Err(e) => {
                        let lines = console::make_finalize_line(
                            console::FinalType::Failed,
                            repo_progress.elapsed(),
                            &format!("//{} Rebase failed: {}", member.path, e),
                        );
                        repo_progress.set_finalize_lines(lines);
                        return Err(format_error!(
                            "//{} Failed to rebase onto {}: {e}",
                            member.path,
                            upstream_branch,
                        ));
                    }
                }
            } else {
                let lines = console::make_finalize_line(
                    console::FinalType::NotRequired,
                    repo_progress.elapsed(),
                    &format!("//{} Not on a branch, skipping rebase", member.path),
                );
                repo_progress.set_finalize_lines(lines);
            }
        }
    }

    Ok(())
}

/// Pop stashes on repositories that were stashed during pre-sync checks.
pub fn pop_stashed_repos(
    console: console::Console,
    workspace_arc: workspace::WorkspaceArc,
    stashed_repos: Vec<Arc<str>>,
) -> anyhow::Result<()> {
    if stashed_repos.is_empty() {
        return Ok(());
    }

    let workspace_members = workspace_arc.read().settings.json.members.clone();

    for (url, member_list) in workspace_members.iter() {
        for member in member_list.iter() {
            // Only process repos that were stashed
            if !stashed_repos.contains(&member.path) {
                continue;
            }

            let mut repo_progress = console::Progress::new(
                console.clone(),
                format!("//{}", member.path),
                None,
                Some(format!("//{}: popping stash", member.path)),
            );

            let repo = git::Repository::new(url.clone(), member.path.clone());

            match repo.stash_pop(&mut repo_progress) {
                Ok(_) => {
                    let lines = console::make_finalize_line(
                        console::FinalType::Completed,
                        repo_progress.elapsed(),
                        &format!("//{}: popped stash successfully", member.path),
                    );
                    repo_progress.set_finalize_lines(lines);
                }
                Err(e) => {
                    let lines = console::make_finalize_line(
                        console::FinalType::Failed,
                        None,
                        &format!("[{}] Failed to pop stash: {e}", member.path),
                    );
                    repo_progress.set_finalize_lines(lines);
                    // Don't fail the entire operation, just warn
                    console.warning(
                        "Failed to pop stash",
                        format!(
                            "//{}: {e}. You may need to manually run 'git stash pop' in this repository.",
                            member.path
                        ),
                    )?;
                }
            }
        }
    }

    Ok(())
}

fn emit_foreach_separator(console: &console::Console) {
    console.emit_line(console::Line::default());
}

fn emit_foreach_header(
    console: &console::Console,
    index: usize,
    total: usize,
    working_directory: &str,
    command_string: &str,
) {
    let badge_style = console::style::ContentStyle {
        foreground_color: Some(console::style::Color::DarkCyan),
        background_color: None,
        underline_color: None,
        attributes: console::style::Attributes::from(console::style::Attribute::Bold),
    };
    let repo_style = console::style::ContentStyle {
        foreground_color: Some(console::style::Color::Cyan),
        background_color: None,
        underline_color: None,
        attributes: console::style::Attributes::from(console::style::Attribute::Bold),
    };
    let command_style = console::style::ContentStyle {
        foreground_color: Some(console::style::Color::DarkGrey),
        background_color: None,
        underline_color: None,
        attributes: console::style::Attributes::default(),
    };

    let mut line = console::Line::default();
    line.push(console::Span::new_styled_lossy(
        console::style::StyledContent::new(badge_style, format!("[{index}/{total}] ")),
    ));
    line.push(console::Span::new_styled_lossy(
        console::style::StyledContent::new(repo_style, working_directory.to_string()),
    ));
    line.push(console::Span::new_unstyled_lossy(" $ "));
    line.push(console::Span::new_styled_lossy(
        console::style::StyledContent::new(command_style, command_string.to_string()),
    ));
    console.emit_line(line);
}

pub fn foreach_repo(
    console: console::Console,
    run_workspace: RunWorkspace,
    for_each_repo: ForEachRepo,
    command_arguments: &[Arc<str>],
) -> anyhow::Result<()> {
    let workspace = get_workspace(
        console.clone(),
        run_workspace,
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
        workspace::IsCreateLogFolder::Yes,
    )
    .context(format_context!("while getting workspace"))?;

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));
    evaluate_environment(console.clone(), workspace_arc.clone())
        .context(format_context!("while evaluating starlark env module"))?;

    let workspace_members = workspace_arc.read().settings.json.members.clone();
    let dev_branch_rules = workspace_arc.read().settings.json.dev_branches.clone();

    let mut repos = Vec::new();

    let is_run_on_branches_only = matches!(
        for_each_repo,
        ForEachRepo::Branch | ForEachRepo::DirtyBranch
    );
    let is_run_on_dirty_branches = matches!(for_each_repo, ForEachRepo::DirtyBranch);

    for (url, member_list) in workspace_members.iter() {
        for member in member_list.iter() {
            if is_run_on_branches_only {
                let mut repo_progress = console::Progress::new(
                    console.clone(),
                    format!("//{}", member.path),
                    Some(100),
                    Some("Queueing for execution".to_string()),
                );
                // use git to check if member is on a branch
                let repo = git::Repository::new(url.clone(), member.path.clone());
                if repo.is_branch(&mut repo_progress, &member.rev) {
                    if repo.is_currently_on_a_branch(&mut repo_progress) {
                        if is_run_on_dirty_branches {
                            // check if the branch is dirty
                            if repo.is_dirty(&mut repo_progress) {
                                repos.push(member.clone());
                            } else {
                                repo_progress.set_finalize("Skipping: branch is clean");
                            }
                        } else {
                            repos.push(member.clone());
                        }
                    } else {
                        repo_progress.set_finalize("Skipping: not currently on a branch");
                    }
                } else {
                    repo_progress.set_finalize("Skipping: rev is not a branch");
                }
            } else if for_each_repo == ForEachRepo::DevBranch {
                // get the dev-branches from the workspace settings
                for dev_branch_rule in dev_branch_rules.iter() {
                    let location_in_workspace =
                        labels::get_rule_name_from_label(dev_branch_rule.as_ref());
                    if location_in_workspace == member.path.as_ref() {
                        repos.push(member.clone());
                        // skip the rest of the loop
                        break;
                    }
                }
            } else {
                // check if member.path is an existing directory
                let path = std::path::Path::new(member.path.as_ref());
                if path.exists() && path.is_dir() {
                    repos.push(member.clone());
                }
            }
        }
    }

    let command_string = command_arguments.join(" ");
    let command_label = command_arguments.join("_");
    let total_repos = repos.len();
    for (index, member) in repos.iter().enumerate() {
        let mut args = command_arguments.to_owned().split_off(1);
        let command = command_arguments[0].clone();
        if command.as_ref() == "git" {
            args.insert(0, "-c".into());
            args.insert(1, "color.ui=always".into());
        }
        let working_directory: Arc<str> = format!("//{}", member.path).into();

        let mut exec_progress = console::Progress::new(
            console.clone(),
            command.as_ref(),
            Some(100),
            Some("Complete".to_string()),
        );

        if index > 0 {
            exec_progress.console.emit_line(console::Line::default());
        }
        emit_foreach_header(
            &exec_progress.console,
            index + 1,
            total_repos,
            working_directory.as_ref(),
            command_string.as_ref(),
        );
        emit_foreach_separator(&exec_progress.console);

        let name = format!("__foreach_{working_directory}_{command_label}");

        let exec = executor::exec::Exec {
            command: command.clone(),
            args: Some(args),
            env: None,
            working_directory: Some(working_directory.clone()),
            redirect_stdout: None,
            expect: None,
            log_level: Some(console::Level::Passthrough),
            timeout: None,
        };

        exec.execute(
            &mut exec_progress,
            workspace_arc.clone(),
            name.as_str(),
            executor::exec::UseWorkspaceEnv::Yes,
        )
        .context(format_context!(
            "while executing command {} in workspace",
            command
        ))?;
    }

    Ok(())
}

pub fn run_shell_in_workspace(
    console: console::Console,
    path: Option<Arc<str>>,
    completions_command: Option<(clap::Command, rules::HasHelp)>,
) -> anyhow::Result<()> {
    let workspace = get_workspace(
        console.clone(),
        RunWorkspace::Target(None, vec![]),
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
        workspace::IsCreateLogFolder::No,
    )
    .context(format_context!("while getting workspace"))?;

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));
    let shell_config_path = std::path::Path::new(workspace::SHELL_TOML_NAME);
    let shell_config_path_option = if shell_config_path.exists() {
        Some(workspace::SHELL_TOML_NAME.into())
    } else {
        None
    };

    let shell_config = shell::Config::load(shell_config_path_option, path)
        .context(format_context!("while loading shell config"))?;

    let shell_md = shell_config.to_markdown();
    std::fs::write(workspace::SHELL_MD_FILE_NAME, shell_md).context(format_context!(
        "while writing {}",
        workspace::SHELL_MD_FILE_NAME
    ))?;

    let completion_content = if let Some((command, has_help)) = completions_command {
        // rules are now available
        let clap_shell = shell_config
            .get_shell()
            .context(format_context!("Shell does not support completions"))?;

        let run_targets = run_starlark_with_workspace_get_targets(
            console.clone(),
            workspace_arc.clone(),
            has_help,
        )
        .context(format_context!("Failed to get targets"))?;

        completions::generate_workspace_completions(&command, clap_shell, run_targets)
            .context(format_context!("Failed to generate workspace completions"))?
    } else {
        evaluate_environment(console.clone(), workspace_arc.clone())
            .context(format_context!("while evaluating starlark env module"))?;
        Vec::new()
    };

    let run_environment = workspace_arc
        .read()
        .get_env_vars()
        .context(format_context!("while getting env vars"))?;

    let absolute_path = workspace_arc.read().absolute_path.clone();
    let absolute_workspace_path = std::path::Path::new(absolute_path.as_ref());
    let relative_directory = workspace_arc.read().relative_invoked_path.clone();
    let working_directory = absolute_workspace_path.join(relative_directory.as_ref());

    const RELATIVE_SHELL_DIR: &str = ".spaces/shell";
    let startup_dir = absolute_workspace_path.join(RELATIVE_SHELL_DIR);

    std::fs::create_dir_all(&startup_dir)
        .context(format_context!("while creating shell startup dir"))?;

    console.shutdown_refresh_thread();

    while !console.is_refresh_thread_ready_to_join() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Run the interactive shell. This blocks until the user exits.
    let pristine_shell = if workspace_arc
        .read()
        .features
        .is_enabled(features::Feature::AllowShellConfig)
    {
        shell::IsPristineShell::No
    } else {
        shell::IsPristineShell::Yes
    };

    // Finalize the console output to flush all pending output before starting the shell
    console.finalize();

    shell::run(
        &shell_config,
        &run_environment,
        &startup_dir,
        completion_content,
        &working_directory,
        pristine_shell,
    )
    .context(format_context!("while running shell"))?;

    Ok(())
}

pub fn run_store_command_in_workspace(
    console: console::Console,
    store_command: store::StoreCommand,
) -> anyhow::Result<()> {
    let workspace_result = get_workspace(
        console.clone(),
        RunWorkspace::Target(None, vec![]),
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
        workspace::IsCreateLogFolder::No,
    );
    let store_path_str = match workspace_result {
        Ok(workspace) => workspace.get_store_path(),
        Err(_) => ws::get_checkout_store_path(),
    };
    let store_path = std::path::Path::new(store_path_str.as_ref());
    let mut store = store::Store::new_from_store_path(store_path).context(format_context!(
        "Failed to load store at {}",
        store_path_str
    ))?;
    let is_ci: ci::IsCi = singleton::get_is_ci().into();

    match store_command {
        store::StoreCommand::Info { sort_by, format } => {
            let rcache_path = ws::get_rcache_path(store_path);
            store
                .show_info(console.clone(), sort_by, format, is_ci, &rcache_path)
                .context(format_context!("While getting store info"))?;
            store
                .save(store_path)
                .context(format_context!("Failed to save store at {store_path_str}"))?;
        }
        store::StoreCommand::Fix { dry_run, git_fsck } => {
            store
                .fix(console.clone(), dry_run, is_ci, git_fsck)
                .context(format_context!("While fixing store"))?;

            store
                .save(store_path)
                .context(format_context!("Failed to save store at {store_path_str}",))?;
        }
        store::StoreCommand::Prune {
            age,
            dry_run,
            rcache_only,
        } => {
            if !rcache_only {
                store
                    .prune(console.clone(), age, dry_run, is_ci)
                    .context(format_context!("While pruning store"))?;

                store
                    .save(store_path)
                    .context(format_context!("Failed to save store at {store_path_str}",))?;
            }

            let rcache_path = ws::get_rcache_path(store_path);
            rcache::prune(&rcache_path, age, dry_run, console.clone(), is_ci)
                .context(format_context!("While pruning rcache"))?;
        }
    }

    Ok(())
}

pub fn run_features_command_in_workspace(
    console: console::Console,
    features_command: features::FeaturesCommand,
) -> anyhow::Result<()> {
    let workspace_result = get_workspace(
        console.clone(),
        RunWorkspace::Target(None, vec![]),
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
        workspace::IsCreateLogFolder::No,
    );
    let store_path_str = match workspace_result {
        Ok(workspace) => workspace.get_store_path(),
        Err(_) => ws::get_checkout_store_path(),
    };
    let store_path = std::path::Path::new(store_path_str.as_ref());

    features_command
        .execute(&console, store_path)
        .context(format_context!("Failed to execute features command"))?;

    Ok(())
}

pub fn run_logs_command_in_workspace(
    console: console::Console,
    logs_command: logs::LogsCommand,
) -> anyhow::Result<()> {
    let workspace = get_workspace(
        console.clone(),
        RunWorkspace::Target(None, vec![]),
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
        workspace::IsCreateLogFolder::No,
    )
    .context(format_context!(
        "Logs command must be run from within a workspace"
    ))?;

    let workspace_path = std::path::Path::new(workspace.absolute_path.as_ref());
    logs::execute(console, workspace_path, logs_command)
        .context(format_context!("Failed to run logs command"))
}

pub fn run_version_command_in_workspace(
    console: console::Console,
    command: version::Command,
) -> anyhow::Result<()> {
    let workspace_result = get_workspace(
        console.clone(),
        RunWorkspace::Target(None, vec![]),
        None,
        workspace::IsClearInputs::No,
        workspace::IsCheckoutPhase::No,
        workspace::IsCreateLogFolder::No,
    );
    let store_path_str = match workspace_result {
        Ok(workspace) => workspace.get_store_path(),
        Err(_) => ws::get_checkout_store_path(),
    };
    let store_path = std::path::Path::new(store_path_str.as_ref());
    let version_manager = version::Manager::new(store_path);

    match command {
        version::Command::List {} => {
            version_manager
                .list(console)
                .context(format_context!("Failed to list versions"))?;
        }
        version::Command::Fetch { tag, prerelease } => {
            let include_prerelease = if prerelease {
                version::IncludePrerelease::Yes
            } else {
                version::IncludePrerelease::No
            };
            version_manager
                .fetch(console, tag.clone(), include_prerelease)
                .context(format_context!(
                    "Failed to fetch {}",
                    tag.as_ref().map(|e| e.as_ref()).unwrap_or("latest")
                ))?;
        }
        version::Command::SetConfig { path } => {
            version_manager
                .set_config(console, path.clone())
                .context(format_context!(
                    "Failed to set version config from {}",
                    path
                ))?;
        }
        version::Command::UnsetConfig {} => {
            version_manager
                .unset_config(console)
                .context(format_context!("Failed to unset version config"))?;
        }
        version::Command::ShowConfig {} => {
            version_manager
                .show_config(console)
                .context(format_context!("Failed to show version config"))?;
        }
    }

    Ok(())
}

fn run_starlark_with_workspace_get_targets(
    console: console::Console,
    workspace: workspace::WorkspaceArc,
    has_help: rules::HasHelp,
) -> anyhow::Result<Vec<Arc<str>>> {
    run_starlark_modules_with_workspace(
        console,
        workspace,
        task::Phase::Inspect,
        RunWorkspace::Target(None, vec![]),
        IsCreateLockFile::No,
        IsExecuteTasks::No,
    )
    .context(format_context!("while executing run rules"))?;

    rules::get_run_targets(has_help).context(format_context!("Failed to get run targets"))
}

pub fn run_starlark_get_targets(
    console: console::Console,
    has_help: rules::HasHelp,
) -> anyhow::Result<Vec<Arc<str>>> {
    run_starlark_modules_in_workspace(
        console,
        task::Phase::Inspect,
        None,
        workspace::IsClearInputs::No,
        RunWorkspace::Target(None, vec![]),
        IsCreateLockFile::No,
        IsExecuteTasks::No,
    )
    .context(format_context!("while executing run rules"))?;

    rules::get_run_targets(has_help).context(format_context!("Failed to get run targets"))
}

fn run_starlark_modules_with_workspace(
    console: console::Console,
    workspace: workspace::WorkspaceArc,
    phase: task::Phase,
    run_workspace: RunWorkspace,
    is_create_lock_file: IsCreateLockFile,
    is_execute_tasks: IsExecuteTasks,
) -> anyhow::Result<()> {
    match run_workspace {
        RunWorkspace::Target(target, trailing_args) => {
            workspace.write().trailing_args = trailing_args;
            let target = target.map(|e| workspace.read().transform_target_path(e));
            workspace.write().target = target.clone();
            let modules = workspace.read().modules.clone();
            evaluator::run_starlark_modules(
                console.clone(),
                workspace.clone(),
                modules,
                phase,
                target,
                is_execute_tasks,
            )?
        }
        RunWorkspace::Script(scripts) => {
            for (name, _) in scripts.iter() {
                logger::Logger::new(console.clone(), name.clone()).debug("Digesting");
            }

            workspace.write().is_create_lock_file = is_create_lock_file.into();

            let eval_result = evaluator::run_starlark_modules(
                console.clone(),
                workspace.clone(),
                scripts,
                phase,
                None,
                is_execute_tasks,
            )
            .context(format_context!("while evaluating starlark modules"));

            if eval_result.is_err() && phase == task::Phase::Checkout {
                // Save settings so .spaces/settings.spaces.json is written with the
                // "order" and "dev-branches" entries when --keep-workspace-on-failure
                // is used. If the workspace is being cleaned up, the file will be
                // removed as part of that cleanup anyway.
                let _ = workspace.read().settings.save_json();
            }

            eval_result?;

            workspace
                .read()
                .save_lock_file()
                .context(format_context!("Failed to save workspace lock file"))?;
        }
    }

    workspace::RuleMetricsFile::update(workspace.clone())
        .context(format_context!("Failed to update rule metrics file"))?;

    Ok(())
}

pub fn run_starlark_modules_in_workspace(
    console: console::Console,
    phase: task::Phase,
    absolute_path_to_workspace: Option<Arc<str>>,
    is_clear_inputs: workspace::IsClearInputs,
    run_workspace: RunWorkspace,
    is_create_lock_file: IsCreateLockFile,
    is_execute_tasks: IsExecuteTasks,
) -> anyhow::Result<()> {
    let is_checkout_phase = if phase == task::Phase::Checkout {
        workspace::IsCheckoutPhase::Yes
    } else {
        workspace::IsCheckoutPhase::No
    };
    let is_create_log_folder = if phase == task::Phase::Checkout || phase == task::Phase::Run {
        workspace::IsCreateLogFolder::Yes
    } else {
        workspace::IsCreateLogFolder::No
    };
    let workspace = get_workspace(
        console.clone(),
        run_workspace.clone(),
        absolute_path_to_workspace,
        is_clear_inputs,
        is_checkout_phase,
        is_create_log_folder,
    )
    .context(format_context!("while getting workspace"))?;

    let workspace_lock = if phase == task::Phase::Checkout {
        let lock_file_name = format!("checkout.{}", lock::LOCK_FILE_SUFFIX);
        let mut workspace_lock = lock::FileLock::new(
            std::path::Path::new(".spaces")
                .join("locks")
                .join(&lock_file_name)
                .into(),
        );

        let try_lock_result = workspace_lock.try_lock().context(format_context!(
            "Failed to create the checkout workspace lock"
        ))?;
        if lock::LockStatus::Busy == try_lock_result {
            return Err(format_error!(
                "Cannot checkout/sync on this workspace. Another sync is in progress."
            ));
        }

        mtarget::ModuleDeps::clear_deps_dir().context(format_context!(
            "while clearing module deps directory before checkout/sync"
        ))?;

        Some(workspace_lock)
    } else {
        None
    };

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));

    // If this is a sync operation, check all repos before proceeding
    // We check repos based on the existing workspace settings (from previous checkout/sync)
    let stashed_repos = if phase == task::Phase::Checkout
        && singleton::get_is_sync()
        && !singleton::get_sync_force()
    {
        let mut pre_sync_progress = console::Progress::new(
            console.clone(),
            "pre-sync => ",
            None,
            Some("Complete".to_string()),
        );
        // Check all repos for cleanliness and potential rebase conflicts
        // This uses the existing members from the workspace settings
        let stashed = check_repos_before_sync(
            console.clone(),
            &mut pre_sync_progress,
            workspace_arc.clone(),
        )
        .context(format_context!("while checking repositories before sync"))?;

        pre_sync_progress.set_message("rebasing branches");
        // Perform rebase operations on dev-branch repos
        rebase_dev_branches(console.clone(), workspace_arc.clone())
            .context(format_context!("while rebasing dev branches"))?;

        pre_sync_progress.set_finalize_lines(console::make_finalize_line(
            console::FinalType::Completed,
            pre_sync_progress.elapsed(),
            "pre-sync repo check",
        ));

        stashed
    } else {
        Vec::new()
    };

    run_starlark_modules_with_workspace(
        console.clone(),
        workspace_arc.clone(),
        phase,
        run_workspace,
        is_create_lock_file,
        is_execute_tasks,
    )?;

    // Pop stashes after sync is complete
    if phase == task::Phase::Checkout && singleton::get_is_sync() && !stashed_repos.is_empty() {
        let mut stash_pop_progress = console::Progress::new(
            console.clone(),
            "pop stashes after sync",
            None,
            Some("Complete".to_string()),
        );
        pop_stashed_repos(console, workspace_arc, stashed_repos)
            .context(format_context!("while popping stashes after sync"))?;
        stash_pop_progress.set_finalize_lines(console::make_finalize_line(
            console::FinalType::Finished,
            stash_pop_progress.elapsed(),
            "stashes popped successfully",
        ));
    }

    drop(workspace_lock);

    Ok(())
}

pub fn run_lsp(console: console::Console) -> anyhow::Result<()> {
    let workspace = {
        workspace::Workspace::new(
            console.clone(),
            None,
            workspace::IsClearInputs::No,
            None,
            workspace::IsCheckoutPhase::No,
            workspace::IsCreateLogFolder::No,
        )
        .context(format_context!("while running workspace"))?
    };

    let workspace_arc = workspace::WorkspaceArc::new(lock::StateLock::new(workspace));

    use starlark_lsp::server;
    eprintln!("Starting Spaces Starlark server-");

    // collect .star files in workspace
    let workspace_path = workspace_arc.read().absolute_path.to_owned();
    let mut modules = Vec::new();
    let walkdir = walkdir::WalkDir::new(workspace_path.as_ref());
    for entry in walkdir {
        let entry = entry.context(format_context!("Failed to walk directory"))?;
        if entry.file_type().is_file() {
            let path = entry.path();
            if let Some(ext) = path.extension()
                && ext == "star"
                && !path
                    .components()
                    .contains(&std::path::Component::Normal("script".as_ref()))
            {
                modules.push(path.to_path_buf());
            }
        }
    }

    let lsp_context = lsp_context::SpacesContext::new(workspace_arc, lsp_context::ContextMode::Run)
        .context(format_context!(
            "Internal Error: Failed to create spaces lsp context"
        ))?;

    // Note that  we must have our logging only write out to stderr.

    server::stdio_server(lsp_context).context(format_context!("spaces LSP server exited"))?;
    eprintln!("Stopping Spaces Starlark server");

    Ok(())
}

pub fn checkout(
    console: console::Console,
    name: Arc<str>,
    script: Vec<Arc<str>>,
    checkout_repo_script: Option<Arc<str>>,
    create_lock_file: IsCreateLockFile,
    keep_workspace_on_failure: bool,
) -> anyhow::Result<()> {
    #[derive(Debug, Clone, PartialEq)]
    enum CheckoutCleanup {
        Workspace,
        WorkspaceContents,
    }

    // Checkout will fail if the target dir exists and is not empty
    let checkout_cleanup = if std::path::Path::new(name.as_ref()).exists() {
        let dir = std::fs::read_dir(name.as_ref())
            .context(format_context!("while reading directory {name}"))?;
        if dir.count() > 0 {
            return Err(format_error!(
                "checkout directory must be non-existent or empty"
            ));
        }

        // on cleanup, delete only the contents of the directory
        CheckoutCleanup::WorkspaceContents
    } else {
        // on cleanup, delete the entire directory since it doesn't exist yet
        CheckoutCleanup::Workspace
    };

    std::fs::create_dir_all(name.as_ref())
        .context(format_context!("while creating workspace directory {name}"))?;

    let mut scripts = Vec::new();

    for one_script in script {
        let script_path = if workspace::is_rules_module(&one_script) {
            one_script.clone()
        } else {
            format!("{one_script}.{}", workspace::SPACES_MODULE_NAME).into()
        };

        let script_as_path = std::path::Path::new(script_path.as_ref());
        let file_name: Arc<str> = script_as_path.file_name().unwrap().to_string_lossy().into();

        let one_script_contents = std::fs::read_to_string(script_path.as_ref())
            .context(format_context!("while reading script file {script_path}"))?;

        std::fs::write(format!("{name}/{file_name}"), one_script_contents.as_str()).context(
            format_context!("while writing script file {script_path} to workspace"),
        )?;

        scripts.push((file_name, one_script_contents.into()));
    }

    if let Some(checkout_repo_script) = checkout_repo_script {
        scripts.push((
            workspace::CHECKOUT_FILE_NAME.into(),
            checkout_repo_script.clone(),
        ));
        std::fs::write(
            format!("{name}/{}", workspace::CHECKOUT_FILE_NAME),
            checkout_repo_script.as_ref(),
        )
        .context(format_context!(
            "while writing script file {} to workspace",
            workspace::CHECKOUT_FILE_NAME
        ))?;
    }

    // ENV file is empty at the beginning of checkout
    let env_path = std::path::Path::new(name.as_ref()).join(workspace::ENV_FILE_NAME);
    std::fs::write(env_path, "").context(format_context!(
        "while creating {} file",
        workspace::ENV_FILE_NAME
    ))?;

    let current_working_directory = std::env::current_dir()
        .context(format_context!("Failed to get current working directory"))?;

    let target_workspace_directory = current_working_directory.join(name.as_ref());
    let absolute_path_to_workspace: Arc<str> = target_workspace_directory.to_string_lossy().into();

    let checkout_result = run_starlark_modules_in_workspace(
        console.clone(),
        task::Phase::Checkout,
        Some(absolute_path_to_workspace.clone()),
        workspace::IsClearInputs::No,
        RunWorkspace::Script(scripts),
        create_lock_file,
        IsExecuteTasks::Yes,
    )
    .context(format_context!(
        "while evaluating starlark modules for checkout"
    ));

    if !keep_workspace_on_failure && checkout_result.is_err() {
        {
            if checkout_cleanup == CheckoutCleanup::WorkspaceContents {
                console.log(
                    console::Level::Debug,
                    format!(
                        "Removing contents from existing workspace {absolute_path_to_workspace}",
                    )
                    .as_str(),
                )?;
                let contents = std::fs::read_dir(absolute_path_to_workspace.as_ref()).context(
                    format_context!(
                        "while reading workspace contents for failed workspace {absolute_path_to_workspace}"
                    ),
                )?;

                for entry in contents {
                    let entry = entry.context(format_context!(
                        "while reading workspace entry in failed workspace {absolute_path_to_workspace}"
                    ))?;
                    let path = entry.path();
                    if path.is_file() {
                        std::fs::remove_file(path).context(format_context!(
                            "while removing file in failed workspace {absolute_path_to_workspace}"
                        ))?;
                    } else if path.is_dir() {
                        std::fs::remove_dir_all(path)
                            .context(format_context!("while removing directory in failed workspace {absolute_path_to_workspace}"))?;
                    }
                }
            } else {
                console.log(
                    console::Level::Debug,
                    format!("Cleaning up workspace {absolute_path_to_workspace}").as_str(),
                )?;

                std::fs::remove_dir_all(absolute_path_to_workspace.as_ref()).context(
                    format_context!(
                        "while cleaning up failed workspace {absolute_path_to_workspace}"
                    ),
                )?;
            }
        }
    }

    checkout_result
}
