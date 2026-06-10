//! Receipt CLI noun implementation.

use crate::domain::plan::DeletionPlan;
use crate::domain::receipt::DeletionReceipt;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum ReceiptAction {
    /// Verify the contents, integrity, and filesystem consistency of a deletion receipt
    Verify {
        /// Path to the deletion receipt
        #[arg(short, long)]
        receipt: PathBuf,
        /// Optional path to the original deletion plan to check correlation
        #[arg(short, long)]
        plan: Option<PathBuf>,
    },
    /// Summarize the outcomes recorded in a deletion receipt
    Summarize {
        /// Path to the deletion receipt
        #[arg(short, long)]
        receipt: PathBuf,
    },
}

pub fn handle(action: ReceiptAction) -> anyhow::Result<()> {
    match action {
        ReceiptAction::Verify { receipt, plan } => {
            let content = std::fs::read_to_string(&receipt)?;
            let receipt_data: DeletionReceipt = serde_json::from_str(&content)?;

            let plan_data = if let Some(plan_path) = plan {
                let plan_content = std::fs::read_to_string(plan_path)?;
                let p: DeletionPlan = serde_json::from_str(&plan_content)?;
                Some(p)
            } else {
                None
            };

            println!("Verifying deletion receipt: {}", receipt.display());
            let report = receipt_data.verify(plan_data.as_ref());

            if report.is_consistent {
                println!(
                    "✅ Receipt verification passed: all records are consistent with disk state."
                );
                Ok(())
            } else {
                println!("\n==================================================");
                println!("          RECEIPT VERIFICATION ISSUES             ");
                println!("==================================================");
                for issue in &report.issues {
                    println!(
                        "  ❌ [{:?}] {}: {}",
                        issue.issue_type,
                        issue.path.display(),
                        issue.message
                    );
                }
                println!("==================================================");
                anyhow::bail!(
                    "Receipt verification failed with {} consistency issues.",
                    report.issues.len()
                );
            }
        }
        ReceiptAction::Summarize { receipt } => {
            let content = std::fs::read_to_string(&receipt)?;
            let receipt_data: DeletionReceipt = serde_json::from_str(&content)?;
            println!("\n==================================================");
            println!("             DELETION RECEIPT SUMMARY             ");
            println!("==================================================");
            println!(
                "  Receipt Version:   {}",
                receipt_data.execution_record.version
            );
            println!(
                "  Plan Created:      {}",
                chrono::DateTime::from_timestamp(
                    receipt_data.execution_record.plan_created_unix as i64,
                    0
                )
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| receipt_data.execution_record.plan_created_unix.to_string())
            );
            println!(
                "  Execution Started: {}",
                chrono::DateTime::from_timestamp(
                    receipt_data.execution_record.execution_started_unix as i64,
                    0
                )
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| receipt_data
                    .execution_record
                    .execution_started_unix
                    .to_string())
            );
            println!(
                "  Execution Ended:   {}",
                chrono::DateTime::from_timestamp(
                    receipt_data.execution_record.execution_completed_unix as i64,
                    0
                )
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| receipt_data
                    .execution_record
                    .execution_completed_unix
                    .to_string())
            );
            println!(
                "  Elapsed Time:      {} seconds",
                receipt_data
                    .execution_record
                    .execution_completed_unix
                    .saturating_sub(receipt_data.execution_record.execution_started_unix)
            );
            println!(
                "  Total Items:       {}",
                receipt_data.execution_record.results.len()
            );

            let mut deleted = 0;
            let mut skipped = 0;
            let mut failed = 0;
            let mut refused = 0;

            for r in &receipt_data.execution_record.results {
                match r.status {
                    crate::domain::receipt::DeletionStatus::Deleted => deleted += 1,
                    crate::domain::receipt::DeletionStatus::SkippedMissing => skipped += 1,
                    crate::domain::receipt::DeletionStatus::Failed => failed += 1,
                    crate::domain::receipt::DeletionStatus::Refused => refused += 1,
                }
            }

            println!("  • Deleted:         {}", deleted);
            println!("  • Skipped:         {}", skipped);
            println!("  • Failed:          {}", failed);
            println!("  • Refused:         {}", refused);
            println!("==================================================");
            Ok(())
        }
    }
}
