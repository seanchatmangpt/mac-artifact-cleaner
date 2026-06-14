//! GitHub CLI noun implementation.
//!
//! Parses, routes, and formats output for GitHub operations, delegating API logic
//! to `integration::github` and candidate classification to `domain::github`.

use crate::domain::delete::{DeletionPlanAdjudicator, PlanSafetyWitness};
use crate::domain::github::{
    is_branch_merged, is_release_stale_or_draft, is_repo_stale_or_empty, is_run_stale,
    GithubTarget,
};
use crate::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
use crate::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
use crate::integration::github::{
    compare_branch, delete_branch, delete_release, delete_repository, delete_run,
    list_branches, list_releases, list_repositories, list_runs, CommandExecutor,
    RealCommandExecutor,
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

pub fn handle(action: GithubAction) -> anyhow::Result<()> {
    let executor = RealCommandExecutor;
    match action {
        GithubAction::Scan {
            repo_days,
            run_days,
            release_days,
        } => {
            println!("Scanning GitHub repositories and resources...");
            let candidates = discover_candidates(&executor, repo_days, run_days, release_days)?;
            print_scan_summary(&candidates);
        }
        GithubAction::Plan {
            repo_days,
            run_days,
            release_days,
            output,
        } => {
            println!("Building GitHub cleanup plan...");
            let candidates = discover_candidates(&executor, repo_days, run_days, release_days)?;
            let plan = DeletionPlan::new(
                vec![PathBuf::from("github://")],
                false,
                false,
                candidates,
                vec![],
            );
            let content = serde_json::to_string_pretty(&plan)?;
            std::fs::write(&output, content)?;
            println!("Successfully wrote GitHub deletion plan to {}", output.display());
        }
        GithubAction::Delete { plan: plan_path, receipt: receipt_path } => {
            let content = std::fs::read_to_string(&plan_path)?;
            let plan: DeletionPlan = serde_json::from_str(&content)?;

            // Validation step using Evidence typestates.
            let raw_evidence = Evidence::<_, Raw, PlanSafetyWitness>::raw(plan);
            let admitted_plan = match DeletionPlanAdjudicator::admit(raw_evidence) {
                Ok(admitted) => admitted.into_evidence(),
                Err(refusal) => anyhow::bail!("Plan validation failed: {}", refusal.reason),
            };
            let plan = admitted_plan.into_inner();

            println!("Executing GitHub deletions from plan: {}", plan_path.display());
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
                let res = if !is_github_uri(&item.path) {
                    DeletionResult {
                        path: item.path.clone(),
                        status: DeletionStatus::SkippedMissing,
                        error: Some("Non-GitHub item skipped during github delete".to_string()),
                        blake3_hash: None,
                        bytes_freed: 0,
                    }
                } else if let Some(target) = GithubTarget::parse(&item.path) {
                    match delete_github_target(&executor, &target) {
                        Ok(()) => DeletionResult {
                            path: item.path.clone(),
                            status: DeletionStatus::Deleted,
                            error: None,
                            blake3_hash: None,
                            bytes_freed: 0,
                        },
                        Err(e) => {
                            let err_str = e.to_string();
                            // Map "not found" or similar HTTP errors to SkippedMissing if appropriate, or keep as Failed
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

            let receipt = DeletionReceipt::new(
                "github-deletion-chain-001".to_string(),
                plan.created_unix,
                start_time,
                end_time,
                results,
                None,
                None,
            );

            let receipt_json = serde_json::to_string_pretty(&receipt)?;
            std::fs::write(&receipt_path, receipt_json)?;
            println!("Successfully wrote execution receipt to {}", receipt_path.display());
        }
        GithubAction::Receipt { receipt: receipt_path, plan: plan_path } => {
            let content = std::fs::read_to_string(&receipt_path)?;
            let receipt: DeletionReceipt = serde_json::from_str(&content)?;

            let plan = if let Some(p_path) = plan_path {
                let p_content = std::fs::read_to_string(&p_path)?;
                Some(serde_json::from_str::<DeletionPlan>(&p_content)?)
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

            if !report.issues.is_empty() {
                println!("\nIssues:");
                for issue in &report.issues {
                    println!("  - [{:?}] {}: {}", issue.issue_type, issue.path.display(), issue.message);
                }
                anyhow::bail!("Receipt verification failed (inconsistent evidence).");
            } else {
                println!("\nAll checks passed. Receipt matches deletion plan and reality rules.");
            }
        }
    }
    Ok(())
}

fn is_github_uri(path: &Path) -> bool {
    path.to_str().map_or(false, |s| s.starts_with("github://"))
}

fn delete_github_target(executor: &dyn CommandExecutor, target: &GithubTarget) -> anyhow::Result<()> {
    match target {
        GithubTarget::Repo { owner, repo } => delete_repository(executor, owner, repo),
        GithubTarget::Branch { owner, repo, branch } => delete_branch(executor, owner, repo, branch),
        GithubTarget::Run { owner, repo, run_id } => delete_run(executor, owner, repo, *run_id),
        GithubTarget::Release { owner, repo, tag } => delete_release(executor, owner, repo, tag),
    }
}

/// Helper to scan GitHub and compile a list of cleanup plan candidates.
pub fn discover_candidates(
    executor: &dyn CommandExecutor,
    repo_days: i64,
    run_days: i64,
    release_days: i64,
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
                            reason: format!("Completed workflow run older than {} days (run #{})", run_days, run.number),
                            bytes: 0,
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to list runs for {}/{}: {}", owner, repo_name, e);
            }
        }

        // 3. Scan branches
        match list_branches(executor, owner, repo_name) {
            Ok(branches) => {
                // Determine default branch name from the branch list
                let default_branch = if branches.iter().any(|b| b.name == "main") {
                    "main"
                } else if branches.iter().any(|b| b.name == "master") {
                    "master"
                } else if let Some(first) = branches.first() {
                    &first.name
                } else {
                    "main"
                };

                for branch in &branches {
                    if branch.name == default_branch || branch.protected {
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
                                    reason: format!("Branch fully merged into default branch ({})", default_branch),
                                    bytes: 0,
                                });
                            }
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to compare branch {} for {}/{}: {}", branch.name, owner, repo_name, e);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to list branches for {}/{}: {}", owner, repo_name, e);
            }
        }

        // 4. Scan releases
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
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to list releases for {}/{}: {}", owner, repo_name, e);
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

    println!("\nFound Cleanup Candidates:");
    for item in items {
        println!("  - {}: {}", item.path.display(), item.reason);
        match item.kind {
            PlanItemKind::GithubRepo => repos += 1,
            PlanItemKind::GithubRun => runs += 1,
            PlanItemKind::GithubBranch => branches += 1,
            PlanItemKind::GithubRelease => releases += 1,
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
    println!("==================================================");
}
