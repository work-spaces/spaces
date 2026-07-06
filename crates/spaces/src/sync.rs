use crate::{singleton, workspace};
use anyhow::Context;
use anyhow_source_location::{format_context, format_error};
use console::bootstrap::{IntoLine, replace_ascii_with_typography};
use itertools::Itertools;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use utils::{git, ws};

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
    create_new_branch: Option<Arc<str>>,
    new_branch_tracking_base: Option<Arc<str>>,
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

type NewBranchRevMismatch = (Arc<str>, Arc<str>, Arc<str>, Arc<str>);
type NewBranchAlreadyExists = (Arc<str>, Arc<str>);
type NonBranchRevTrackingAttempt = (Arc<str>, Arc<str>, bool, bool);

#[derive(Debug, clap::Args)]
pub struct SyncArgs {
    #[arg(
        long,
        help = r#"Environment variables to add to the workspace.
  Use `--env=VAR=VALUE`. Makes workspace not reproducible."#
    )]
    pub env: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Store values accessible via workspace.load_value().
  Use `--store=KEY=VALUE`. Values are stored with path `//` and url `<command line>`.
  Command line store values take priority over all other path or url values."#
    )]
    pub store: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Remove a store value previously set via --store=KEY=VALUE.
  Use `--no-store=KEY`. Removes the named key from the command-line store entry."#
    )]
    pub no_store: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Use --dev-branch=<repo-path> to add a repo to the dev-branch list.
  Unlike --new-branch, this does not create a new git branch."#
    )]
    pub dev_branch: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Use --new-branch=<repo-path> to create a new branch from the repo's configured workspace rev.
  The new branch name matches the workspace name and the repo is marked as a dev-branch.
  This flag can be used multiple times."#
    )]
    pub new_branch: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Use --no-dev-branch=<repo-path> to remove a repo from the dev-branch list.
  This has the opposite effect of --dev-branch."#
    )]
    pub no_dev_branch: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Stash uncommitted changes before sync and pop the stash after sync.
  This allows syncing dirty repositories without manually stashing."#
    )]
    pub stash: bool,
    #[arg(
        long,
        help = r#"Same as --skip-pre-evaluation. This option will be removed in a future version."#
    )]
    pub allow_dirty: bool,
    #[arg(
        long,
        help = r#"Skip repository status checks and rebase operations.
  Use with caution: this bypasses safety checks for dirty repos and rebase conflicts."#
    )]
    pub skip_pre_evaluation: bool,
    #[arg(
        long,
        help = r#"For matching dev-branch repos, merge instead of rebase.
  Use `--merge=<repo-path>`. This flag can be used multiple times."#
    )]
    pub merge: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"For matching dev-branch repos, skip both rebase and merge.
  Use `--no-rebase-repo=<repo-path>`. This flag can be used multiple times."#
    )]
    pub no_rebase_repo: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Skip rebase for all dev-branch repos unless explicitly listed in `--merge`."#
    )]
    pub no_rebase: bool,
    #[arg(
        long,
        help = r#"Override sync base ref for a repo.
  Use `--dev-branch-base=<repo-path>=<ref>`. This flag can be used multiple times.
  Useful for dev-branch rebases/merges and for non-branch rev repos checked out on a local branch."#
    )]
    pub dev_branch_base: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Remove a stored sync base override for a repo.
  Use `--no-dev-branch-base=<repo-path>`. This has the opposite effect of --dev-branch-base."#
    )]
    pub no_dev_branch_base: Vec<Arc<str>>,
    #[arg(
        long,
        help = r#"Run sync pre-evaluation planning only and print what would happen.
  Does not modify repositories and does not execute evaluation tasks."#
    )]
    pub dry_run: bool,
    #[arg(
        long,
        help = r#"Skip Starlark evaluation and task execution.
  Still runs sync pre-evaluation and sync post-evaluation actions, including branch updates and stash pop."#
    )]
    pub skip_evaluation: bool,
    /// The workspace lock rev's will override the rule rev for repos during sync.
    #[arg(long)]
    pub locked: bool,
}

