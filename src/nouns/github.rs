//! GitHub CLI noun implementation.
//!
//! Parses, routes, and formats output for GitHub operations, delegating API logic
//! to `integration::github` and candidate classification to `domain::github`.

use crate::domain::delete::{DeletionPlanAdjudicator, PlanSafetyWitness};
use crate::domain::github::{
    is_branch_merged, is_cache_stale, is_issue_stale, is_pr_stale, is_release_stale_or_draft,
    is_repo_stale_or_empty, is_run_stale, GithubTarget,
};
use crate::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
use crate::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
use crate::integration::github::{
    close_issue, close_pr, compare_branch, delete_branch, delete_cache, delete_release,
    delete_release_asset, delete_repository, delete_run, list_branches, list_caches, list_issues,
    list_prs, list_releases, list_repositories, list_runs, CommandExecutor, RealCommandExecutor,
};
use clap::Subcommand;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use wasm4pm_compat::admission::Admit;
use wasm4pm_compat::evidence::Evidence;
use wasm4pm_compat::state::Raw;

#[derive(Subcommand, Debug)]
pub enum GithubAction {
    /// Scan GitHub for empty/stale repositories, merged branches, stale runs, and draft/stale releases
    #[command(alias = "audit")]
    Scan {
        /// Number of days of inactivity to consider a repository stale
        #[arg(long, default_value = "180")]
        repo_days: i64,
        /// Number of days of age to consider a workflow run stale
        #[arg(long, default_value = "30")]
        run_days: i64,
        /// Number of days of age to consider a release stale
        #[arg(long, default_value = "30")]
        release_days: i64,
        /// Number of days of age to consider a cache stale
        #[arg(long, default_value = "30")]
        cache_days: i64,
        /// Number of days of inactivity to consider an issue stale
        #[arg(long, default_value = "30")]
        issue_days: i64,
        /// Number of days of inactivity to consider a pull request stale
        #[arg(long, default_value = "30")]
        pr_days: i64,
        /// Minimum size of release assets in MB to consider for cleanup
        #[arg(long, default_value = "0")]
        min_asset_size_mb: u64,
    },
    /// Build and write a deletion plan containing identified GitHub candidates
    Plan {
        /// Number of days of inactivity to consider a repository stale
        #[arg(long, default_value = "180")]
        repo_days: i64,
        /// Number of days of age to consider a workflow run stale
        #[arg(long, default_value = "30")]
        run_days: i64,
        /// Number of days of age to consider a release stale
        #[arg(long, default_value = "30")]
        release_days: i64,
        /// Number of days of age to consider a cache stale
        #[arg(long, default_value = "30")]
        cache_days: i64,
        /// Number of days of inactivity to consider an issue stale
        #[arg(long, default_value = "30")]
        issue_days: i64,
        /// Number of days of inactivity to consider a pull request stale
        #[arg(long, default_value = "30")]
        pr_days: i64,
        /// Minimum size of release assets in MB to consider for cleanup
        #[arg(long, default_value = "0")]
        min_asset_size_mb: u64,
        /// Output path for the deletion plan JSON file
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Load a deletion plan, validate it, execute GitHub resource deletions, and write a receipt
    Delete {
        /// Path to the deletion plan
        #[arg(short, long)]
        plan: PathBuf,
        /// Path to write the execution receipt
        #[arg(short, long)]
        receipt: PathBuf,
        /// Skip interactive confirmation
        #[arg(short, long)]
        yes: bool,
    },
    /// Verify and summarize a deletion receipt containing GitHub items
    Receipt {
        /// Path to the deletion receipt
        #[arg(short, long)]
        receipt: PathBuf,
        /// Optional path to the original deletion plan to check completeness
        #[arg(short, long)]
        plan: Option<PathBuf>,
    },
}

/// Scan GitHub for empty/stale repositories, merged branches, stale runs, and draft/stale releases.
///
/// # Arguments
/// * `repo_days` - Number of days of inactivity to consider a repository stale [default: 180]
/// * `run_days` - Number of days of age to consider a workflow run stale [default: 30]
/// * `release_days` - Number of days of age to consider a release stale [default: 30]
/// * `cache_days` - Number of days of age to consider a cache stale [default: 30]
/// * `issue_days` - Number of days of inactivity to consider an issue stale [default: 30]
/// * `pr_days` - Number of days of inactivity to consider a pull request stale [default: 30]
/// * `min_asset_size_mb` - Minimum size of release assets in MB to consider for cleanup [default: 0]
#[clap_noun_verb_macros::verb("scan", "github")]
pub fn github_scan(
    repo_days: Option<i64>,
    run_days: Option<i64>,
    release_days: Option<i64>,
    cache_days: Option<i64>,
    issue_days: Option<i64>,
    pr_days: Option<i64>,
    min_asset_size_mb: Option<u64>,
) -> clap_noun_verb::Result<serde_json::Value> {
    let executor = RealCommandExecutor;
    let candidates = discover_candidates(
        &executor,
        repo_days.unwrap_or(180),
        run_days.unwrap_or(30),
        release_days.unwrap_or(30),
        cache_days.unwrap_or(30),
        issue_days.unwrap_or(30),
        pr_days.unwrap_or(30),
        min_asset_size_mb.unwrap_or(0),
    )
    .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?;

    print_scan_summary(&candidates);

    Ok(serde_json::json!({
        "status": "success",
        "candidates_count": candidates.len(),
    }))
}

/// Build and write a deletion plan containing identified GitHub candidates.
///
/// # Arguments
/// * `repo_days` - Number of days of inactivity to consider a repository stale [default: 180]
/// * `run_days` - Number of days of age to consider a workflow run stale [default: 30]
/// * `release_days` - Number of days of age to consider a release stale [default: 30]
/// * `cache_days` - Number of days of age to consider a cache stale [default: 30]
/// * `issue_days` - Number of days of inactivity to consider an issue stale [default: 30]
/// * `pr_days` - Number of days of inactivity to consider a pull request stale [default: 30]
/// * `min_asset_size_mb` - Minimum size of release assets in MB to consider for cleanup [default: 0]
/// * `output` - Output path for the deletion plan JSON file
#[clap_noun_verb_macros::verb("plan", "github")]
pub fn github_plan(
    repo_days: Option<i64>,
    run_days: Option<i64>,
    release_days: Option<i64>,
    cache_days: Option<i64>,
    issue_days: Option<i64>,
    pr_days: Option<i64>,
    min_asset_size_mb: Option<u64>,
    #[arg(short = 'o')] output: String,
) -> clap_noun_verb::Result<serde_json::Value> {
    let executor = RealCommandExecutor;
    let candidates = discover_candidates(
        &executor,
        repo_days.unwrap_or(180),
        run_days.unwrap_or(30),
        release_days.unwrap_or(30),
        cache_days.unwrap_or(30),
        issue_days.unwrap_or(30),
        pr_days.unwrap_or(30),
        min_asset_size_mb.unwrap_or(0),
    )
    .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?;

    let plan = DeletionPlan::new(
        vec![PathBuf::from("github://")],
        false,
        false,
        candidates.clone(),
        vec![],
    );
    let content = serde_json::to_string_pretty(&plan)
        .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?;
    std::fs::write(&output, content)
        .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?;

    println!("Successfully wrote GitHub deletion plan to {}", output);

    Ok(serde_json::json!({
        "status": "success",
        "candidates_count": candidates.len(),
        "output": output,
    }))
}

/// Load a deletion plan, validate it, execute GitHub resource deletions, and write a receipt.
///
/// # Arguments
/// * `plan` - Path to the deletion plan
/// * `receipt` - Path to write the execution receipt
/// * `yes` - Skip interactive confirmation
#[clap_noun_verb_macros::verb("delete", "github")]
pub fn github_delete(
    #[arg(short = 'p')] plan: String,
    #[arg(short = 'r')] receipt: String,
    #[arg(short = 'y')] yes: bool,
) -> clap_noun_verb::Result<serde_json::Value> {
    let plan_path = PathBuf::from(plan);
    let receipt_path = PathBuf::from(receipt);
    let content = std::fs::read_to_string(&plan_path)
        .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?;
    let plan_item: DeletionPlan = serde_json::from_str(&content)
        .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?;

    execute_delete_plan_helper(&RealCommandExecutor, plan_item, receipt_path, yes)
        .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))
}

