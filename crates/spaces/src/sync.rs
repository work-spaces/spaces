use crate::{singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use console::bootstrap::{IntoLine, replace_ascii_with_typography};
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
    rev: Arc<str>,
    pull_from: Option<Arc<str>>,
    rebase_from: Option<Arc<str>>,
    merge_from: Option<Arc<str>>,
    will_stash: bool,
}

#[derive(Debug, Clone)]
pub struct RepoSyncSnapshot {
    path: Arc<str>,
    is_dev_branch: bool,
    is_rev_branch: bool,
    workspace_rev: Arc<str>,
    current_branch: Option<Arc<str>>,
    current_tag: Option<Arc<str>>,
    current_commit_hash: Option<Arc<str>>,
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
) -> anyhow::Result<HashMap<Arc<str>, Arc<str>>> {
    let mut unknown = Vec::new();
    let mut map = HashMap::new();

    for entry in parsed_entries {
        if !member_paths.contains(&entry.repo_path) {
            unknown.push(entry.repo_path.clone());
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

const DRY_RUN_CURRENT_STATUS_HEADER: &str = "Current Status";
const DRY_RUN_PRE_SYNC_HEADER: &str = "Pre-Sync";
const DRY_RUN_SYNC_HEADER: &str = "Sync (May Change Based on Updated Rules)";
const DRY_RUN_POST_SYNC_HEADER: &str = "Post-Sync";

fn has_rev_not_branch_mismatch(
    is_rev_branch: bool,
    is_on_branch: bool,
    has_explicit_base_ref: bool,
) -> bool {
    !is_rev_branch && is_on_branch && !has_explicit_base_ref
}

fn resolve_pull_from(
    is_dev_branch: bool,
    is_on_branch: bool,
    is_rev_branch: bool,
    rev: &str,
) -> Option<Arc<str>> {
    (!is_dev_branch && is_on_branch && is_rev_branch).then(|| format!("origin/{rev}").into())
}

fn describe_current_status(plan: &RepoSyncPlan) -> String {
    if plan.is_dev_branch {
        format!("dev-branch targeting origin/{}", plan.rev)
    } else if plan.is_rev_branch {
        format!("branch tracking remote origin/{}", plan.rev)
    } else {
        format!("commit pinned to {}", plan.rev)
    }
}

fn describe_pre_sync_action(plan: &RepoSyncPlan) -> Option<String> {
    if let Some(base_ref) = plan.rebase_from.as_ref() {
        if plan.will_stash {
            Some(format!("stash, then rebase onto {base_ref}"))
        } else {
            Some(format!("rebase onto {base_ref}"))
        }
    } else if let Some(base_ref) = plan.merge_from.as_ref() {
        if plan.will_stash {
            Some(format!("stash, then merge {base_ref}"))
        } else {
            Some(format!("merge {base_ref}"))
        }
    } else if plan.will_stash {
        Some("stash local changes".to_string())
    } else {
        None
    }
}

fn describe_sync_action(plan: &RepoSyncPlan) -> Option<String> {
    plan.pull_from
        .as_ref()
        .map(|base_ref| format!("pull from {base_ref}"))
}

fn describe_post_sync_action(plan: &RepoSyncPlan) -> Option<String> {
    if plan.will_stash {
        Some("pop stashed changes".to_string())
    } else {
        None
    }
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
    let dev_branch_base_map =
        resolve_dev_branch_base_map(parsed_dev_branch_base.as_slice(), &member_paths)?;

    let no_rebase_global = sync_options.no_rebase;

    let mut dev_branch_dirty = Vec::new();
    let mut branch_dirty = Vec::new();
    let mut detached_dirty = Vec::new();
    let mut rev_not_branch_but_on_branch: Vec<(Arc<str>, Arc<str>)> = Vec::new();
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

            let detached_head_rev = if !is_on_branch {
                repo.get_commit_tag(&mut repo_progress)
                    .or_else(|| repo.get_commit_hash(&mut repo_progress).ok().flatten())
            } else {
                None
            };

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

            let explicit_base_ref = dev_branch_base_map.get(&member.path).cloned();
            let rev_not_branch_mismatch = has_rev_not_branch_mismatch(
                is_rev_branch,
                is_on_branch,
                explicit_base_ref.is_some(),
            );
            if rev_not_branch_mismatch {
                rev_not_branch_but_on_branch.push((member.path.clone(), member.rev.clone()));
                skip_reason = Some("rev is not a branch".into());
            }

            let pull_from =
                resolve_pull_from(is_dev_branch, is_on_branch, is_rev_branch, &member.rev);
            let mut rebase_from = None;
            let mut merge_from = None;

            if matches!(
                action,
                Some(DevBranchAction::Rebase) | Some(DevBranchAction::Merge)
            ) {
                if !is_on_branch {
                    skip_reason = Some("detached HEAD".into());
                } else if rev_not_branch_mismatch {
                    skip_reason = Some("rev is not a branch".into());
                } else {
                    let effective_base_ref = explicit_base_ref
                        .clone()
                        .unwrap_or_else(|| format!("origin/{}", member.rev).into());
                    if matches!(action, Some(DevBranchAction::Rebase)) {
                        rebase_from = Some(effective_base_ref);
                    } else {
                        merge_from = Some(effective_base_ref);
                    }
                }
            }

            if rebase_from.is_some() || merge_from.is_some() {
                let effective_base_ref = rebase_from
                    .clone()
                    .or_else(|| merge_from.clone())
                    .expect("pre-sync action base ref should be present");

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

            let finalize_message = if let Some(base_ref) = rebase_from.as_ref() {
                format!("//{} ready for rebase onto {}", member.path, base_ref)
            } else if let Some(base_ref) = merge_from.as_ref() {
                format!("//{} ready for merge from {}", member.path, base_ref)
            } else if let Some(base_ref) = pull_from.as_ref() {
                format!("//{} ready for pull from {}", member.path, base_ref)
            } else if let Some(reason) = skip_reason.as_ref() {
                format!("//{} no pre-sync action ({reason})", member.path)
            } else if !is_rev_branch {
                format!("//{} no pre-sync action (rev is not a branch)", member.path)
            } else if !is_on_branch {
                if let Some(detached_rev) = detached_head_rev.as_ref() {
                    format!(
                        "//{} no pre-sync action (detached HEAD at `{detached_rev}`)",
                        member.path
                    )
                } else {
                    format!("//{} no pre-sync action (detached HEAD)", member.path)
                }
            } else {
                format!("//{} no pre-sync action", member.path)
            };

            let (finalize_type, finalize_message) = if rev_not_branch_mismatch {
                (
                    console::FinalType::Failed,
                    format!("//{} cannot sync (rev is not a branch)", member.path),
                )
            } else if is_dirty && requires_update && !use_stash {
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
                rev: member.rev.clone(),
                pull_from,
                rebase_from,
                merge_from,
                will_stash,
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

    if !rev_not_branch_but_on_branch.is_empty() {
        singleton::set_is_error_already_reported();

        rev_not_branch_but_on_branch.sort_by(|(repo_a, _), (repo_b, _)| repo_a.cmp(repo_b));
        for (repo, _) in &rev_not_branch_but_on_branch {
            problems.add_item(
                format!("//{repo}"),
                "workspace rev is not a branch while local HEAD is on a branch\npinned/non-branch revs cannot be pulled/rebased by default.",
            );
        }
    }

    if dirty_repo_count > 0
        || !rebase_conflicts.is_empty()
        || !merge_conflicts.is_empty()
        || !rev_not_branch_but_on_branch.is_empty()
    {
        container.add(console::bootstrap::VerticalSpacer::new(1));
        container.add(
            console::bootstrap::Banner::new("Sync operation cannot proceed".to_string())
                .variant(console::bootstrap::Variant::Danger)
                .width(console::bootstrap::Width::Large),
        );

        container.add(problems);

        let mut recommended_actions = Vec::new();

        if dirty_repo_count > 0 {
            let mut stash_line = console::Line::default();
            stash_line.push(console::bootstrap::plain_text("Use "));
            stash_line.push(console::bootstrap::code("spaces sync --stash"));
            stash_line.push(console::bootstrap::plain_text(
                " to automatically stash/pop changes.",
            ));
            recommended_actions.push(stash_line);
            recommended_actions.extend(console::bootstrap::VerticalSpacer::new(1).render());
        }

        for (repo, rev) in &rev_not_branch_but_on_branch {
            recommended_actions
                .push(console::bootstrap::plain_text(format!("For //{repo}")).into_line());
            recommended_actions.push(
                console::bootstrap::plain_text("  Checkout the workspace `rev`:").into_line(),
            );

            recommended_actions.push(
                console::bootstrap::code(format!("    git -C {repo} checkout {rev}")).into_line(),
            );

            let mut dev_branch_intro_line = console::Line::default();
            dev_branch_intro_line.push(console::bootstrap::plain_text(
                "  Or convert to a dev branch:",
            ));
            recommended_actions.push(dev_branch_intro_line);

            let mut dev_branch_cmd_line = console::Line::default();
            dev_branch_cmd_line.push(console::bootstrap::code("    spaces sync \\"));
            recommended_actions.push(dev_branch_cmd_line);

            let mut dev_branch_repo_line = console::Line::default();
            dev_branch_repo_line.push(console::bootstrap::code(format!(
                "      --dev-branch=//{repo} \\"
            )));
            recommended_actions.push(dev_branch_repo_line);

            let mut dev_branch_base_line = console::Line::default();
            dev_branch_base_line.push(console::bootstrap::code(format!(
                "      --dev-branch-base=//{repo}=<remote-branch>"
            )));
            dev_branch_base_line.push(console::bootstrap::plain_text("."));
            recommended_actions.push(dev_branch_base_line);
        }

        if !recommended_actions.is_empty() {
            container.add(
                console::components::Alert::new(recommended_actions)
                    .title("Recommended Actions")
                    .width(console::bootstrap::Width::Large),
            );
        }

        container.add(
            console::bootstrap::Divider::new().style(console::bootstrap::DividerStyle::Double),
        );

        container.add(console::bootstrap::VerticalSpacer::new(1));
        console.emit_container(&container);

        return Err(format_error!(
            "Cannot sync: {} repositories need to be resolved manually",
            dirty_repo_count
                + rebase_conflicts.len()
                + merge_conflicts.len()
                + rev_not_branch_but_on_branch.len()
        ));
    }

    Ok(plans)
}

fn format_repo_label_from_kind(path: &str, is_dev_branch: bool, is_rev_branch: bool) -> String {
    let mut label = format!("//{path}");

    if is_dev_branch {
        label.push_str(" [dev-branch]");
    } else if is_rev_branch {
        label.push_str(" [branch]");
    } else {
        label.push_str(" [commit]");
    }

    label
}

fn format_repo_label(plan: &RepoSyncPlan) -> String {
    format_repo_label_from_kind(plan.path.as_ref(), plan.is_dev_branch, plan.is_rev_branch)
}

fn short_commit(commit: Option<&Arc<str>>) -> Arc<str> {
    commit
        .map(|hash| hash.chars().take(8).collect::<String>().into())
        .unwrap_or_else(|| "unknown".into())
}

fn describe_snapshot_ref(snapshot: &RepoSyncSnapshot) -> String {
    if let Some(tag) = snapshot.current_tag.as_ref() {
        return tag.to_string();
    }

    if let Some(branch) = snapshot.current_branch.as_ref() {
        if let Some(hash) = snapshot.current_commit_hash.as_ref() {
            return format!("{} {}", branch, hash.chars().take(8).collect::<String>());
        }

        return branch.to_string();
    }

    if let Some(hash) = snapshot.current_commit_hash.as_ref() {
        return hash.chars().take(8).collect::<String>();
    }

    snapshot.workspace_rev.to_string()
}

fn describe_pre_sync_action_for_complete_summary(plan: &RepoSyncPlan) -> Option<String> {
    if plan.will_stash {
        return Some("stash local changes".to_string());
    }

    if plan.rebase_from.is_some() || plan.merge_from.is_some() {
        return None;
    }

    describe_pre_sync_action(plan)
}

fn pre_post_actions(plan: Option<&RepoSyncPlan>) -> Vec<String> {
    let mut actions = Vec::new();

    if let Some(plan) = plan {
        if let Some(action) = describe_pre_sync_action_for_complete_summary(plan) {
            actions.push(format!("pre-sync: {action}"));
        }

        if let Some(action) = describe_post_sync_action(plan) {
            actions.push(format!("post-sync: {action}"));
        }
    }

    actions
}

fn describe_sync_complete_result(
    before: &RepoSyncSnapshot,
    after: &RepoSyncSnapshot,
    plan: Option<&RepoSyncPlan>,
) -> String {
    let transition = replace_ascii_with_typography("->");

    let mut summary = if let Some(plan) = plan {
        if let Some(base_ref) = plan.rebase_from.as_ref() {
            format!("rebased onto {base_ref}")
        } else if let Some(base_ref) = plan.merge_from.as_ref() {
            format!("merged {base_ref}")
        } else if plan.pull_from.is_some() {
            let before_hash = short_commit(before.current_commit_hash.as_ref());
            let after_hash = short_commit(after.current_commit_hash.as_ref());
            if before_hash == after_hash {
                format!("pulled {} {before_hash} (no change)", plan.rev)
            } else {
                format!(
                    "pulled {} {before_hash} {transition} {after_hash}",
                    plan.rev,
                )
            }
        } else {
            format!(
                "{} {transition} {}",
                describe_snapshot_ref(before),
                describe_snapshot_ref(after)
            )
        }
    } else {
        format!(
            "{} {transition} {}",
            describe_snapshot_ref(before),
            describe_snapshot_ref(after)
        )
    };

    let actions = pre_post_actions(plan);
    if !actions.is_empty() {
        summary.push_str(" | ");
        summary.push_str(actions.join("; ").as_str());
    }

    summary
}

pub fn collect_repo_sync_snapshots(
    console: console::Console,
    workspace_arc: workspace::WorkspaceArc,
    progress_name: &str,
) -> anyhow::Result<Vec<RepoSyncSnapshot>> {
    let workspace_members = workspace_arc.read().settings.json.members.clone();
    let dev_branch_rules = workspace_arc.read().settings.json.dev_branches.clone();

    let mut snapshots = Vec::new();
    let mut progress = console::Progress::new(console, progress_name, None, None);

    for (url, member_list) in workspace_members.iter() {
        for member in member_list {
            progress.set_message(format!("capturing //{}", member.path).as_str());

            let member_git_path = std::path::Path::new(member.path.as_ref()).join(".git");
            if !member_git_path.exists() {
                continue;
            }

            let repo = git::Repository::new(url.clone(), member.path.clone());
            let is_dev_branch = is_member_dev_branch(member.path.as_ref(), &dev_branch_rules);
            let is_rev_branch = repo.is_branch(&mut progress, &member.rev);
            let current_branch = repo.get_current_branch(&mut progress).ok().flatten();
            let current_tag = repo.get_commit_tag(&mut progress);
            let current_commit_hash = repo.get_commit_hash(&mut progress).ok().flatten();

            snapshots.push(RepoSyncSnapshot {
                path: member.path.clone(),
                is_dev_branch,
                is_rev_branch,
                workspace_rev: member.rev.clone(),
                current_branch,
                current_tag,
                current_commit_hash,
            });
        }
    }

    snapshots.sort_by(|a, b| a.path.cmp(&b.path));

    progress.set_finalize_lines(console::make_finalize_line(
        console::FinalType::Completed,
        progress.elapsed(),
        &format!(
            "captured current revisions for {} repositories",
            snapshots.len()
        ),
    ));

    Ok(snapshots)
}

pub fn emit_sync_complete_report(
    console: console::Console,
    before_snapshots: &[RepoSyncSnapshot],
    after_snapshots: &[RepoSyncSnapshot],
    plans: &[RepoSyncPlan],
) -> anyhow::Result<()> {
    use console::container;

    let after_by_path = after_snapshots
        .iter()
        .map(|snapshot| (snapshot.path.clone(), snapshot))
        .collect::<HashMap<_, _>>();
    let plan_by_path = plans
        .iter()
        .map(|plan| (plan.path.clone(), plan))
        .collect::<HashMap<_, _>>();

    let report_rows = before_snapshots
        .iter()
        .map(|before| {
            let after = after_by_path.get(&before.path).copied().unwrap_or(before);
            let plan = plan_by_path.get(&before.path).copied();
            let label = format_repo_label_from_kind(
                before.path.as_ref(),
                before.is_dev_branch,
                before.is_rev_branch,
            );
            let summary = describe_sync_complete_result(before, after, plan);
            (label, summary)
        })
        .collect::<Vec<_>>();

    let mut container = container::Container::new();

    container.add(console::bootstrap::VerticalSpacer::new(1));
    container.add(
        console::bootstrap::Banner::new(format!(
            "{} Sync Complete ",
            console::bootstrap::icon_success()
        ))
        .width(console::bootstrap::Width::Large)
        .variant(console::bootstrap::Variant::Success),
    );

    let mut report_list = console::bootstrap::DescriptionList::new()
        .compact(true)
        .variant(console::bootstrap::Variant::Primary);
    for (label, summary) in report_rows {
        report_list = report_list.item(label, summary);
    }
    container.add(report_list);

    container.add(console::bootstrap::VerticalSpacer::new(1));
    container
        .add(console::bootstrap::Divider::new().style(console::bootstrap::DividerStyle::Double));

    console.emit_container(&container);

    Ok(())
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

    let sorted_plans = plans
        .iter()
        .sorted_by(|a, b| a.path.cmp(&b.path))
        .collect::<Vec<_>>();

    let current_status_entries = sorted_plans
        .iter()
        .map(|plan| (format_repo_label(plan), describe_current_status(plan)))
        .collect::<Vec<_>>();

    if !current_status_entries.is_empty() {
        container.add(components::Header::new(
            components::HeaderLevel::H2,
            DRY_RUN_CURRENT_STATUS_HEADER,
        ));

        let mut status_list = components::DescriptionList::new()
            .compact(true)
            .variant(components::Variant::Primary);

        for (repo, status) in current_status_entries {
            status_list.add_item(repo, status);
        }

        container.add(status_list);
        container.add(console::bootstrap::VerticalSpacer::new(1));
    }

    let pre_sync_entries = sorted_plans
        .iter()
        .filter_map(|plan| {
            describe_pre_sync_action(plan).map(|action| (format_repo_label(plan), action))
        })
        .collect::<Vec<_>>();

    if !pre_sync_entries.is_empty() {
        container.add(components::Header::new(
            components::HeaderLevel::H2,
            DRY_RUN_PRE_SYNC_HEADER,
        ));

        let mut pre_sync_actions = components::DescriptionList::new()
            .compact(true)
            .variant(components::Variant::Primary);

        for (repo, action) in pre_sync_entries {
            pre_sync_actions.add_item(repo, action);
        }

        container.add(pre_sync_actions);
        container.add(console::bootstrap::VerticalSpacer::new(1));
    }

    let sync_entries = sorted_plans
        .iter()
        .filter_map(|plan| {
            describe_sync_action(plan).map(|action| (format_repo_label(plan), action))
        })
        .collect::<Vec<_>>();

    if !sync_entries.is_empty() {
        container.add(components::Header::new(
            components::HeaderLevel::H2,
            DRY_RUN_SYNC_HEADER,
        ));

        let mut sync_actions = components::DescriptionList::new()
            .compact(true)
            .variant(components::Variant::Primary);

        for (repo, action) in sync_entries {
            sync_actions.add_item(repo, action);
        }

        container.add(sync_actions);
        container.add(console::bootstrap::VerticalSpacer::new(1));
    }

    let post_sync_entries = sorted_plans
        .iter()
        .filter_map(|plan| {
            describe_post_sync_action(plan).map(|action| (format_repo_label(plan), action))
        })
        .collect::<Vec<_>>();

    if !post_sync_entries.is_empty() {
        container.add(components::Header::new(
            components::HeaderLevel::H2,
            DRY_RUN_POST_SYNC_HEADER,
        ));

        let mut post_sync_actions = components::DescriptionList::new()
            .compact(true)
            .variant(components::Variant::Primary);

        for (repo, action) in post_sync_entries {
            post_sync_actions.add_item(repo, action);
        }

        container.add(post_sync_actions);
        container.add(console::bootstrap::VerticalSpacer::new(1));
    }

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

    fn repo_sync_plan(
        is_dev_branch: bool,
        is_rev_branch: bool,
        _is_on_branch: bool,
        pull_from: Option<&str>,
        rebase_from: Option<&str>,
        merge_from: Option<&str>,
        will_stash: bool,
        _skip_reason: Option<&str>,
    ) -> RepoSyncPlan {
        RepoSyncPlan {
            path: "repo-a".into(),
            url: "https://example.com/repo-a.git".into(),
            is_dev_branch,
            is_rev_branch,
            rev: "main".into(),
            pull_from: pull_from.map(Arc::<str>::from),
            rebase_from: rebase_from.map(Arc::<str>::from),
            merge_from: merge_from.map(Arc::<str>::from),
            will_stash,
        }
    }

    fn repo_sync_plan_for_label(is_dev_branch: bool, is_rev_branch: bool) -> RepoSyncPlan {
        repo_sync_plan(
            is_dev_branch,
            is_rev_branch,
            true,
            None,
            None,
            None,
            false,
            None,
        )
    }

    fn repo_sync_snapshot(
        is_dev_branch: bool,
        is_rev_branch: bool,
        workspace_rev: &str,
        current_branch: Option<&str>,
        current_tag: Option<&str>,
        current_commit_hash: Option<&str>,
    ) -> RepoSyncSnapshot {
        RepoSyncSnapshot {
            path: "repo-a".into(),
            is_dev_branch,
            is_rev_branch,
            workspace_rev: workspace_rev.into(),
            current_branch: current_branch.map(Arc::<str>::from),
            current_tag: current_tag.map(Arc::<str>::from),
            current_commit_hash: current_commit_hash.map(Arc::<str>::from),
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
    fn resolve_dev_branch_base_map_returns_map_for_known_member_repos() {
        let map = resolve_dev_branch_base_map(
            &[
                parsed_entry("repo-a", "origin/main"),
                parsed_entry("repo-b", "origin/release"),
            ],
            &set(&["repo-a", "repo-b"]),
        )
        .unwrap();

        assert_eq!(map.len(), 2);
        assert_eq!(map.get("repo-a").map(|v| v.as_ref()), Some("origin/main"));
        assert_eq!(
            map.get("repo-b").map(|v| v.as_ref()),
            Some("origin/release")
        );
    }

    #[test]
    fn resolve_dev_branch_base_map_errors_on_unknown_repo_selectors() {
        let err = resolve_dev_branch_base_map(
            &[
                parsed_entry("repo-z", "origin/main"),
                parsed_entry("repo-b", "origin/dev"),
            ],
            &set(&["repo-a"]),
        )
        .unwrap_err();

        assert!(
            format!("{err:#}")
                .contains("Unknown repo selectors for --dev-branch-base: //repo-b, //repo-z")
        );
    }

    #[test]
    fn resolve_pull_from_skips_non_branch_revs() {
        let pull_from = resolve_pull_from(false, true, false, "deadbeef");

        assert_eq!(pull_from, None);
    }

    #[test]
    fn rev_not_branch_mismatch_requires_explicit_base_to_be_cleared() {
        assert!(has_rev_not_branch_mismatch(false, true, false));
        assert!(!has_rev_not_branch_mismatch(false, true, true));
    }

    #[test]
    fn describe_pre_sync_action_omits_non_action_entries() {
        let plan = repo_sync_plan(false, false, true, None, None, None, false, None);

        assert_eq!(describe_pre_sync_action(&plan), None);
    }

    #[test]
    fn describe_pre_sync_action_reports_stash_then_rebase() {
        let plan = repo_sync_plan(
            true,
            true,
            true,
            None,
            Some("origin/main"),
            None,
            true,
            None,
        );

        assert_eq!(
            describe_pre_sync_action(&plan),
            Some("stash, then rebase onto origin/main".to_string())
        );
    }

    #[test]
    fn describe_pre_sync_action_reports_stash_for_dirty_repo() {
        let plan = repo_sync_plan(
            false,
            true,
            true,
            Some("origin/main"),
            None,
            None,
            true,
            None,
        );

        assert_eq!(
            describe_pre_sync_action(&plan),
            Some("stash local changes".to_string())
        );
    }

    #[test]
    fn describe_sync_action_reports_pull() {
        let plan = repo_sync_plan(
            false,
            true,
            true,
            Some("origin/main"),
            None,
            None,
            false,
            None,
        );

        assert_eq!(
            describe_sync_action(&plan),
            Some("pull from origin/main".to_string())
        );
    }

    #[test]
    fn describe_sync_action_omits_non_pull_entries() {
        let plan = repo_sync_plan(
            true,
            true,
            true,
            None,
            Some("origin/main"),
            None,
            false,
            None,
        );

        assert_eq!(describe_sync_action(&plan), None);
    }

    #[test]
    fn describe_post_sync_action_reports_stash_pop() {
        let plan = repo_sync_plan(
            false,
            true,
            true,
            Some("origin/main"),
            None,
            None,
            true,
            None,
        );

        assert_eq!(
            describe_post_sync_action(&plan),
            Some("pop stashed changes".to_string())
        );
    }

    #[test]
    fn dry_run_section_headers_are_stable() {
        assert_eq!(DRY_RUN_CURRENT_STATUS_HEADER, "Current Status");
        assert_eq!(DRY_RUN_PRE_SYNC_HEADER, "Pre-Sync");
        assert_eq!(
            DRY_RUN_SYNC_HEADER,
            "Sync (May Change Based on Updated Rules)"
        );
        assert_eq!(DRY_RUN_POST_SYNC_HEADER, "Post-Sync");
    }

    #[test]
    fn describe_current_status_for_dev_branch() {
        let mut plan = repo_sync_plan_for_label(true, true);
        plan.rev = "develop".into();

        assert_eq!(
            describe_current_status(&plan),
            "dev-branch targeting origin/develop"
        );
    }

    #[test]
    fn describe_current_status_for_branch() {
        let mut plan = repo_sync_plan_for_label(false, true);
        plan.rev = "main".into();

        assert_eq!(
            describe_current_status(&plan),
            "branch tracking remote origin/main"
        );
    }

    #[test]
    fn describe_current_status_for_commit() {
        let mut plan = repo_sync_plan_for_label(false, false);
        plan.rev = "deadbeef".into();

        assert_eq!(describe_current_status(&plan), "commit pinned to deadbeef");
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

    #[test]
    fn describe_snapshot_ref_prefers_tag() {
        let snapshot = repo_sync_snapshot(
            false,
            false,
            "deadbeef",
            None,
            Some("v0.4.0"),
            Some("0123456789abcdef"),
        );

        assert_eq!(describe_snapshot_ref(&snapshot), "v0.4.0");
    }

    #[test]
    fn describe_sync_complete_result_reports_pull_with_short_hashes() {
        let plan = repo_sync_plan(
            false,
            true,
            true,
            Some("origin/main"),
            None,
            None,
            false,
            None,
        );
        let before = repo_sync_snapshot(
            false,
            true,
            "main",
            Some("main"),
            None,
            Some("0123456789abcdef"),
        );
        let after = repo_sync_snapshot(
            false,
            true,
            "main",
            Some("main"),
            None,
            Some("fedcba9876543210"),
        );

        assert_eq!(
            describe_sync_complete_result(&before, &after, Some(&plan)),
            "pulled main 01234567 ⇒ fedcba98"
        );
    }

    #[test]
    fn describe_sync_complete_result_includes_pre_and_post_actions() {
        let plan = repo_sync_plan(
            true,
            true,
            true,
            None,
            Some("origin/main"),
            None,
            true,
            None,
        );
        let before = repo_sync_snapshot(
            true,
            true,
            "main",
            Some("feature"),
            None,
            Some("0123456789abcdef"),
        );
        let after = repo_sync_snapshot(
            true,
            true,
            "main",
            Some("feature"),
            None,
            Some("fedcba9876543210"),
        );

        assert_eq!(
            describe_sync_complete_result(&before, &after, Some(&plan)),
            "rebased onto origin/main | pre-sync: stash local changes; post-sync: pop stashed changes"
        );
    }

    #[test]
    fn describe_sync_complete_result_omits_redundant_pre_sync_rebase() {
        let plan = repo_sync_plan(
            true,
            true,
            true,
            None,
            Some("origin/main"),
            None,
            false,
            None,
        );
        let before = repo_sync_snapshot(
            true,
            true,
            "main",
            Some("feature"),
            None,
            Some("0123456789abcdef"),
        );
        let after = repo_sync_snapshot(
            true,
            true,
            "main",
            Some("feature"),
            None,
            Some("fedcba9876543210"),
        );

        assert_eq!(
            describe_sync_complete_result(&before, &after, Some(&plan)),
            "rebased onto origin/main"
        );
    }
}
