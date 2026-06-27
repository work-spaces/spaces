use crate::{singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use itertools::Itertools;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use utils::git;

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
enum DevBranchAction {
    Rebase,
    Merge,
    Skip,
}

#[derive(Debug, Clone)]
pub struct RepoSyncPlan {
    path: Arc<str>,
    url: Arc<str>,
    is_dev_branch: bool,
    is_rev_branch: bool,
    is_on_branch: bool,
    is_dirty: bool,
    pull_from: Option<Arc<str>>,
    rebase_from: Option<Arc<str>>,
    merge_from: Option<Arc<str>>,
    will_stash: bool,
    skip_reason: Option<Arc<str>>,
}

#[derive(Debug, Clone)]
struct ParsedDevBranchBase {
    repo_path: Arc<str>,
    base_ref: Arc<str>,
}

fn normalize_repo_selector(selector: &str) -> Arc<str> {
    selector.strip_prefix("//").unwrap_or(selector).into()
}

fn is_member_dev_branch(member_path: &str, dev_branch_rules: &[Arc<str>]) -> bool {
    if dev_branch_rules
        .iter()
        .any(|item| item.as_ref() == member_path)
    {
        return true;
    }

    for item in dev_branch_rules {
        if member_path.ends_with(item.as_ref()) {
            return true;
        }
    }

    false
}

fn resolve_selector_set(
    flag_name: &str,
    selectors: &[Arc<str>],
    member_paths: &HashSet<Arc<str>>,
) -> anyhow::Result<HashSet<Arc<str>>> {
    let mut resolved = HashSet::new();
    let mut unknown = Vec::new();

    for selector in selectors {
        let normalized = normalize_repo_selector(selector.as_ref());
        if member_paths.contains(&normalized) {
            resolved.insert(normalized);
        } else {
            unknown.push(selector.clone());
        }
    }

    if !unknown.is_empty() {
        unknown.sort();
        return Err(format_error!(
            "Unknown repo selectors for {flag_name}: {}",
            unknown.iter().map(|item| item.as_ref()).join(", ")
        ));
    }

    Ok(resolved)
}

fn parse_dev_branch_base_entries(
    raw_entries: &[Arc<str>],
) -> anyhow::Result<Vec<ParsedDevBranchBase>> {
    let mut parsed_entries = Vec::new();
    let mut seen_repos: HashSet<Arc<str>> = HashSet::new();

    for entry in raw_entries {
        let Some((repo_selector, base_ref)) = entry.split_once('=') else {
            return Err(format_error!(
                "Bad --dev-branch-base argument `{entry}`: expected <repo-path>=<ref>"
            ));
        };

        if repo_selector.is_empty() || base_ref.is_empty() {
            return Err(format_error!(
                "Bad --dev-branch-base argument `{entry}`: expected <repo-path>=<ref>"
            ));
        }

        let repo_path = normalize_repo_selector(repo_selector);
        if !seen_repos.insert(repo_path.clone()) {
            return Err(format_error!(
                "Duplicate --dev-branch-base for repo `//{repo_path}`"
            ));
        }

        parsed_entries.push(ParsedDevBranchBase {
            repo_path,
            base_ref: base_ref.into(),
        });
    }

    Ok(parsed_entries)
}

fn resolve_dev_branch_base_map(
    parsed_entries: &[ParsedDevBranchBase],
    member_paths: &HashSet<Arc<str>>,
    dev_branch_paths: &HashSet<Arc<str>>,
) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
    let mut unknown = Vec::new();
    let mut non_dev_branch = Vec::new();
    let mut map = HashMap::new();

    for entry in parsed_entries {
        if !member_paths.contains(&entry.repo_path) {
            unknown.push(entry.repo_path.clone());
            continue;
        }

        if !dev_branch_paths.contains(&entry.repo_path) {
            non_dev_branch.push(entry.repo_path.clone());
            continue;
        }

        map.insert(entry.repo_path.clone(), entry.base_ref.clone());
    }

    if !unknown.is_empty() {
        unknown.sort();
        return Err(format_error!(
            "Unknown repo selectors for --dev-branch-base: {}",
            unknown
                .iter()
                .map(|item| format!("//{item}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if !non_dev_branch.is_empty() {
        non_dev_branch.sort();
        return Err(format_error!(
            "`--dev-branch-base` is only valid for dev-branch repos. Not dev-branch: {}",
            non_dev_branch
                .iter()
                .map(|item| format!("//{item}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    Ok(map)
}

fn base_ref_exists(
    repo: &git::Repository,
    progress: &mut console::Progress,
    base_ref: &str,
) -> anyhow::Result<bool> {
    let result = git::execute_git_command(
        progress,
        &repo.url,
        console::ExecuteOptions {
            working_directory: Some(repo.full_path.clone()),
            arguments: vec!["rev-parse".into(), "--verify".into(), base_ref.into()],
            ..Default::default()
        },
    );

    Ok(result.is_ok())
}

pub fn build_repo_sync_plan(
    console: console::Console,
    top_progress: &mut console::Progress,
    workspace_arc: workspace::WorkspaceArc,
) -> anyhow::Result<Vec<RepoSyncPlan>> {
    let workspace_members = workspace_arc.read().settings.json.members.clone();
    let dev_branch_rules = workspace_arc.read().settings.json.dev_branches.clone();
    let sync_options = singleton::get_sync_options();
    let use_stash = sync_options.stash;

    let mut member_paths: HashSet<Arc<str>> = HashSet::new();
    let mut dev_branch_paths: HashSet<Arc<str>> = HashSet::new();

    for member_list in workspace_members.values() {
        for member in member_list {
            member_paths.insert(member.path.clone());
            if is_member_dev_branch(member.path.as_ref(), &dev_branch_rules) {
                dev_branch_paths.insert(member.path.clone());
            }
        }
    }

    let merge_repos = resolve_selector_set("--merge", &sync_options.merge_repos, &member_paths)?;
    let no_rebase_repos = resolve_selector_set(
        "--no-rebase-repo",
        &sync_options.no_rebase_repos,
        &member_paths,
    )?;

    let overlap: Vec<_> = merge_repos
        .intersection(&no_rebase_repos)
        .cloned()
        .collect();
    if !overlap.is_empty() {
        let mut sorted = overlap;
        sorted.sort();
        return Err(format_error!(
            "Conflicting dev-branch actions: repos are present in both `--merge` and `--no-rebase-repo`: {}",
            sorted
                .iter()
                .map(|path| format!("//{path}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let parsed_dev_branch_base = parse_dev_branch_base_entries(&sync_options.dev_branch_bases)?;
    let dev_branch_base_map = resolve_dev_branch_base_map(
        parsed_dev_branch_base.as_slice(),
        &member_paths,
        &dev_branch_paths,
    )?;

    let no_rebase_global = sync_options.no_rebase;

    let mut dev_branch_dirty = Vec::new();
    let mut branch_dirty = Vec::new();
    let mut detached_dirty = Vec::new();
    let mut rebase_conflicts = Vec::new();
    let mut merge_conflicts = Vec::new();

    let mut plans = Vec::new();

    for (url, member_list) in workspace_members.iter() {
        for member in member_list.iter() {
            top_progress.set_message(format!("checking {}", member.path).as_str());

            let member_git_path = std::path::Path::new(member.path.as_ref()).join(".git");
            if !member_git_path.exists() {
                continue;
            }

            let mut repo_progress = console::Progress::new(
                console.clone(),
                format!("//{}", member.path),
                None,
                Some(format!("//{} planning sync actions", member.path)),
            );

            let repo = git::Repository::new(url.clone(), member.path.clone());
            let is_dev_branch = dev_branch_paths.contains(&member.path);
            let is_rev_branch = repo.is_branch(&mut repo_progress, &member.rev);

            let is_on_branch = repo
                .get_current_branch(&mut repo_progress)
                .context(format_context!(
                    "//{} while checking current branch",
                    member.path
                ))?
                .is_some();

            let mut action = None;
            let mut skip_reason: Option<Arc<str>> = None;

            if is_dev_branch {
                if no_rebase_repos.contains(&member.path) {
                    action = Some(DevBranchAction::Skip);
                    skip_reason = Some("--no-rebase-repo".into());
                } else if merge_repos.contains(&member.path) {
                    action = Some(DevBranchAction::Merge);
                } else if no_rebase_global {
                    action = Some(DevBranchAction::Skip);
                    skip_reason = Some("--no-rebase".into());
                } else {
                    action = Some(DevBranchAction::Rebase);
                }
            }

            let pull_from =
                (!is_dev_branch && is_on_branch).then(|| format!("origin/{}", member.rev).into());
            let mut rebase_from = matches!(action, Some(DevBranchAction::Rebase))
                .then(|| format!("origin/{}", member.rev).into());
            let mut merge_from = matches!(action, Some(DevBranchAction::Merge))
                .then(|| format!("origin/{}", member.rev).into());

            if (rebase_from.is_some() || merge_from.is_some()) && !is_on_branch {
                rebase_from = None;
                merge_from = None;
                skip_reason = Some("detached HEAD".into());
            }

            if rebase_from.is_some() || merge_from.is_some() {
                let explicit_base_ref = dev_branch_base_map.get(&member.path).cloned();
                let effective_base_ref = explicit_base_ref
                    .clone()
                    .unwrap_or_else(|| format!("origin/{}", member.rev).into());

                if rebase_from.is_some() {
                    rebase_from = Some(effective_base_ref.clone());
                } else {
                    merge_from = Some(effective_base_ref.clone());
                }

                repo.fetch_with_prune(&mut repo_progress, git::IgnoreSubmodules::Yes)
                    .context(format_context!(
                        "//{} while fetching updates before sync planning",
                        member.path
                    ))?;

                if !base_ref_exists(&repo, &mut repo_progress, effective_base_ref.as_ref())? {
                    if explicit_base_ref.is_some() {
                        return Err(format_error!(
                            "//{} explicit base ref `{}` from `--dev-branch-base` was not found after fetch",
                            member.path,
                            effective_base_ref
                        ));
                    }

                    rebase_from = None;
                    merge_from = None;
                    skip_reason = Some(
                        format!(
                            "default base `{}` not found; use `--dev-branch-base={}=<ref>`",
                            effective_base_ref, member.path
                        )
                        .into(),
                    );
                } else if rebase_from.is_some() {
                    match repo
                        .can_rebase_without_conflicts(
                            &mut repo_progress,
                            effective_base_ref.as_ref(),
                        )
                        .context(format_context!(
                            "//{} while checking rebase conflicts",
                            member.path
                        ))? {
                        true => {}
                        false => {
                            rebase_conflicts
                                .push((member.path.clone(), effective_base_ref.clone()));
                        }
                    }
                } else if merge_from.is_some() {
                    match repo
                        .can_merge_without_conflicts(
                            &mut repo_progress,
                            effective_base_ref.as_ref(),
                        )
                        .context(format_context!(
                            "//{} while checking merge conflicts",
                            member.path
                        ))? {
                        true => {}
                        false => {
                            merge_conflicts.push((member.path.clone(), effective_base_ref.clone()));
                        }
                    }
                }
            }

            let is_dirty = repo.is_dirty(&mut repo_progress, git::IgnoreSubmodules::Yes);
            let mut will_stash = false;

            let requires_update =
                pull_from.is_some() || rebase_from.is_some() || merge_from.is_some();
            if is_dirty && requires_update {
                if use_stash {
                    will_stash = true;
                } else if is_dev_branch {
                    dev_branch_dirty.push(member.path.clone());
                } else if is_on_branch {
                    branch_dirty.push(member.path.clone());
                } else {
                    detached_dirty.push(member.path.clone());
                }
            }

            let finalize_message = if let Some(reason) = skip_reason.as_ref() {
                format!("//{} would skip rebase/merge ({reason})", member.path)
            } else if let Some(base_ref) = rebase_from.as_ref() {
                format!("//{} ready for rebase onto {}", member.path, base_ref)
            } else if let Some(base_ref) = merge_from.as_ref() {
                format!("//{} ready for merge from {}", member.path, base_ref)
            } else if let Some(base_ref) = pull_from.as_ref() {
                format!("//{} ready for pull from {}", member.path, base_ref)
            } else {
                format!("//{} no pre-sync updates required", member.path)
            };

            let (finalize_type, finalize_message) = if is_dirty && requires_update && !use_stash {
                (
                    console::FinalType::Failed,
                    format!("//{} cannot sync", member.path),
                )
            } else if skip_reason.is_some() {
                (console::FinalType::NotRequired, finalize_message)
            } else {
                (console::FinalType::Completed, finalize_message)
            };

            repo_progress.set_finalize_lines(console::make_finalize_line(
                finalize_type,
                repo_progress.elapsed(),
                &finalize_message,
            ));

            plans.push(RepoSyncPlan {
                path: member.path.clone(),
                url: url.clone(),
                is_dev_branch,
                is_rev_branch,
                is_on_branch,
                is_dirty,
                pull_from,
                rebase_from,
                merge_from,
                will_stash,
                skip_reason,
            });
        }
    }

    let dirty_repo_count = dev_branch_dirty.len() + branch_dirty.len() + detached_dirty.len();
    let mut container = console::container::Container::new();
    let mut problems = console::components::DescriptionList::new()
        .compact(true)
        .variant(console::components::Variant::Warning);

    if dirty_repo_count > 0 {
        singleton::set_is_error_already_reported();

        if !dev_branch_dirty.is_empty() {
            for repo in &dev_branch_dirty {
                problems.add_item(
                    format!("//{repo}"),
                    "[dev-branch] is dirty and not ready for rebase/merge",
                );
            }
        }

        if !branch_dirty.is_empty() {
            for repo in &branch_dirty {
                problems.add_item(
                    format!("//{repo}"),
                    "[branch] is dirty and not ready for pull",
                );
            }
        }

        if !detached_dirty.is_empty() {
            for repo in &detached_dirty {
                problems.add_item(
                    format!("//{repo}"),
                    "[detached HEAD] is dirty and not ready for sync",
                )
            }
        }
    }

    if !rebase_conflicts.is_empty() || !merge_conflicts.is_empty() {
        singleton::set_is_error_already_reported();

        if !rebase_conflicts.is_empty() {
            for (path, base_ref) in &rebase_conflicts {
                problems.add_item(
                    format!("//{path}"),
                    format!("cannot rebase onto {base_ref} without conflicts"),
                );
            }
        }

        if !merge_conflicts.is_empty() {
            for (path, base_ref) in &merge_conflicts {
                problems.add_item(
                    format!("//{path}"),
                    format!("cannot merge {base_ref} without conflicts"),
                );
            }
        }
    }

    if dirty_repo_count > 0 || !rebase_conflicts.is_empty() || !merge_conflicts.is_empty() {
        container.add(console::bootstrap::VerticalSpacer::new(1));
        container.add(
            console::bootstrap::Banner::new("Sync operation cannot proceed".to_string())
                .variant(console::bootstrap::Variant::Danger)
                .width(console::bootstrap::Width::Large),
        );

        container.add(problems);

        if dirty_repo_count > 0 {
            let mut help_line = console::Line::default();
            help_line.push(console::bootstrap::plain_text("Use "));
            help_line.push(console::bootstrap::code("spaces sync --stash"));
            help_line.push(console::bootstrap::plain_text(
                " to automatically stash/pop changes.",
            ));

            container.add(
                console::components::Alert::new(help_line).width(console::bootstrap::Width::Large),
            );
        }

        container.add(console::bootstrap::VerticalSpacer::new(1));
        console.emit_container(&container);

        return Err(format_error!(
            "Cannot sync: {} repositories need to be resolved manually",
            dirty_repo_count + rebase_conflicts.len() + merge_conflicts.len()
        ));
    }

    Ok(plans)
}

fn format_repo_label(plan: &RepoSyncPlan) -> String {
    let mut label = format!("//{}", plan.path);

    if plan.is_dev_branch {
        label.push_str(" [dev-branch]");
    } else if plan.is_rev_branch {
        label.push_str(" [branch]");
    } else {
        label.push_str(" [commit]");
    }

    label
}

pub fn emit_dry_run_repo_plan(
    console: console::Console,
    plans: &[RepoSyncPlan],
) -> anyhow::Result<()> {
    use console::{components, container};
    let mut container = container::Container::new();

    container.add(components::Header::new(
        components::HeaderLevel::H1,
        "Sync Dry Run Results",
    ));

    let mut description_list = components::DescriptionList::new()
        .compact(true)
        .variant(components::Variant::Primary);

    for plan in plans {
        let action = if let Some(base_ref) = plan.rebase_from.as_ref() {
            if plan.is_dirty && plan.will_stash {
                format!("stash, rebase onto {base_ref}, and pop stash after sync")
            } else {
                format!("rebase onto {base_ref}")
            }
        } else if let Some(base_ref) = plan.merge_from.as_ref() {
            if plan.is_dirty && plan.will_stash {
                format!("stash, merge {base_ref}, and pop stash after sync")
            } else {
                format!("merge {base_ref}")
            }
        } else if let Some(base_ref) = plan.pull_from.as_ref() {
            if plan.is_dirty && plan.will_stash {
                format!("stash, pull from {base_ref}, and pop stash after sync")
            } else {
                format!("pull from {base_ref}")
            }
        } else if plan.is_dev_branch {
            let reason = plan
                .skip_reason
                .clone()
                .unwrap_or_else(|| "no update requested".into());
            format!("skip rebase/merge ({reason})")
        } else if plan.is_on_branch {
            "pull".to_string()
        } else {
            "skip pull (detached HEAD)".to_string()
        };

        description_list.add_item(format_repo_label(plan), action);
    }

    container.add(description_list);
    container.add(console::bootstrap::VerticalSpacer::new(1));
    console.emit_container(&container);

    Ok(())
}

fn pop_stashes_after_failed_pre_sync(
    console: console::Console,
    workspace_arc: workspace::WorkspaceArc,
    stashed_repos: &[Arc<str>],
) -> anyhow::Result<()> {
    if stashed_repos.is_empty() {
        return Ok(());
    }

    if let Err(pop_err) = pop_stashed_repos(console.clone(), workspace_arc, stashed_repos.to_vec())
    {
        console.warning(
            "Failed to pop stashes",
            format!(
                "Some stashes could not be popped: {pop_err}. You may need to manually run 'git stash pop'."
            ),
        )?;
    }

    Ok(())
}

pub fn execute_repo_sync_plan(
    console: console::Console,
    workspace_arc: workspace::WorkspaceArc,
    plans: &[RepoSyncPlan],
) -> anyhow::Result<Vec<Arc<str>>> {
    let mut stashed_repos = Vec::new();

    for plan in plans {
        if !plan.will_stash && plan.rebase_from.is_none() && plan.merge_from.is_none() {
            continue;
        }

        let mut repo_progress = console::Progress::new(
            console.clone(),
            format!("//{}", plan.path),
            None,
            Some(format!("//{} applying pre-sync actions", plan.path)),
        );

        let repo = git::Repository::new(plan.url.clone(), plan.path.clone());

        if plan.will_stash {
            if let Err(error) = repo.stash(&mut repo_progress) {
                repo_progress.set_finalize_lines(console::make_finalize_line(
                    console::FinalType::Failed,
                    repo_progress.elapsed(),
                    &format!("//{} failed to stash changes", plan.path),
                ));
                pop_stashes_after_failed_pre_sync(
                    console.clone(),
                    workspace_arc.clone(),
                    &stashed_repos,
                )?;
                return Err(format_error!(
                    "//{} failed to stash changes: {error}",
                    plan.path
                ));
            }
            stashed_repos.push(plan.path.clone());
        }

        if let Some(base_ref) = plan.rebase_from.as_ref()
            && let Err(error) = repo.rebase_onto(&mut repo_progress, base_ref.as_ref())
        {
            repo_progress.set_finalize_lines(console::make_finalize_line(
                console::FinalType::Failed,
                repo_progress.elapsed(),
                &format!("//{} rebase failed", plan.path),
            ));
            pop_stashes_after_failed_pre_sync(
                console.clone(),
                workspace_arc.clone(),
                &stashed_repos,
            )?;
            return Err(format_error!(
                "//{} failed to rebase onto {}: {error}",
                plan.path,
                base_ref
            ));
        }

        if let Some(base_ref) = plan.merge_from.as_ref()
            && let Err(error) = repo.merge_from(&mut repo_progress, base_ref.as_ref())
        {
            repo_progress.set_finalize_lines(console::make_finalize_line(
                console::FinalType::Failed,
                repo_progress.elapsed(),
                &format!("//{} merge failed", plan.path),
            ));
            pop_stashes_after_failed_pre_sync(
                console.clone(),
                workspace_arc.clone(),
                &stashed_repos,
            )?;
            return Err(format_error!(
                "//{} failed to merge {}: {error}",
                plan.path,
                base_ref
            ));
        }

        let final_message = if plan.will_stash && plan.rebase_from.is_some() {
            format!(
                "//{} stashed changes and rebased onto {}",
                plan.path,
                plan.rebase_from.clone().unwrap_or_default()
            )
        } else if plan.will_stash && plan.merge_from.is_some() {
            format!(
                "//{} stashed changes and merged {}",
                plan.path,
                plan.merge_from.clone().unwrap_or_default()
            )
        } else if plan.will_stash {
            format!("//{} stashed uncommitted changes", plan.path)
        } else if plan.rebase_from.is_some() {
            format!(
                "//{} rebased successfully on {}",
                plan.path,
                plan.rebase_from.clone().unwrap_or_default()
            )
        } else {
            format!(
                "//{} merged successfully from {}",
                plan.path,
                plan.merge_from.clone().unwrap_or_default()
            )
        };

        repo_progress.set_finalize_lines(console::make_finalize_line(
            console::FinalType::Completed,
            repo_progress.elapsed(),
            &final_message,
        ));
    }

    Ok(stashed_repos)
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
            if !stashed_repos.contains(&member.path) {
                continue;
            }

            let mut repo_progress = console::Progress::new(
                console.clone(),
                format!("//{}", member.path),
                None,
                Some(format!("//{} popping stash", member.path)),
            );

            let repo = git::Repository::new(url.clone(), member.path.clone());

            match repo.stash_pop(&mut repo_progress) {
                Ok(_) => {
                    let lines = console::make_finalize_line(
                        console::FinalType::Completed,
                        repo_progress.elapsed(),
                        &format!("//{} popped stash successfully", member.path),
                    );
                    repo_progress.set_finalize_lines(lines);
                }
                Err(e) => {
                    let lines = console::make_finalize_line(
                        console::FinalType::Failed,
                        None,
                        &format!("//{} failed to pop stash", member.path),
                    );
                    repo_progress.set_finalize_lines(lines);
                    console.warning(
                        "Failed to pop stash",
                        format!(
                            "//{} {e}. Manually check this repo with 'git status'",
                            member.path
                        ),
                    )?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arcs(items: &[&str]) -> Vec<Arc<str>> {
        items.iter().map(|item| Arc::<str>::from(*item)).collect()
    }

    fn set(items: &[&str]) -> HashSet<Arc<str>> {
        items.iter().map(|item| Arc::<str>::from(*item)).collect()
    }

    fn parsed_entry(repo_path: &str, base_ref: &str) -> ParsedDevBranchBase {
        ParsedDevBranchBase {
            repo_path: repo_path.into(),
            base_ref: base_ref.into(),
        }
    }

    fn repo_sync_plan_for_label(is_dev_branch: bool, is_rev_branch: bool) -> RepoSyncPlan {
        RepoSyncPlan {
            path: "repo-a".into(),
            url: "https://example.com/repo-a.git".into(),
            is_dev_branch,
            is_rev_branch,
            is_on_branch: true,
            is_dirty: false,
            pull_from: None,
            rebase_from: None,
            merge_from: None,
            will_stash: false,
            skip_reason: None,
        }
    }

    #[test]
    fn parse_dev_branch_base_entries_parses_and_normalizes_repo_paths() {
        let parsed = parse_dev_branch_base_entries(&arcs(&[
            "//repo-a=origin/main",
            "repo-b=refs/heads/trunk",
        ]))
        .unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].repo_path.as_ref(), "repo-a");
        assert_eq!(parsed[0].base_ref.as_ref(), "origin/main");
        assert_eq!(parsed[1].repo_path.as_ref(), "repo-b");
        assert_eq!(parsed[1].base_ref.as_ref(), "refs/heads/trunk");
    }

    #[test]
    fn parse_dev_branch_base_entries_rejects_malformed_arguments() {
        let err = parse_dev_branch_base_entries(&arcs(&["repo-a"])).unwrap_err();

        assert!(format!("{err:#}").contains("expected <repo-path>=<ref>"));
    }

    #[test]
    fn parse_dev_branch_base_entries_rejects_duplicate_repo_after_normalization() {
        let err = parse_dev_branch_base_entries(&arcs(&[
            "//repo-a=origin/main",
            "repo-a=origin/release",
        ]))
        .unwrap_err();

        assert!(format!("{err:#}").contains("Duplicate --dev-branch-base for repo `//repo-a`"));
    }

    #[test]
    fn resolve_selector_set_normalizes_and_deduplicates() {
        let member_paths = set(&["repo-a", "repo-b"]);

        let resolved = resolve_selector_set(
            "--merge",
            &arcs(&["//repo-a", "repo-a", "repo-b"]),
            &member_paths,
        )
        .unwrap();

        assert_eq!(resolved, set(&["repo-a", "repo-b"]));
    }

    #[test]
    fn resolve_selector_set_returns_sorted_unknown_selectors() {
        let err = resolve_selector_set(
            "--merge",
            &arcs(&["//repo-z", "//repo-b"]),
            &set(&["repo-a"]),
        )
        .unwrap_err();

        assert!(
            format!("{err:#}").contains("Unknown repo selectors for --merge: //repo-b, //repo-z")
        );
    }

    #[test]
    fn resolve_dev_branch_base_map_returns_map_for_known_dev_branch_repos() {
        let map = resolve_dev_branch_base_map(
            &[parsed_entry("repo-a", "origin/main")],
            &set(&["repo-a", "repo-b"]),
            &set(&["repo-a"]),
        )
        .unwrap();

        assert_eq!(map.len(), 1);
        assert_eq!(map.get("repo-a").map(|v| v.as_ref()), Some("origin/main"));
    }

    #[test]
    fn resolve_dev_branch_base_map_errors_on_unknown_repo_selectors() {
        let err = resolve_dev_branch_base_map(
            &[
                parsed_entry("repo-z", "origin/main"),
                parsed_entry("repo-b", "origin/dev"),
            ],
            &set(&["repo-a"]),
            &set(&["repo-a"]),
        )
        .unwrap_err();

        assert!(
            format!("{err:#}")
                .contains("Unknown repo selectors for --dev-branch-base: //repo-b, //repo-z")
        );
    }

    #[test]
    fn resolve_dev_branch_base_map_errors_on_non_dev_branch_repos() {
        let err = resolve_dev_branch_base_map(
            &[parsed_entry("repo-b", "origin/main")],
            &set(&["repo-a", "repo-b"]),
            &set(&["repo-a"]),
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains(
            "`--dev-branch-base` is only valid for dev-branch repos. Not dev-branch: //repo-b"
        ));
    }

    #[test]
    fn resolve_dev_branch_base_map_prioritizes_unknown_over_non_dev_branch_errors() {
        let err = resolve_dev_branch_base_map(
            &[
                parsed_entry("repo-unknown", "origin/main"),
                parsed_entry("repo-b", "origin/dev"),
            ],
            &set(&["repo-a", "repo-b"]),
            &set(&["repo-a"]),
        )
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("Unknown repo selectors for --dev-branch-base: //repo-unknown"),
            "unexpected error message: {msg}"
        );
        assert!(
            !msg.contains("Not dev-branch"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn format_repo_label_marks_branch_revs() {
        let plan = repo_sync_plan_for_label(false, true);

        assert_eq!(format_repo_label(&plan), "//repo-a [branch]");
    }

    #[test]
    fn format_repo_label_marks_commit_revs() {
        let plan = repo_sync_plan_for_label(false, false);

        assert_eq!(format_repo_label(&plan), "//repo-a [commit]");
    }

    #[test]
    fn format_repo_label_prioritizes_dev_branch_over_branch_rev() {
        let plan = repo_sync_plan_for_label(true, true);

        assert_eq!(format_repo_label(&plan), "//repo-a [dev-branch]");
    }
}