pub fn execute_delete_plan_helper(
    executor: &dyn CommandExecutor,
    plan: DeletionPlan,
    receipt_path: PathBuf,
    yes: bool,
) -> anyhow::Result<serde_json::Value> {
    let raw_evidence = Evidence::<_, Raw, PlanSafetyWitness>::raw(plan);
    let admitted_plan = match DeletionPlanAdjudicator::admit(raw_evidence) {
        Ok(admitted) => admitted.into_evidence(),
        Err(refusal) => {
            return Err(anyhow::anyhow!(
                "Plan validation failed: {}",
                refusal.reason
            ))
        }
    };
    let plan = admitted_plan.into_inner();

    println!("Executing GitHub deletions from plan");
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let pb = ProgressBar::new(plan.items.len() as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let mut results = Vec::new();
    for item in &plan.items {
        pb.set_message(format!("Deleting {} ...", item.path.display()));
        let proceed = if yes {
            true
        } else {
            pb.suspend(|| {
                dialoguer::Confirm::new()
                    .with_prompt(format!("Do you want to delete {}?", item.path.display()))
                    .default(false)
                    .interact()
                    .unwrap_or(false)
            })
        };

        let res = if !proceed {
            DeletionResult {
                path: item.path.clone(),
                status: DeletionStatus::Refused,
                error: Some("Deletion refused by user".to_string()),
                blake3_hash: None,
                bytes_freed: 0,
            }
        } else if !is_github_uri(&item.path) {
            DeletionResult {
                path: item.path.clone(),
                status: DeletionStatus::SkippedMissing,
                error: Some("Non-GitHub item skipped during github delete".to_string()),
                blake3_hash: None,
                bytes_freed: 0,
            }
        } else if let Some(target) = GithubTarget::parse(&item.path) {
            match delete_github_target(executor, &target) {
                Ok(()) => {
                    let bytes_freed = match target {
                        GithubTarget::Cache { .. } | GithubTarget::ReleaseAsset { .. } => {
                            item.bytes
                        }
                        _ => 0,
                    };
                    DeletionResult {
                        path: item.path.clone(),
                        status: DeletionStatus::Deleted,
                        error: None,
                        blake3_hash: None,
                        bytes_freed,
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    let status = if err_str.contains("not found") || err_str.contains("404") {
                        DeletionStatus::SkippedMissing
                    } else {
                        DeletionStatus::Failed
                    };
                    DeletionResult {
                        path: item.path.clone(),
                        status,
                        error: Some(err_str),
                        blake3_hash: None,
                        bytes_freed: 0,
                    }
                }
            }
        } else {
            DeletionResult {
                path: item.path.clone(),
                status: DeletionStatus::Failed,
                error: Some("Invalid github:// URI".to_string()),
                blake3_hash: None,
                bytes_freed: 0,
            }
        };
        results.push(res);
        pb.inc(1);
    }
    pb.finish_with_message("GitHub deletion execution complete.");

    let end_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let receipt_obj = DeletionReceipt::new(
        plan.created_unix,
        start_time,
        end_time,
        results.clone(),
        None,
        None,
    );

    let receipt_json = serde_json::to_string_pretty(&receipt_obj)?;
    std::fs::write(&receipt_path, receipt_json)?;
    println!(
        "Successfully wrote execution receipt to {}",
        receipt_path.display()
    );

    let deleted_count = results
        .iter()
        .filter(|r| r.status == DeletionStatus::Deleted)
        .count();
    let failed_count = results
        .iter()
        .filter(|r| r.status == DeletionStatus::Failed)
        .count();
    let skipped_count = results
        .iter()
        .filter(|r| r.status == DeletionStatus::SkippedMissing)
        .count();
    let refused_count = results
        .iter()
        .filter(|r| r.status == DeletionStatus::Refused)
        .count();

    Ok(serde_json::json!({
        "status": "success",
        "receipt_path": receipt_path.to_string_lossy(),
        "deleted_count": deleted_count,
        "failed_count": failed_count,
        "skipped_count": skipped_count,
        "refused_count": refused_count,
    }))
}

/// Verify and summarize a deletion receipt containing GitHub items.
///
/// # Arguments
/// * `receipt` - Path to the deletion receipt
/// * `plan` - Optional path to the original deletion plan to check completeness
#[clap_noun_verb_macros::verb("receipt", "github")]
pub fn github_receipt(
    #[arg(short = 'r')] receipt: String,
    #[arg(short = 'p')] plan: Option<String>,
) -> clap_noun_verb::Result<serde_json::Value> {
    let receipt_path = PathBuf::from(receipt);
    let content = std::fs::read_to_string(&receipt_path)
        .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?;
    let receipt: DeletionReceipt = serde_json::from_str(&content)
        .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?;

    let plan = if let Some(ref p_str) = plan {
        let p_content = std::fs::read_to_string(p_str)
            .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?;
        Some(
            serde_json::from_str::<DeletionPlan>(&p_content)
                .map_err(|e| clap_noun_verb::NounVerbError::execution_error(e.to_string()))?,
        )
    } else {
        None
    };

    let report = receipt.verify(plan.as_ref());
    println!("\n==================================================");
    println!("            RECEIPT VERIFICATION REPORT           ");
    println!("==================================================");
    println!("  Consistent:          {}", report.is_consistent);
    println!("  Total issues found:  {}", report.issues.len());
    println!("==================================================");

    if !report.is_consistent {
        println!("\nIssues:");
        for issue in &report.issues {
            println!(
                "  - [{:?}] {}: {}",
                issue.issue_type,
                issue.path.display(),
                issue.message
            );
        }
        return Err(clap_noun_verb::NounVerbError::execution_error(
            "Receipt verification failed (inconsistent evidence).",
        ));
    } else {
        println!("\nAll checks passed. Receipt matches deletion plan and reality rules.");
    }

    Ok(serde_json::json!({
        "status": "success",
        "verified": true,
        "issues_count": report.issues.len(),
    }))
}

pub fn handle(action: GithubAction) -> anyhow::Result<()> {
    match action {
        GithubAction::Scan {
            repo_days,
            run_days,
            release_days,
            cache_days,
            issue_days,
            pr_days,
            min_asset_size_mb,
        } => {
            github_scan(
                Some(repo_days),
                Some(run_days),
                Some(release_days),
                Some(cache_days),
                Some(issue_days),
                Some(pr_days),
                Some(min_asset_size_mb),
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        GithubAction::Plan {
            repo_days,
            run_days,
            release_days,
            cache_days,
            issue_days,
            pr_days,
            min_asset_size_mb,
            output,
        } => {
            github_plan(
                Some(repo_days),
                Some(run_days),
                Some(release_days),
                Some(cache_days),
                Some(issue_days),
                Some(pr_days),
                Some(min_asset_size_mb),
                output.to_string_lossy().to_string(),
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        GithubAction::Delete { plan, receipt, yes } => {
            github_delete(
                plan.to_string_lossy().to_string(),
                receipt.to_string_lossy().to_string(),
                yes,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        GithubAction::Receipt { receipt, plan } => {
            github_receipt(
                receipt.to_string_lossy().to_string(),
                plan.map(|p| p.to_string_lossy().to_string()),
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
    }
    Ok(())
}

fn is_github_uri(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.starts_with("github://"))
}

fn delete_github_target(
    executor: &dyn CommandExecutor,
    target: &GithubTarget,
) -> anyhow::Result<()> {
    match target {
        GithubTarget::Repo { owner, repo } => delete_repository(executor, owner, repo),
        GithubTarget::Branch {
            owner,
            repo,
            branch,
        } => delete_branch(executor, owner, repo, branch),
        GithubTarget::Run {
            owner,
            repo,
            run_id,
        } => delete_run(executor, owner, repo, *run_id),
        GithubTarget::Release { owner, repo, tag } => delete_release(executor, owner, repo, tag),
        GithubTarget::Cache {
            owner,
            repo,
            cache_id,
            ..
        } => delete_cache(executor, owner, repo, *cache_id),
        GithubTarget::Issue {
            owner,
            repo,
            number,
        } => close_issue(executor, owner, repo, *number),
        GithubTarget::Pr {
            owner,
            repo,
            number,
        } => close_pr(executor, owner, repo, *number),
        GithubTarget::ReleaseAsset {
            owner,
            repo,
            asset_id,
            ..
        } => delete_release_asset(executor, owner, repo, *asset_id),
    }
}

/// Helper to scan GitHub and compile a list of cleanup plan candidates.
#[allow(clippy::too_many_arguments)]
pub fn discover_candidates(
    executor: &dyn CommandExecutor,
    repo_days: i64,
    run_days: i64,
    release_days: i64,
    cache_days: i64,
    issue_days: i64,
    pr_days: i64,
    min_asset_size_mb: u64,
) -> anyhow::Result<Vec<PlanItem>> {
    let current_time_iso = chrono::Utc::now().to_rfc3339();
    let mut items = Vec::new();

    let repos = match list_repositories(executor) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Warning: Failed to list GitHub repositories: {}", e);
            return Ok(items);
        }
    };

    for repo in repos {
        let (owner, repo_name) = (&repo.owner.login, &repo.name);

        // 1. Evaluate repository itself
        if is_repo_stale_or_empty(&repo, repo_days, &current_time_iso) {
            let target = GithubTarget::Repo {
                owner: owner.clone(),
                repo: repo_name.clone(),
            };
            let reason = if repo.is_empty {
                "Empty repository".to_string()
            } else {
                format!("Stale repository (inactive for > {} days)", repo_days)
            };
            items.push(PlanItem {
                path: target.to_path_buf(),
                kind: PlanItemKind::GithubRepo,
                reason,
                bytes: 0,
            });
            // If deleting the repo itself, we don't need to plan deleting its components
            continue;
        }

        // 2. Scan workflow runs
        match list_runs(executor, owner, repo_name) {
            Ok(runs) => {
                for run in runs {
                    if is_run_stale(&run, run_days, &current_time_iso) {
                        let target = GithubTarget::Run {
                            owner: owner.clone(),
                            repo: repo_name.clone(),
                            run_id: run.database_id,
                        };
                        items.push(PlanItem {
                            path: target.to_path_buf(),
                            kind: PlanItemKind::GithubRun,
                            reason: format!(
                                "Completed workflow run older than {} days (run #{})",
                                run_days, run.number
                            ),
                            bytes: 0,
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to list runs for {}/{}: {}",
                    owner, repo_name, e
                );
            }
        }

        // 3. Scan branches
        match list_branches(executor, owner, repo_name) {
            Ok(branches) => {
                // Determine default branch name using the repo default_branch_ref, falling back to heuristics.
                let default_branch_fallback;
                let default_branch = if let Some(ref db_ref) = repo.default_branch_ref {
                    &db_ref.name
                } else {
                    default_branch_fallback = if branches.iter().any(|b| b.name == "main") {
                        "main".to_string()
                    } else if branches.iter().any(|b| b.name == "master") {
                        "master".to_string()
                    } else if let Some(first) = branches.first() {
                        first.name.clone()
                    } else {
                        "main".to_string()
                    };
                    &default_branch_fallback
                };

                for branch in &branches {
                    if branch.name.as_str() == default_branch.as_str() || branch.protected {
                        continue;
                    }
                    match compare_branch(executor, owner, repo_name, default_branch, &branch.name) {
                        Ok(compare) => {
                            if is_branch_merged(&compare) {
                                let target = GithubTarget::Branch {
                                    owner: owner.clone(),
                                    repo: repo_name.clone(),
                                    branch: branch.name.clone(),
                                };
                                items.push(PlanItem {
                                    path: target.to_path_buf(),
                                    kind: PlanItemKind::GithubBranch,
                                    reason: format!(
                                        "Branch fully merged into default branch ({})",
                                        default_branch
                                    ),
                                    bytes: 0,
                                });
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to compare branch {} for {}/{}: {}",
                                branch.name, owner, repo_name, e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to list branches for {}/{}: {}",
                    owner, repo_name, e
                );
            }
        }

        // 4. Scan releases and assets
        match list_releases(executor, owner, repo_name) {
            Ok(releases) => {
                for release in releases {
                    if is_release_stale_or_draft(&release, release_days, &current_time_iso) {
                        let target = GithubTarget::Release {
                            owner: owner.clone(),
                            repo: repo_name.clone(),
                            tag: release.tag_name.clone(),
                        };
                        let reason = if release.is_draft {
                            "Draft release".to_string()
                        } else {
                            format!("Stale release (created > {} days ago)", release_days)
                        };
                        items.push(PlanItem {
                            path: target.to_path_buf(),
                            kind: PlanItemKind::GithubRelease,
                            reason,
                            bytes: 0,
                        });
                    } else {
                        // Release is not stale/draft, scan its assets!
                        for asset in release.assets {
                            let min_size_bytes = min_asset_size_mb * 1024 * 1024;
                            if asset.size >= min_size_bytes {
                                let target = GithubTarget::ReleaseAsset {
                                    owner: owner.clone(),
                                    repo: repo_name.clone(),
                                    asset_id: asset.id,
                                    asset_name: asset.name.clone(),
                                };
                                let asset_size_mb = asset.size as f64 / (1024.0 * 1024.0);
                                items.push(PlanItem {
                                    path: target.to_path_buf(),
                                    kind: PlanItemKind::GithubReleaseAsset,
                                    reason: format!(
                                        "Release asset exceeding size threshold: {} ({:.2} MB)",
                                        asset.name, asset_size_mb
                                    ),
                                    bytes: asset.size,
                                });
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to list releases for {}/{}: {}",
                    owner, repo_name, e
                );
            }
        }

        // 5. Scan caches
        match list_caches(executor, owner, repo_name) {
            Ok(caches) => {
                for cache in caches {
                    if is_cache_stale(&cache, cache_days, &current_time_iso) {
                        let target = GithubTarget::Cache {
                            owner: owner.clone(),
                            repo: repo_name.clone(),
                            cache_id: cache.id,
                            key: cache.key.clone(),
                        };
                        items.push(PlanItem {
                            path: target.to_path_buf(),
                            kind: PlanItemKind::GithubCache,
                            reason: format!("Stale cache (inactive for > {} days)", cache_days),
                            bytes: cache.size_in_bytes,
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to list caches for {}/{}: {}",
                    owner, repo_name, e
                );
            }
        }

        // 6. Scan issues
        match list_issues(executor, owner, repo_name) {
            Ok(issues) => {
                for issue in issues {
                    if is_issue_stale(&issue, issue_days, &current_time_iso) {
                        let target = GithubTarget::Issue {
                            owner: owner.clone(),
                            repo: repo_name.clone(),
                            number: issue.number,
                        };
                        items.push(PlanItem {
                            path: target.to_path_buf(),
                            kind: PlanItemKind::GithubIssue,
                            reason: format!("Stale issue (inactive for > {} days)", issue_days),
                            bytes: 0,
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to list issues for {}/{}: {}",
                    owner, repo_name, e
                );
            }
        }

        // 7. Scan pull requests
        match list_prs(executor, owner, repo_name) {
            Ok(prs) => {
                for pr in prs {
                    if is_pr_stale(&pr, pr_days, &current_time_iso) {
                        let target = GithubTarget::Pr {
                            owner: owner.clone(),
                            repo: repo_name.clone(),
                            number: pr.number,
                        };
                        items.push(PlanItem {
                            path: target.to_path_buf(),
                            kind: PlanItemKind::GithubPr,
                            reason: format!("Stale pull request (inactive for > {} days)", pr_days),
                            bytes: 0,
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to list PRs for {}/{}: {}",
                    owner, repo_name, e
                );
            }
        }
    }

    Ok(items)
}

fn print_scan_summary(items: &[PlanItem]) {
    let mut repos = 0;
    let mut runs = 0;
    let mut branches = 0;
    let mut releases = 0;
    let mut caches = 0;
    let mut issues = 0;
    let mut prs = 0;
    let mut assets = 0;

    println!("\nFound Cleanup Candidates:");
    for item in items {
        println!("  - {}: {}", item.path.display(), item.reason);
        match item.kind {
            PlanItemKind::GithubRepo => repos += 1,
            PlanItemKind::GithubRun => runs += 1,
            PlanItemKind::GithubBranch => branches += 1,
            PlanItemKind::GithubRelease => releases += 1,
            PlanItemKind::GithubCache => caches += 1,
            PlanItemKind::GithubIssue => issues += 1,
            PlanItemKind::GithubPr => prs += 1,
            PlanItemKind::GithubReleaseAsset => assets += 1,
            _ => {}
        }
    }

    println!("\n==================================================");
    println!("               GITHUB SCAN SUMMARY               ");
    println!("==================================================");
    println!("  Stale/Empty Repositories:    {}", repos);
    println!("  Merged Branches:            {}", branches);
    println!("  Stale Workflow Runs:        {}", runs);
    println!("  Draft/Stale Releases:       {}", releases);
    println!("  Stale Caches:               {}", caches);
    println!("  Stale Issues:               {}", issues);
    println!("  Stale Pull Requests:        {}", prs);
    println!("  Large Release Assets:       {}", assets);
    println!("==================================================");
}