fn is_member_dev_branch(member_path: &str, dev_branch_rules: &[Arc<str>]) -> bool {
    let normalized_member_path = ws::normalize_repo_selector(member_path);

    for item in dev_branch_rules {
        let normalized_item = ws::normalize_repo_selector(item.as_ref());
        if ws::normalized_selector_matches_member_path(
            normalized_member_path.as_ref(),
            normalized_item.as_ref(),
        ) {
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
        let normalized = ws::normalize_repo_selector(selector.as_ref());
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

const DRY_RUN_CURRENT_STATUS_HEADER: &str = "Current Status";
const DRY_RUN_PRE_SYNC_HEADER: &str = "Sync Pre-Evaluation (Pre-Eval)";
const DRY_RUN_SYNC_HEADER: &str = "Evaluation (May Change Based on Updated Rules)";
const DRY_RUN_POST_SYNC_HEADER: &str = "Sync Post-Evaluation";
const MAX_REPO_SYNC_PARALLEL_JOBS: usize = 8;

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

fn resolve_explicit_dev_branch_base(
    member_path: &str,
    dev_branch_base_map: &HashMap<Arc<str>, Arc<str>>,
) -> Option<Arc<str>> {
    let normalized_member_path = ws::normalize_repo_selector(member_path);

    if let Some(base_ref) = dev_branch_base_map.get(&normalized_member_path) {
        return Some(base_ref.clone());
    }

    dev_branch_base_map
        .iter()
        .filter_map(|(selector, base_ref)| {
            let normalized_selector = ws::normalize_repo_selector(selector.as_ref());
            if ws::normalized_selector_matches_member_path(
                normalized_member_path.as_ref(),
                normalized_selector.as_ref(),
            ) {
                Some((normalized_selector.len(), base_ref.clone()))
            } else {
                None
            }
        })
        .max_by_key(|(selector_len, _)| *selector_len)
        .map(|(_, base_ref)| base_ref)
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
    if let Some(branch_name) = plan.create_new_branch.as_ref() {
        if let Some(base_ref) = plan.new_branch_tracking_base.as_ref() {
            return Some(format!(
                "create dev branch `{branch_name}`\n  from {}\n  track {base_ref} via --dev-branch-base",
                plan.rev
            ));
        }

        return Some(format!(
            "create dev branch `{branch_name}`\n   from {}",
            plan.rev
        ));
    }

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

fn recommend_dev_branch(branch_flag: &str, repo: &str) -> Vec<console::Line> {
    let mut result = Vec::new();
    result.push(console::bootstrap::code("    spaces sync \\").into_line());
    result.push(console::bootstrap::code(format!("      --{branch_flag}=//{repo} \\")).into_line());
    result.push(
        console::bootstrap::code(format!("      --dev-branch-base=//{repo}=<remote-branch>"))
            .into_line(),
    );
    result
}

pub fn build_repo_sync_plan(
    console: console::Console,
    top_progress: &mut console::Progress,
    workspace_arc: workspace::WorkspaceArc,
) -> anyhow::Result<Vec<RepoSyncPlan>> {
    let workspace_members = workspace_arc.read().settings.json.members.clone();
    let dev_branch_rules = workspace_arc.read().settings.json.dev_branches.clone();
    let stored_dev_branch_base_map = workspace_arc.read().settings.json.dev_branch_bases.clone();
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

    let mut dev_branch_base_map = HashMap::new();
    for (repo_selector, base_ref) in stored_dev_branch_base_map {
        dev_branch_base_map.insert(
            ws::normalize_repo_selector(repo_selector.as_ref()),
            base_ref,
        );
    }

    let requested_dev_branch_selectors = sync_options
        .dev_branch_repos
        .iter()
        .map(|selector| ws::normalize_repo_selector(selector.as_ref()))
        .collect::<Vec<_>>();

    let new_branch_repos = resolve_selector_set(
        "--new-branch",
        &sync_options.new_branch_repos,
        &member_paths,
    )?;

    let workspace_new_branch_name = workspace_arc.read().get_new_branch_name();
    let no_rebase_global = sync_options.no_rebase;

    let mut dev_branch_dirty = Vec::new();
    let mut branch_dirty = Vec::new();
    let mut detached_dirty = Vec::new();
    let mut rev_not_branch_but_on_branch: Vec<(Arc<str>, Arc<str>)> = Vec::new();
    let mut rebase_conflicts = Vec::new();
    let mut merge_conflicts = Vec::new();
    let mut non_branch_rev_tracking_attempts: Vec<NonBranchRevTrackingAttempt> = Vec::new();
    let mut new_branch_rev_mismatches: Vec<NewBranchRevMismatch> = Vec::new();
    let mut new_branch_already_exists: Vec<NewBranchAlreadyExists> = Vec::new();

    struct RepoPlanEvaluation {
        plan: RepoSyncPlan,
        dev_branch_dirty: Option<Arc<str>>,
        branch_dirty: Option<Arc<str>>,
        detached_dirty: Option<Arc<str>>,
        rev_not_branch_but_on_branch: Option<(Arc<str>, Arc<str>)>,
        rebase_conflict: Option<(Arc<str>, Arc<str>)>,
        merge_conflict: Option<(Arc<str>, Arc<str>)>,
        non_branch_rev_tracking_attempt: Option<NonBranchRevTrackingAttempt>,
        new_branch_rev_mismatch: Option<NewBranchRevMismatch>,
        new_branch_already_exists: Option<NewBranchAlreadyExists>,
    }

    let repo_entries = workspace_members
        .iter()
        .flat_map(|(url, member_list)| {
            member_list
                .iter()
                .map(move |member| (url.clone(), member.path.clone(), member.rev.clone()))
        })
        .collect::<Vec<_>>();

    top_progress
        .set_message(format!("checking {} repositories in parallel", repo_entries.len()).as_str());

    let pre_eval_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(MAX_REPO_SYNC_PARALLEL_JOBS)
        .build()
        .map_err(|error| format_error!("failed to create pre-evaluation worker pool: {error}"))?;

    let repo_results = pre_eval_pool.install(|| {
        repo_entries
            .par_iter()
            .map(
            |(url, member_path, member_rev)| -> anyhow::Result<Option<RepoPlanEvaluation>> {
                let member_git_path = std::path::Path::new(member_path.as_ref()).join(".git");
                if !member_git_path.exists() {
                    return Ok(None);
                }

                let mut repo_progress = console::Progress::new(
                    console.clone(),
                    format!("//{}", member_path),
                    None,
                    Some(format!(
                        "//{} planning sync pre-evaluation actions",
                        member_path
                    )),
                );

                repo_progress.set_message("checking rev type");
                let repo = git::Repository::new(url.clone(), member_path.clone());
                let is_dev_branch = dev_branch_paths.contains(member_path);
                let is_rev_branch = repo.is_branch(&mut repo_progress, member_rev);

                let is_on_branch = repo
                    .get_current_branch(&mut repo_progress)
                    .context(format_context!(
                        "//{} while checking current branch",
                        member_path
                    ))?
                    .is_some();

                let detached_head_rev = if !is_on_branch {
                    repo.get_commit_tag(&mut repo_progress)
                        .or_else(|| repo.get_commit_hash(&mut repo_progress).ok().flatten())
                } else {
                    None
                };

                let is_new_branch_repo = new_branch_repos.contains(member_path);
                let is_requested_dev_branch_repo =
                    is_member_dev_branch(member_path.as_ref(), &requested_dev_branch_selectors);
                let explicit_base_ref =
                    resolve_explicit_dev_branch_base(member_path.as_ref(), &dev_branch_base_map);
                let has_non_branch_rev_tracking_attempt = !is_rev_branch
                    && explicit_base_ref.is_none()
                    && (is_requested_dev_branch_repo || is_new_branch_repo);

                let mut action = None;
                let mut skip_reason: Option<Arc<str>> = None;
                let mut create_new_branch = None;
                let mut non_branch_rev_tracking_attempt = None;
                let mut new_branch_rev_mismatch = None;
                let mut new_branch_already_exists_result = None;
                let mut rebase_conflict = None;
                let mut merge_conflict = None;

                if has_non_branch_rev_tracking_attempt {
                    non_branch_rev_tracking_attempt = Some((
                        member_path.clone(),
                        member_rev.clone(),
                        is_requested_dev_branch_repo,
                        is_new_branch_repo,
                    ));
                    skip_reason = Some(
                        "workspace rev is not a branch and no --dev-branch-base is configured"
                            .into(),
                    );
                }

                if is_new_branch_repo {
                    repo_progress.set_message("validating --new-branch state");

                    if !has_non_branch_rev_tracking_attempt {
                        let head_commit = repo
                            .resolve_ref_to_commit(&mut repo_progress, "HEAD")?
                            .ok_or_else(|| {
                                format_error!(
                                    "//{} unable to resolve HEAD commit while validating `--new-branch`",
                                    member_path
                                )
                            })?;

                        let resolved_workspace_rev = repo
                            .resolve_revision(&mut repo_progress, member_rev)
                            .context(format_context!(
                                "//{} while resolving workspace rev `{}` for `--new-branch`",
                                member_path,
                                member_rev
                            ))?;

                        let target_commit = repo
                            .resolve_ref_to_commit(
                                &mut repo_progress,
                                resolved_workspace_rev.commit.as_ref(),
                            )?
                            .ok_or_else(|| {
                                format_error!(
                                    "//{} unable to resolve workspace rev `{}` while validating `--new-branch`",
                                    member_path,
                                    member_rev
                                )
                            })?;

                        if head_commit != target_commit {
                            new_branch_rev_mismatch = Some((
                                member_path.clone(),
                                member_rev.clone(),
                                head_commit,
                                target_commit,
                            ));
                            skip_reason = Some("not on workspace rev".into());
                        } else if repo.local_branch_exists(
                            &mut repo_progress,
                            workspace_new_branch_name.as_ref(),
                        )? {
                            new_branch_already_exists_result = Some((
                                member_path.clone(),
                                workspace_new_branch_name.clone(),
                            ));
                            skip_reason = Some("new branch already exists".into());
                        } else {
                            create_new_branch = Some(workspace_new_branch_name.clone());
                        }
                    }
                } else if is_dev_branch {
                    if no_rebase_repos.contains(member_path) {
                        action = Some(DevBranchAction::Skip);
                        skip_reason = Some("--no-rebase-repo".into());
                    } else if merge_repos.contains(member_path) {
                        action = Some(DevBranchAction::Merge);
                    } else if no_rebase_global {
                        action = Some(DevBranchAction::Skip);
                        skip_reason = Some("--no-rebase".into());
                    } else {
                        action = Some(DevBranchAction::Rebase);
                    }
                }

                let rev_not_branch_mismatch = !is_requested_dev_branch_repo
                    && !is_new_branch_repo
                    && has_rev_not_branch_mismatch(
                        is_rev_branch,
                        is_on_branch,
                        explicit_base_ref.is_some(),
                    );
                if rev_not_branch_mismatch {
                    skip_reason = Some("rev is not a branch".into());
                }

                let pull_from = if create_new_branch.is_some() {
                    None
                } else {
                    resolve_pull_from(is_dev_branch, is_on_branch, is_rev_branch, member_rev)
                };
                let mut rebase_from = None;
                let mut merge_from = None;

                if create_new_branch.is_none()
                    && !has_non_branch_rev_tracking_attempt
                    && matches!(
                        action,
                        Some(DevBranchAction::Rebase) | Some(DevBranchAction::Merge)
                    )
                {
                    if !is_on_branch {
                        skip_reason = Some("detached HEAD".into());
                    } else if rev_not_branch_mismatch {
                        skip_reason = Some("rev is not a branch".into());
                    } else {
                        let effective_base_ref = explicit_base_ref
                            .clone()
                            .unwrap_or_else(|| format!("origin/{}", member_rev).into());
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
                        .expect("sync pre-evaluation action base ref should be present");

                    repo_progress.set_message("fetching latest");
                    repo.fetch_with_prune(&mut repo_progress, git::IgnoreSubmodules::Yes)
                        .context(format_context!(
                            "//{} while fetching updates before sync pre-evaluation planning",
                            member_path
                        ))?;

                    if !repo.base_ref_exists(&mut repo_progress, effective_base_ref.as_ref())? {
                        if explicit_base_ref.is_some() {
                            return Err(format_error!(
                                "//{} explicit base ref `{}` from `--dev-branch-base` was not found after fetch",
                                member_path,
                                effective_base_ref
                            ));
                        }

                        rebase_from = None;
                        merge_from = None;
                        skip_reason = Some(
                            format!(
                                "default base `{}` not found; use `--dev-branch-base={}=<ref>`",
                                effective_base_ref, member_path
                            )
                            .into(),
                        );
                    } else if rebase_from.is_some() {
                        repo_progress.set_message("checking for rebase conflicts");
                        match repo
                            .can_rebase_without_conflicts(
                                &mut repo_progress,
                                effective_base_ref.as_ref(),
                            )
                            .context(format_context!(
                                "//{} while checking rebase conflicts",
                                member_path
                            ))? {
                            true => {}
                            false => {
                                rebase_conflict = Some((
                                    member_path.clone(),
                                    effective_base_ref.clone(),
                                ));
                            }
                        }
                    } else if merge_from.is_some() {
                        repo_progress.set_message("checking for merge conflicts");
                        match repo
                            .can_merge_without_conflicts(
                                &mut repo_progress,
                                effective_base_ref.as_ref(),
                            )
                            .context(format_context!(
                                "//{} while checking merge conflicts",
                                member_path
                            ))? {
                            true => {}
                            false => {
                                merge_conflict = Some((
                                    member_path.clone(),
                                    effective_base_ref.clone(),
                                ));
                            }
                        }
                    }
                }

                repo_progress.set_message("checking if dirty");
                let is_dirty = repo.is_dirty(&mut repo_progress, git::IgnoreSubmodules::Yes);
                let mut will_stash = false;
                let mut dev_branch_dirty_result = None;
                let mut branch_dirty_result = None;
                let mut detached_dirty_result = None;

                let requires_update =
                    pull_from.is_some() || rebase_from.is_some() || merge_from.is_some();
                if is_dirty && requires_update {
                    if use_stash {
                        will_stash = true;
                    } else if is_dev_branch {
                        dev_branch_dirty_result = Some(member_path.clone());
                    } else if is_on_branch {
                        branch_dirty_result = Some(member_path.clone());
                    } else {
                        detached_dirty_result = Some(member_path.clone());
                    }
                }

                let new_branch_tracking_base = if create_new_branch.is_some() && !is_rev_branch {
                    explicit_base_ref.clone()
                } else {
                    None
                };

                let finalize_message = if let Some(base_ref) = rebase_from.as_ref() {
                    format!("//{} ready for rebase onto {}", member_path, base_ref)
                } else if let Some(base_ref) = merge_from.as_ref() {
                    format!("//{} ready for merge from {}", member_path, base_ref)
                } else if let Some(branch_name) = create_new_branch.as_ref() {
                    if let Some(base_ref) = new_branch_tracking_base.as_ref() {
                        format!(
                            "//{} ready to create dev branch `{}`\n  from {}\n  track {} via --dev-branch-base",
                            member_path, branch_name, member_rev, base_ref
                        )
                    } else {
                        format!(
                            "//{} ready to create dev branch `{}`\n  from {}",
                            member_path, branch_name, member_rev
                        )
                    }
                } else if let Some(base_ref) = pull_from.as_ref() {
                    format!("//{} ready for pull from {}", member_path, base_ref)
                } else if let Some(reason) = skip_reason.as_ref() {
                    format!("//{} no pre-eval action ({reason})", member_path)
                } else if !is_rev_branch {
                    format!("//{} no pre-eval action (rev is not a branch)", member_path)
                } else if !is_on_branch {
                    if let Some(detached_rev) = detached_head_rev.as_ref() {
                        format!(
                            "//{} no pre-eval action (detached HEAD at `{detached_rev}`)",
                            member_path
                        )
                    } else {
                        format!("//{} no pre-eval action (detached HEAD)", member_path)
                    }
                } else {
                    format!("//{} no pre-eval action", member_path)
                };

                let is_new_branch_blocked = is_new_branch_repo && create_new_branch.is_none();

                let (finalize_type, finalize_message) = if has_non_branch_rev_tracking_attempt {
                    let attempted_flags = if is_requested_dev_branch_repo && is_new_branch_repo {
                        "--dev-branch/--new-branch"
                    } else if is_new_branch_repo {
                        "--new-branch"
                    } else {
                        "--dev-branch"
                    };
                    (
                        console::FinalType::Failed,
                        format!(
                            "//{} cannot evaluate ({attempted_flags} requires a branch rev or --dev-branch-base)",
                            member_path
                        ),
                    )
                } else if rev_not_branch_mismatch {
                    (
                        console::FinalType::Failed,
                        format!("//{} cannot evaluate (rev is not a branch)", member_path),
                    )
                } else if is_new_branch_blocked {
                    (
                        console::FinalType::Failed,
                        format!("//{} cannot create new branch", member_path),
                    )
                } else if is_dirty && requires_update && !use_stash {
                    (
                        console::FinalType::Failed,
                        format!("//{} cannot evaluate", member_path),
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

                Ok(Some(RepoPlanEvaluation {
                    plan: RepoSyncPlan {
                        path: member_path.clone(),
                        url: url.clone(),
                        is_dev_branch,
                        is_rev_branch,
                        rev: member_rev.clone(),
                        pull_from,
                        rebase_from,
                        merge_from,
                        create_new_branch,
                        new_branch_tracking_base,
                        will_stash,
                    },
                    dev_branch_dirty: dev_branch_dirty_result,
                    branch_dirty: branch_dirty_result,
                    detached_dirty: detached_dirty_result,
                    rev_not_branch_but_on_branch: rev_not_branch_mismatch
                        .then(|| (member_path.clone(), member_rev.clone())),
                    rebase_conflict,
                    merge_conflict,
                    non_branch_rev_tracking_attempt,
                    new_branch_rev_mismatch,
                    new_branch_already_exists: new_branch_already_exists_result,
                }))
            },
            )
            .collect::<Vec<_>>()
    });

    let mut plans = Vec::new();

    for repo_result in repo_results {
        let Some(repo_evaluation) = repo_result? else {
            continue;
        };

        let RepoPlanEvaluation {
            plan,
            dev_branch_dirty: dev_branch_dirty_result,
            branch_dirty: branch_dirty_result,
            detached_dirty: detached_dirty_result,
            rev_not_branch_but_on_branch: rev_not_branch_but_on_branch_result,
            rebase_conflict: rebase_conflict_result,
            merge_conflict: merge_conflict_result,
            non_branch_rev_tracking_attempt: non_branch_rev_tracking_attempt_result,
            new_branch_rev_mismatch: new_branch_rev_mismatch_result,
            new_branch_already_exists: new_branch_already_exists_result,
        } = repo_evaluation;

        if let Some(path) = dev_branch_dirty_result {
            dev_branch_dirty.push(path);
        }
        if let Some(path) = branch_dirty_result {
            branch_dirty.push(path);
        }
        if let Some(path) = detached_dirty_result {
            detached_dirty.push(path);
        }
        if let Some(item) = rev_not_branch_but_on_branch_result {
            rev_not_branch_but_on_branch.push(item);
        }
        if let Some(item) = rebase_conflict_result {
            rebase_conflicts.push(item);
        }
        if let Some(item) = merge_conflict_result {
            merge_conflicts.push(item);
        }
        if let Some(item) = non_branch_rev_tracking_attempt_result {
            non_branch_rev_tracking_attempts.push(item);
        }
        if let Some(item) = new_branch_rev_mismatch_result {
            new_branch_rev_mismatches.push(item);
        }
        if let Some(item) = new_branch_already_exists_result {
            new_branch_already_exists.push(item);
        }

        plans.push(plan);
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

    if !non_branch_rev_tracking_attempts.is_empty() {
        singleton::set_is_error_already_reported();

        non_branch_rev_tracking_attempts
            .sort_by(|(repo_a, _, _, _), (repo_b, _, _, _)| repo_a.cmp(repo_b));
        for (repo, rev, is_dev_branch_request, is_new_branch_request) in
            &non_branch_rev_tracking_attempts
        {
            let attempted_flags = if *is_dev_branch_request && *is_new_branch_request {
                "--dev-branch/--new-branch"
            } else if *is_new_branch_request {
                "--new-branch"
            } else {
                "--dev-branch"
            };
            problems.add_item(
                format!("//{repo}"),
                format!(
                    "{attempted_flags} cannot track workspace rev `{rev}` because it is not a branch\nset `--dev-branch-base=//{repo}=<remote-branch>` or use a branch rev."
                ),
            );
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

    if !new_branch_rev_mismatches.is_empty() || !new_branch_already_exists.is_empty() {
        singleton::set_is_error_already_reported();

        new_branch_rev_mismatches
            .sort_by(|(repo_a, _, _, _), (repo_b, _, _, _)| repo_a.cmp(repo_b));
        for (repo, rev, current_commit, target_commit) in &new_branch_rev_mismatches {
            let current_short = current_commit.chars().take(8).collect::<String>();
            let target_short = target_commit.chars().take(8).collect::<String>();
            problems.add_item(
                format!("//{repo}"),
                format!(
                    "cannot create new branch: expected HEAD at `{rev}` ({target_short}), found {current_short}"
                ),
            );
        }

        new_branch_already_exists.sort_by(|(repo_a, _), (repo_b, _)| repo_a.cmp(repo_b));
        for (repo, branch_name) in &new_branch_already_exists {
            problems.add_item(
                format!("//{repo}"),
                format!("cannot create new branch: local branch `{branch_name}` already exists"),
            );
        }
    }

    if dirty_repo_count > 0
        || !rebase_conflicts.is_empty()
        || !merge_conflicts.is_empty()
        || !non_branch_rev_tracking_attempts.is_empty()
        || !rev_not_branch_but_on_branch.is_empty()
        || !new_branch_rev_mismatches.is_empty()
        || !new_branch_already_exists.is_empty()
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

        for (repo, _, is_dev_branch_request, is_new_branch_request) in
            &non_branch_rev_tracking_attempts
        {
            let branch_flag = if *is_new_branch_request && !*is_dev_branch_request {
                "new-branch"
            } else {
                "dev-branch"
            };

            recommended_actions
                .push(console::bootstrap::plain_text(format!("For //{repo}")).into_line());
            recommended_actions.push(
                console::bootstrap::plain_text(
                    "  Use a branch rev, or provide an explicit dev-branch base:",
                )
                .into_line(),
            );
            recommended_actions.extend(recommend_dev_branch(branch_flag, repo));
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

            recommended_actions.extend(recommend_dev_branch("dev-branch", repo));
        }

        for (repo, rev, _, _) in &new_branch_rev_mismatches {
            recommended_actions
                .push(console::bootstrap::plain_text(format!("For //{repo}")).into_line());
            recommended_actions.push(
                console::bootstrap::plain_text("  Checkout the workspace `rev`, then retry:")
                    .into_line(),
            );
            recommended_actions.push(
                console::bootstrap::code(format!("    git -C {repo} checkout {rev}")).into_line(),
            );
            recommended_actions.push(
                console::bootstrap::code(format!("    spaces sync --new-branch=//{repo}"))
                    .into_line(),
            );
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
            "Cannot evaluate: {} repositories need to be resolved manually",
            dirty_repo_count
                + rebase_conflicts.len()
                + merge_conflicts.len()
                + non_branch_rev_tracking_attempts.len()
                + rev_not_branch_but_on_branch.len()
                + new_branch_rev_mismatches.len()
                + new_branch_already_exists.len()
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

    if plan.rebase_from.is_some() || plan.merge_from.is_some() || plan.create_new_branch.is_some() {
        return None;
    }

    describe_pre_sync_action(plan)
}

fn pre_post_actions(plan: Option<&RepoSyncPlan>) -> Vec<String> {
    let mut actions = Vec::new();

    if let Some(plan) = plan {
        if let Some(action) = describe_pre_sync_action_for_complete_summary(plan) {
            actions.push(format!("pre-eval: {action}"));
        }

        if let Some(action) = describe_post_sync_action(plan) {
            actions.push(format!("post-eval: {action}"));
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
        } else if let Some(branch_name) = plan.create_new_branch.as_ref() {
            if let Some(base_ref) = plan.new_branch_tracking_base.as_ref() {
                format!(
                    "created dev branch `{branch_name}` from {} tracking {}",
                    plan.rev, base_ref
                )
            } else {
                format!("created dev branch `{branch_name}` from {}", plan.rev)
            }
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
        summary.push('\n');
        summary.push_str(actions.join("\n").as_str());
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

    let mut progress = console::Progress::new(console.clone(), progress_name, None, None);
    progress.set_message("capturing repositories in parallel");

    let snapshot_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(MAX_REPO_SYNC_PARALLEL_JOBS)
        .build()
        .map_err(|error| format_error!("failed to create snapshot worker pool: {error}"))?;

    let mut snapshots = snapshot_pool.install(|| {
        workspace_members
            .par_iter()
            .flat_map(|(url, member_list)| {
                member_list.par_iter().filter_map(|member| {
                    let member_git_path = std::path::Path::new(member.path.as_ref()).join(".git");
                    if !member_git_path.exists() {
                        return None;
                    }

                    let mut repo_progress = console::Progress::new(
                        console.clone(),
                        format!("//{}", member.path),
                        None,
                        None,
                    );
                    repo_progress.set_message(format!("capturing //{}", member.path).as_str());

                    let repo = git::Repository::new(url.clone(), member.path.clone());
                    let is_dev_branch =
                        is_member_dev_branch(member.path.as_ref(), &dev_branch_rules);
                    let is_rev_branch = repo.is_branch(&mut repo_progress, &member.rev);
                    let current_branch = repo.get_current_branch(&mut repo_progress).ok().flatten();
                    let current_tag = repo.get_commit_tag(&mut repo_progress);
                    let current_commit_hash =
                        repo.get_commit_hash(&mut repo_progress).ok().flatten();

                    repo_progress.set_finalize_lines(console::make_finalize_line(
                        console::FinalType::Completed,
                        progress.elapsed(),
                        &format!("//{} captured repo snapshot", member.path),
                    ));

                    Some(RepoSyncSnapshot {
                        path: member.path.clone(),
                        is_dev_branch,
                        is_rev_branch,
                        workspace_rev: member.rev.clone(),
                        current_branch,
                        current_tag,
                        current_commit_hash,
                    })
                })
            })
            .collect::<Vec<_>>()
    });

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
            let after = after_by_path.get(&before.path).copied();
            let plan = plan_by_path.get(&before.path).copied();

            if let Some(after) = after {
                let label = format_repo_label_from_kind(
                    before.path.as_ref(),
                    before.is_dev_branch,
                    before.is_rev_branch,
                );
                let summary = describe_sync_complete_result(before, after, plan);
                (label, summary)
            } else {
                (
                    format!("//{} [removed]", before.path),
                    "removed from checkout rules".to_string(),
                )
            }
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

fn resolve_removed_tracked_repo_paths(
    before_tracked_repos: &HashMap<Arc<str>, ws::CheckoutRepo>,
    after_tracked_repos: &HashMap<Arc<str>, ws::CheckoutRepo>,
) -> Vec<Arc<str>> {
    let mut removed_paths = before_tracked_repos
        .keys()
        .filter(|path| !after_tracked_repos.contains_key(*path))
        .cloned()
        .collect::<Vec<_>>();
    removed_paths.sort();
    removed_paths
}

fn is_safe_repo_cleanup_path(path: &str) -> bool {
    let repo_path = std::path::Path::new(path);
    if path.is_empty() || repo_path.is_absolute() {
        return false;
    }

    let mut has_normal_component = false;
    for component in repo_path.components() {
        match component {
            std::path::Component::Normal(_) => has_normal_component = true,
            _ => return false,
        }
    }

    has_normal_component
}

fn is_git_repo_root_at_path(
    progress: &mut console::Progress,
    repo: &git::Repository,
    path: &str,
) -> anyhow::Result<bool> {
    let git_dir_path = std::path::Path::new(path).join(".git");
    if !git_dir_path.exists() || !git_dir_path.is_dir() {
        return Ok(false);
    }

    let show_toplevel = git::execute_git_command(
        progress,
        repo.url.as_ref(),
        console::ExecuteOptions {
            working_directory: Some(path.into()),
            arguments: vec!["rev-parse".into(), "--show-toplevel".into()],
            is_return_stdout: true,
            ..Default::default()
        },
    )?;

    let Some(toplevel) = show_toplevel else {
        return Ok(false);
    };

    let toplevel = toplevel.trim();
    if toplevel.is_empty() {
        return Ok(false);
    }

    let expected_root = std::fs::canonicalize(std::path::Path::new(path));
    let resolved_root = std::fs::canonicalize(std::path::Path::new(toplevel));

    match (expected_root, resolved_root) {
        (Ok(expected), Ok(resolved)) => Ok(expected == resolved),
        _ => Ok(false),
    }
}

fn has_local_commits_not_on_remotes(
    progress: &mut console::Progress,
    repo: &git::Repository,
) -> bool {
    let output = git::execute_git_command(
        progress,
        repo.url.as_ref(),
        console::ExecuteOptions {
            working_directory: Some(repo.full_path.clone()),
            arguments: vec![
                "rev-list".into(),
                "--count".into(),
                "HEAD".into(),
                "--branches".into(),
                "--not".into(),
                "--remotes".into(),
            ],
            is_return_stdout: true,
            ..Default::default()
        },
    )
    .unwrap_or(None);

    let Some(output) = output else {
        return true;
    };

    output
        .trim()
        .parse::<usize>()
        .map(|count| count > 0)
        .unwrap_or(true)
}

pub fn execute_removed_repo_post_sync_actions(
    console: console::Console,
    before_tracked_repos: &HashMap<Arc<str>, ws::CheckoutRepo>,
    after_tracked_repos: &HashMap<Arc<str>, ws::CheckoutRepo>,
) -> anyhow::Result<()> {
    let removed_repo_paths =
        resolve_removed_tracked_repo_paths(before_tracked_repos, after_tracked_repos);
    if removed_repo_paths.is_empty() {
        return Ok(());
    }

    let mut deleted_repos = Vec::new();
    let mut dirty_repos = Vec::new();
    let mut repos_with_local_commits_not_on_remotes = Vec::new();
    let mut non_rooted_git_repos = Vec::new();
    let mut non_directory_repo_paths = Vec::new();
    let mut unsafe_repo_paths = Vec::new();
    let mut dev_branch_repos = Vec::new();
    let mut failed_deletions: Vec<(Arc<str>, String)> = Vec::new();

    for path in removed_repo_paths {
        let Some(repo_meta) = before_tracked_repos.get(path.as_ref()) else {
            continue;
        };

        if !is_safe_repo_cleanup_path(path.as_ref()) {
            unsafe_repo_paths.push(path.clone());
            continue;
        }

        let repo_path = std::path::Path::new(path.as_ref());
        if !repo_path.exists() {
            continue;
        }

        if repo_meta.is_dev_branch {
            dev_branch_repos.push(path.clone());
            continue;
        }

        let mut repo_progress = console::Progress::new(
            console.clone(),
            format!("//{}", path),
            None,
            Some(format!(
                "//{} evaluating post-eval removed repo actions",
                path
            )),
        );
        repo_progress.set_message("checking if repository is clean");

        if !repo_path.is_dir() {
            non_directory_repo_paths.push(path.clone());
            repo_progress.set_finalize_lines(console::make_finalize_line(
                console::FinalType::NotRequired,
                repo_progress.elapsed(),
                &format!(
                    "//{} kept (path is not a directory; cannot remove as repository)",
                    path
                ),
            ));
            continue;
        }

        let repo = git::Repository::new(repo_meta.url.clone(), path.clone());
        if !is_git_repo_root_at_path(&mut repo_progress, &repo, path.as_ref())? {
            non_rooted_git_repos.push(path.clone());
            repo_progress.set_finalize_lines(console::make_finalize_line(
                console::FinalType::NotRequired,
                repo_progress.elapsed(),
                &format!(
                    "//{} kept (.git root mismatch; not deleting to avoid ancestral git repo)",
                    path
                ),
            ));
            continue;
        }

        if repo.is_dirty(&mut repo_progress, git::IgnoreSubmodules::Yes) {
            dirty_repos.push(path.clone());
            repo_progress.set_finalize_lines(console::make_finalize_line(
                console::FinalType::NotRequired,
                repo_progress.elapsed(),
                &format!("//{} kept (dirty repository)", path),
            ));
            continue;
        }

        repo_progress.set_message("checking for local commits not present on remotes");
        if has_local_commits_not_on_remotes(&mut repo_progress, &repo) {
            repos_with_local_commits_not_on_remotes.push(path.clone());
            repo_progress.set_finalize_lines(console::make_finalize_line(
                console::FinalType::NotRequired,
                repo_progress.elapsed(),
                &format!("//{} kept (has local commits not present on remotes)", path),
            ));
            continue;
        }

        repo_progress.set_message("deleting removed repository");
        let remove_result = std::fs::remove_dir_all(repo_path);

        match remove_result {
            Ok(()) => {
                deleted_repos.push(path.clone());
                repo_progress.set_finalize_lines(console::make_finalize_line(
                    console::FinalType::Completed,
                    repo_progress.elapsed(),
                    &format!("//{} deleted (removed from checkout rules)", path),
                ));
            }
            Err(error) => {
                failed_deletions.push((path.clone(), error.to_string()));
                repo_progress.set_finalize_lines(console::make_finalize_line(
                    console::FinalType::Failed,
                    repo_progress.elapsed(),
                    &format!("//{} failed to delete", path),
                ));
            }
        }
    }

    if deleted_repos.is_empty()
        && dirty_repos.is_empty()
        && repos_with_local_commits_not_on_remotes.is_empty()
        && non_rooted_git_repos.is_empty()
        && non_directory_repo_paths.is_empty()
        && unsafe_repo_paths.is_empty()
        && dev_branch_repos.is_empty()
        && failed_deletions.is_empty()
    {
        return Ok(());
    }

    let mut container = console::container::Container::new();
    container.add(console::bootstrap::VerticalSpacer::new(1));
    container.add(
        console::bootstrap::Banner::new("Removed Repository Cleanup".to_string())
            .variant(console::bootstrap::Variant::Warning)
            .width(console::bootstrap::Width::Large),
    );

    let mut status_list = console::components::DescriptionList::new()
        .compact(true)
        .variant(console::components::Variant::Primary);

    for path in &deleted_repos {
        status_list.add_item(
            format!("//{path}"),
            "deleted (clean, synced with remotes, and no longer in checkout rules)",
        );
    }

    for path in &dirty_repos {
        status_list.add_item(
            format!("//{path}"),
            "kept (dirty repository; manual cleanup required)",
        );
    }

    for path in &repos_with_local_commits_not_on_remotes {
        status_list.add_item(
            format!("//{path}"),
            "kept (has local commits not present on remotes; manual cleanup required)",
        );
    }

    for path in &non_rooted_git_repos {
        status_list.add_item(
            format!("//{path}"),
            "kept (.git root mismatch; git may be resolving an ancestor repository)",
        );
    }

    for path in &non_directory_repo_paths {
        status_list.add_item(
            format!("//{path}"),
            "kept (path is not a directory; cannot remove as repository)",
        );
    }

    for path in &unsafe_repo_paths {
        status_list.add_item(
            path.to_string(),
            "kept (unsafe path in checkout settings; expected non-empty relative path without `.` or `..`)",
        );
    }

    for path in &dev_branch_repos {
        status_list.add_item(
            format!("//{path}"),
            "kept (dev-branch repository; manual cleanup recommended)",
        );
    }

    for (path, error) in &failed_deletions {
        status_list.add_item(format!("//{path}"), format!("failed to delete: {error}"));
    }

    container.add(status_list);

    if !dev_branch_repos.is_empty() {
        let mut recommendation_lines = Vec::new();
        for path in &dev_branch_repos {
            recommendation_lines.push(
                console::bootstrap::plain_text(format!(
                    "//{path} is a dev-branch and was removed from checkout rules. Delete it manually when ready:"
                ))
                .into_line(),
            );
            recommendation_lines
                .push(console::bootstrap::code(format!("  rm -rf {path}")).into_line());
            recommendation_lines.extend(console::bootstrap::VerticalSpacer::new(1).render());
        }

        container.add(
            console::components::Alert::new(recommendation_lines)
                .title("Recommended Actions")
                .width(console::bootstrap::Width::Large),
        );
    }

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
        "Evaluation Dry Run Results",
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

    let sync_entries = if singleton::get_sync_options().skip_evaluation {
        Vec::new()
    } else {
        sorted_plans
            .iter()
            .filter_map(|plan| {
                describe_sync_action(plan).map(|action| (format_repo_label(plan), action))
            })
            .collect::<Vec<_>>()
    };

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

struct PreSyncExecutionOutcome {
    path: Arc<str>,
    stashed: bool,
}

struct PreSyncExecutionFailure {
    path: Arc<str>,
    stashed: bool,
    error_message: String,
}

fn execute_pre_sync_actions_for_repo(
    console: console::Console,
    plan: RepoSyncPlan,
) -> Result<PreSyncExecutionOutcome, PreSyncExecutionFailure> {
    let mut repo_progress = console::Progress::new(
        console.clone(),
        format!("//{}", plan.path),
        None,
        Some(format!(
            "//{} applying sync pre-evaluation actions",
            plan.path
        )),
    );

    let repo = git::Repository::new(plan.url.clone(), plan.path.clone());

    let mut stashed = false;

    if plan.will_stash {
        if let Err(error) = repo.stash(&mut repo_progress) {
            repo_progress.set_finalize_lines(console::make_finalize_line(
                console::FinalType::Failed,
                repo_progress.elapsed(),
                &format!("//{} failed to stash changes", plan.path),
            ));
            return Err(PreSyncExecutionFailure {
                path: plan.path.clone(),
                stashed,
                error_message: format!("//{} failed to stash changes: {error}", plan.path),
            });
        }

        stashed = true;
    }

    if let Some(base_ref) = plan.rebase_from.as_ref()
        && let Err(error) = repo.rebase_onto(&mut repo_progress, base_ref.as_ref())
    {
        repo_progress.set_finalize_lines(console::make_finalize_line(
            console::FinalType::Failed,
            repo_progress.elapsed(),
            &format!("//{} rebase failed", plan.path),
        ));

        return Err(PreSyncExecutionFailure {
            path: plan.path.clone(),
            stashed,
            error_message: format!(
                "//{} failed to rebase onto {}: {error}",
                plan.path, base_ref
            ),
        });
    }

    if let Some(base_ref) = plan.merge_from.as_ref()
        && let Err(error) = repo.merge_from(&mut repo_progress, base_ref.as_ref())
    {
        repo_progress.set_finalize_lines(console::make_finalize_line(
            console::FinalType::Failed,
            repo_progress.elapsed(),
            &format!("//{} merge failed", plan.path),
        ));

        return Err(PreSyncExecutionFailure {
            path: plan.path.clone(),
            stashed,
            error_message: format!("//{} failed to merge {}: {error}", plan.path, base_ref),
        });
    }

    if let Some(branch_name) = plan.create_new_branch.as_ref()
        && let Err(error) = repo.execute(
            &mut repo_progress,
            vec!["switch".into(), "-c".into(), branch_name.clone()],
        )
    {
        repo_progress.set_finalize_lines(console::make_finalize_line(
            console::FinalType::Failed,
            repo_progress.elapsed(),
            &format!("//{} new branch creation failed", plan.path),
        ));

        return Err(PreSyncExecutionFailure {
            path: plan.path.clone(),
            stashed,
            error_message: format!(
                "//{} failed to create new branch `{}`: {error}",
                plan.path, branch_name,
            ),
        });
    }

    let final_message = if plan.will_stash {
        if let Some(base_ref) = plan.rebase_from.as_ref() {
            format!(
                "//{} stashed changes and rebased onto {}",
                plan.path, base_ref
            )
        } else if let Some(base_ref) = plan.merge_from.as_ref() {
            format!("//{} stashed changes and merged {}", plan.path, base_ref)
        } else {
            format!("//{} stashed uncommitted changes", plan.path)
        }
    } else if let Some(base_ref) = plan.rebase_from.as_ref() {
        format!("//{} rebased successfully on {}", plan.path, base_ref)
    } else if let Some(base_ref) = plan.merge_from.as_ref() {
        format!("//{} merged successfully from {}", plan.path, base_ref)
    } else if let Some(branch_name) = plan.create_new_branch.as_ref() {
        if let Some(base_ref) = plan.new_branch_tracking_base.as_ref() {
            format!(
                "//{} created new dev branch `{}` from {} tracking {}",
                plan.path, branch_name, plan.rev, base_ref,
            )
        } else {
            format!(
                "//{} created new dev branch `{}` from {}",
                plan.path, branch_name, plan.rev,
            )
        }
    } else {
        format!("//{} sync pre-evaluation actions complete", plan.path)
    };

    repo_progress.set_finalize_lines(console::make_finalize_line(
        console::FinalType::Completed,
        repo_progress.elapsed(),
        &final_message,
    ));

    Ok(PreSyncExecutionOutcome {
        path: plan.path,
        stashed,
    })
}

fn collect_pre_sync_job_result(
    path: Arc<str>,
    may_have_stashed: bool,
    result: std::thread::Result<Result<PreSyncExecutionOutcome, PreSyncExecutionFailure>>,
    stashed_repos: &mut Vec<Arc<str>>,
    errors: &mut Vec<String>,
) {
    match result {
        Ok(Ok(outcome)) => {
            if outcome.stashed {
                stashed_repos.push(outcome.path);
            }
        }
        Ok(Err(error)) => {
            if error.stashed {
                stashed_repos.push(error.path);
            }
            errors.push(error.error_message);
        }
        Err(_) => {
            if may_have_stashed {
                stashed_repos.push(path.clone());
            }
            errors.push(format!("//{} pre-eval worker thread panicked", path));
        }
    }
}

pub fn execute_repo_sync_plan(
    console: console::Console,
    workspace_arc: workspace::WorkspaceArc,
    plans: &[RepoSyncPlan],
) -> anyhow::Result<Vec<Arc<str>>> {
    let actionable_plans = plans
        .iter()
        .filter(|plan| {
            plan.will_stash
                || plan.rebase_from.is_some()
                || plan.merge_from.is_some()
                || plan.create_new_branch.is_some()
        })
        .cloned()
        .collect::<Vec<_>>();

    let mut stashed_repos = Vec::new();
    let mut errors = Vec::new();

    let pre_sync_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(MAX_REPO_SYNC_PARALLEL_JOBS)
        .build()
        .map_err(|error| format_error!("failed to create pre-sync worker pool: {error}"))?;

    let job_results = pre_sync_pool.install(|| {
        actionable_plans
            .into_par_iter()
            .map(|plan| {
                let path = plan.path.clone();
                let may_have_stashed = plan.will_stash;
                let worker_console = console.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    execute_pre_sync_actions_for_repo(worker_console, plan)
                }));
                (path, may_have_stashed, result)
            })
            .collect::<Vec<_>>()
    });

    for (path, may_have_stashed, result) in job_results {
        collect_pre_sync_job_result(
            path,
            may_have_stashed,
            result,
            &mut stashed_repos,
            &mut errors,
        );
    }

    stashed_repos.sort();
    stashed_repos.dedup();

    if !errors.is_empty() {
        errors.sort();
        pop_stashes_after_failed_pre_sync(console, workspace_arc, &stashed_repos)?;

        if errors.len() == 1 {
            return Err(format_error!("{}", errors[0]));
        }

        return Err(format_error!(
            "failed to apply sync pre-evaluation actions on {} repositories:\n{}",
            errors.len(),
            errors.iter().map(|error| format!("- {error}")).join("\n")
        ));
    }

    Ok(stashed_repos)
}

fn pop_stash_for_repo(
    console: console::Console,
    url: Arc<str>,
    path: Arc<str>,
) -> anyhow::Result<()> {
    let mut repo_progress = console::Progress::new(
        console.clone(),
        format!("//{}", path),
        None,
        Some(format!("//{} popping stash", path)),
    );

    let repo = git::Repository::new(url, path.clone());

    match repo.stash_pop(&mut repo_progress) {
        Ok(_) => {
            let lines = console::make_finalize_line(
                console::FinalType::Completed,
                repo_progress.elapsed(),
                &format!("//{} popped stash successfully", path),
            );
            repo_progress.set_finalize_lines(lines);
            Ok(())
        }
        Err(error) => {
            let lines = console::make_finalize_line(
                console::FinalType::Failed,
                None,
                &format!("//{} failed to pop stash", path),
            );
            repo_progress.set_finalize_lines(lines);
            Err(format_error!(
                "//{} {error}. Manually check this repo with 'git status'",
                path
            ))
        }
    }
}

fn collect_stash_pop_job_result(result: anyhow::Result<()>, warnings: &mut Vec<String>) {
    if let Err(err) = result {
        warnings.push(format!("while popping stash because {err:?}"));
    }
}

fn resolve_stash_pop_targets(
    stashed_repos: Vec<Arc<str>>,
    current_repo_urls_by_path: &HashMap<Arc<str>, Arc<str>>,
) -> Vec<(Arc<str>, Arc<str>)> {
    stashed_repos
        .into_iter()
        .map(|path| {
            let url = current_repo_urls_by_path
                .get(&path)
                .cloned()
                .unwrap_or_else(|| path.clone());
            (url, path)
        })
        .collect()
}

/// Pop stashes on repositories that were stashed during sync pre-evaluation checks.
pub fn pop_stashed_repos(
    console: console::Console,
    workspace_arc: workspace::WorkspaceArc,
    stashed_repos: Vec<Arc<str>>,
) -> anyhow::Result<()> {
    if stashed_repos.is_empty() {
        return Ok(());
    }

    let workspace_members = workspace_arc.read().settings.json.members.clone();
    let current_repo_urls_by_path = workspace_members
        .iter()
        .flat_map(|(url, member_list)| {
            member_list
                .iter()
                .map(move |member| (member.path.clone(), url.clone()))
        })
        .collect::<HashMap<_, _>>();

    let repos_to_pop = resolve_stash_pop_targets(stashed_repos, &current_repo_urls_by_path);

    let mut warnings = Vec::new();

    let stash_pop_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(MAX_REPO_SYNC_PARALLEL_JOBS)
        .build()
        .map_err(|error| format_error!("failed to create stash-pop worker pool: {error}"))?;

    let job_results = stash_pop_pool.install(|| {
        repos_to_pop
            .into_par_iter()
            .map(|(url, path)| {
                let worker_console = console.clone();
                pop_stash_for_repo(worker_console, url, path)
            })
            .collect::<Vec<_>>()
    });

    for result in job_results {
        collect_stash_pop_job_result(result, &mut warnings);
    }

    warnings.sort();
    for warning in warnings {
        console.warning("Failed to pop stash", warning)?;
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

    fn checkout_repo(url: &str, is_dev_branch: bool) -> ws::CheckoutRepo {
        ws::CheckoutRepo {
            url: url.into(),
            is_dev_branch,
        }
    }

    fn tracked_repo_map(items: &[(&str, &str, bool)]) -> HashMap<Arc<str>, ws::CheckoutRepo> {
        items
            .iter()
            .map(|(path, url, is_dev_branch)| {
                (Arc::<str>::from(*path), checkout_repo(url, *is_dev_branch))
            })
            .collect()
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
            create_new_branch: None,
            new_branch_tracking_base: None,
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
    fn resolve_removed_tracked_repo_paths_returns_sorted_removed_repos() {
        let before = tracked_repo_map(&[
            ("@star/sdk", "https://example.com/sdk.git", false),
            ("@star/dev", "https://example.com/dev.git", true),
            ("@star/keep", "https://example.com/keep.git", false),
        ]);
        let after = tracked_repo_map(&[("@star/keep", "https://example.com/keep.git", false)]);

        assert_eq!(
            resolve_removed_tracked_repo_paths(&before, &after),
            arcs(&["@star/dev", "@star/sdk"]),
        );
    }

    #[test]
    fn is_safe_repo_cleanup_path_accepts_non_empty_relative_paths() {
        assert!(is_safe_repo_cleanup_path("@star/sdk"));
        assert!(is_safe_repo_cleanup_path("repos/sdk"));
    }

    #[test]
    fn is_safe_repo_cleanup_path_rejects_unsafe_paths() {
        assert!(!is_safe_repo_cleanup_path(""));
        assert!(!is_safe_repo_cleanup_path("."));
        assert!(!is_safe_repo_cleanup_path(".."));
        assert!(!is_safe_repo_cleanup_path("../repo"));
        assert!(!is_safe_repo_cleanup_path("repo/.."));
        assert!(!is_safe_repo_cleanup_path("/tmp/repo"));
    }

    #[test]
    fn resolve_stash_pop_targets_includes_removed_repo_paths() {
        let url_by_path = HashMap::from([(
            Arc::<str>::from("@star/keep"),
            Arc::<str>::from("https://example.com/keep.git"),
        )]);

        let targets =
            resolve_stash_pop_targets(arcs(&["@star/removed", "@star/keep"]), &url_by_path);

        assert_eq!(
            targets,
            vec![
                (
                    Arc::<str>::from("@star/removed"),
                    Arc::<str>::from("@star/removed")
                ),
                (
                    Arc::<str>::from("https://example.com/keep.git"),
                    Arc::<str>::from("@star/keep")
                )
            ]
        );
    }

    #[test]
    fn is_member_dev_branch_accepts_selectors_with_or_without_prefix() {
        assert!(is_member_dev_branch("@star/sdk", &arcs(&["@star/sdk"])));
        assert!(is_member_dev_branch("@star/sdk", &arcs(&["//@star/sdk"])));
        assert!(is_member_dev_branch("@star/sdk", &arcs(&["sdk"])));
    }

    #[test]
    fn resolve_pull_from_skips_non_branch_revs() {
        let pull_from = resolve_pull_from(false, true, false, "deadbeef");

        assert_eq!(pull_from, None);
    }

    #[test]
    fn resolve_explicit_dev_branch_base_uses_exact_match() {
        let base_map = HashMap::from([(
            Arc::<str>::from("@star/sdk"),
            Arc::<str>::from("origin/main"),
        )]);

        assert_eq!(
            resolve_explicit_dev_branch_base("@star/sdk", &base_map).as_deref(),
            Some("origin/main")
        );
    }

    #[test]
    fn resolve_explicit_dev_branch_base_supports_suffix_selectors() {
        let base_map = HashMap::from([(Arc::<str>::from("sdk"), Arc::<str>::from("origin/main"))]);

        assert_eq!(
            resolve_explicit_dev_branch_base("@star/sdk", &base_map).as_deref(),
            Some("origin/main")
        );
    }

    #[test]
    fn resolve_explicit_dev_branch_base_prefers_most_specific_selector() {
        let base_map = HashMap::from([
            (Arc::<str>::from("sdk"), Arc::<str>::from("origin/develop")),
            (
                Arc::<str>::from("@star/sdk"),
                Arc::<str>::from("origin/main"),
            ),
        ]);

        assert_eq!(
            resolve_explicit_dev_branch_base("@star/sdk", &base_map).as_deref(),
            Some("origin/main")
        );
    }

    #[test]
    fn resolve_explicit_dev_branch_base_accepts_selectors_with_prefix() {
        let base_map = HashMap::from([(
            Arc::<str>::from("//@star/sdk"),
            Arc::<str>::from("origin/main"),
        )]);

        assert_eq!(
            resolve_explicit_dev_branch_base("@star/sdk", &base_map).as_deref(),
            Some("origin/main")
        );
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
    fn describe_pre_sync_action_reports_new_branch_creation() {
        let mut plan = repo_sync_plan(false, true, true, None, None, None, false, None);
        plan.create_new_branch = Some("my-workspace".into());

        assert_eq!(
            describe_pre_sync_action(&plan),
            Some("create dev branch `my-workspace`\n   from main".to_string())
        );
    }

    #[test]
    fn describe_pre_sync_action_reports_new_branch_tracking_base_for_non_branch_rev() {
        let mut plan = repo_sync_plan(false, false, true, None, None, None, false, None);
        plan.rev = "v0.4.0".into();
        plan.create_new_branch = Some("my-workspace".into());
        plan.new_branch_tracking_base = Some("origin/main".into());

        assert_eq!(
            describe_pre_sync_action(&plan),
            Some(
                "create dev branch `my-workspace`\n  from v0.4.0\n  track origin/main via --dev-branch-base"
                    .to_string()
            )
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
        assert_eq!(DRY_RUN_PRE_SYNC_HEADER, "Sync Pre-Evaluation (Pre-Eval)");
        assert_eq!(
            DRY_RUN_SYNC_HEADER,
            "Evaluation (May Change Based on Updated Rules)"
        );
        assert_eq!(DRY_RUN_POST_SYNC_HEADER, "Sync Post-Evaluation");
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
            "pulled main 01234567 → fedcba98"
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
            "rebased onto origin/main\npre-eval: stash local changes\npost-eval: pop stashed changes"
        );
    }

    #[test]
    fn describe_sync_complete_result_reports_new_branch_creation() {
        let mut plan = repo_sync_plan(false, true, true, None, None, None, false, None);
        plan.create_new_branch = Some("my-workspace".into());
        let before = repo_sync_snapshot(
            false,
            true,
            "main",
            Some("main"),
            None,
            Some("0123456789abcdef"),
        );
        let after = repo_sync_snapshot(
            true,
            true,
            "main",
            Some("my-workspace"),
            None,
            Some("0123456789abcdef"),
        );

        assert_eq!(
            describe_sync_complete_result(&before, &after, Some(&plan)),
            "created dev branch `my-workspace` from main"
        );
    }

    #[test]
    fn describe_sync_complete_result_reports_new_branch_creation_tracking_base() {
        let mut plan = repo_sync_plan(false, false, true, None, None, None, false, None);
        plan.rev = "v0.4.0".into();
        plan.create_new_branch = Some("my-workspace".into());
        plan.new_branch_tracking_base = Some("origin/main".into());
        let before = repo_sync_snapshot(
            false,
            false,
            "v0.4.0",
            None,
            Some("v0.4.0"),
            Some("0123456789abcdef"),
        );
        let after = repo_sync_snapshot(
            true,
            false,
            "v0.4.0",
            Some("my-workspace"),
            None,
            Some("0123456789abcdef"),
        );

        assert_eq!(
            describe_sync_complete_result(&before, &after, Some(&plan)),
            "created dev branch `my-workspace` from v0.4.0 tracking origin/main"
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
