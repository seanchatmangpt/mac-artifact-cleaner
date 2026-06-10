//! Delete CLI noun implementation.
//!
//! **Noun layer rule**: This module parses, routes, and formats output only.
//! All destructive filesystem operations are delegated to `integration::fs`.

use crate::domain::delete::{DeletionPlanAdjudicator, PlanSafetyWitness};
use crate::domain::plan::{DeletionPlan, PlanItemKind};
use crate::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus};
use crate::integration::fs::{delete_dir_all, delete_file};
use clap::Subcommand;
use rayon::prelude::*;
use std::path::PathBuf;
use wasm4pm_compat::admission::Admit;
use wasm4pm_compat::evidence::Evidence;
use wasm4pm_compat::state::Raw;

#[derive(Subcommand, Debug)]
pub enum DeleteAction {
    /// Execute plan-bound deletions
    Execute {
        /// Path to the deletion plan
        #[arg(short, long)]
        plan: PathBuf,
        /// Path to write the execution receipt
        #[arg(short, long)]
        receipt: PathBuf,
    },
}

use indicatif::{ProgressBar, ProgressStyle};

pub fn handle(action: DeleteAction) -> anyhow::Result<()> {
    match action {
        DeleteAction::Execute {
            plan: plan_path,
            receipt: receipt_path,
        } => {
            let content = std::fs::read_to_string(&plan_path)?;
            let plan: DeletionPlan = serde_json::from_str(&content)?;

            // Validation step — transition from Raw to Admitted using Evidence typestates.
            let raw_evidence = Evidence::<_, Raw, PlanSafetyWitness>::raw(plan);
            let admitted_plan = match DeletionPlanAdjudicator::admit(raw_evidence) {
                Ok(admitted) => admitted.into_evidence(),
                Err(refusal) => anyhow::bail!("Plan validation failed: {}", refusal.reason),
            };

            // Rebind plan to the value inside the Admitted evidence to prove it's safe to use.
            let plan = admitted_plan.into_inner();

            println!("Executing deletion from plan: {}", plan_path.display());
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

            // Concurrent deletion using Rayon
            let results: Vec<DeletionResult> = plan
                .items
                .par_iter()
                .map(|item| {
                    pb.set_message(format!("Deleting {} ...", item.path.display()));

                    let res = if !item.path.exists() {
                        DeletionResult {
                            path: item.path.clone(),
                            status: DeletionStatus::SkippedMissing,
                            error: None,
                        }
                    } else {
                        // Delegate all filesystem mutations to the integration layer.
                        match item.kind {
                            PlanItemKind::File => match delete_file(&item.path) {
                                Ok(()) => DeletionResult {
                                    path: item.path.clone(),
                                    status: DeletionStatus::Deleted,
                                    error: None,
                                },
                                Err(e) => DeletionResult {
                                    path: item.path.clone(),
                                    status: DeletionStatus::Failed,
                                    error: Some(e.to_string()),
                                },
                            },
                            PlanItemKind::Dir => match delete_dir_all(&item.path) {
                                Ok(()) => DeletionResult {
                                    path: item.path.clone(),
                                    status: DeletionStatus::Deleted,
                                    error: None,
                                },
                                Err(e) => DeletionResult {
                                    path: item.path.clone(),
                                    status: DeletionStatus::Failed,
                                    error: Some(e.to_string()),
                                },
                            },
                        }
                    };
                    pb.inc(1);
                    res
                })
                .collect();

            pb.finish_with_message("Deletion execution complete.");

            let end_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let receipt = DeletionReceipt::new(
                "deletion-chain-001".to_string(),
                plan.created_unix,
                start_time,
                end_time,
                results,
            );
            let serialized_receipt = serde_json::to_string_pretty(&receipt)?;
            std::fs::write(&receipt_path, serialized_receipt)?;

            let mut deleted_count = 0;
            let mut skipped_count = 0;
            let mut failed_count = 0;
            let mut failures = Vec::new();

            for r in &receipt.execution_record.results {
                match r.status {
                    DeletionStatus::Deleted => deleted_count += 1,
                    DeletionStatus::SkippedMissing => skipped_count += 1,
                    DeletionStatus::Failed => {
                        failed_count += 1;
                        if let Some(err) = &r.error {
                            failures.push((r.path.clone(), err.clone()));
                        } else {
                            failures.push((r.path.clone(), "Unknown error".to_string()));
                        }
                    }
                    DeletionStatus::Refused => skipped_count += 1,
                }
            }

            println!("\n==================================================");
            println!("             DELETION EXECUTION SUMMARY           ");
            println!("==================================================");
            println!("  Total Items: {}", plan.items.len());
            println!("  Deleted:     {}", deleted_count);
            println!("  Skipped:     {}", skipped_count);
            println!("  Failed:      {}", failed_count);
            println!("  Elapsed:     {} seconds", end_time - start_time);
            println!("==================================================");

            if !failures.is_empty() {
                println!("\n==================================================");
                println!("               DELETION FAILURES                  ");
                println!("==================================================");
                for (path, err) in &failures {
                    println!("  ❌ {}: {}", path.display(), err);
                }
                println!("==================================================");
            }

            println!("\nReceipt written to: {}", receipt_path.display());
        }
    }
    Ok(())
}
