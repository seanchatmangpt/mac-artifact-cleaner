//! Receipt CLI noun implementation.

use std::path::PathBuf;

use clap::Subcommand;

use crate::domain::{
    affidavit_integration as affidavit, plan::DeletionPlan, receipt::DeletionReceipt,
};

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
    /// Build (and optionally persist) an affidavit `core/v1` provenance receipt
    /// from a deletion receipt, then run the 7-stage certify pipeline over it.
    Certify {
        /// Path to the deletion receipt
        #[arg(short, long)]
        receipt: PathBuf,
        /// Optional path to write the sealed affidavit receipt (canonical JSON)
        #[arg(short, long)]
        out: Option<PathBuf>,
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

            // Affidavit provenance: build the sealed core/v1 chain from this
            // receipt and certify it through the 7-stage pipeline. This is a
            // structural witness (the receipt was not forged or hand-edited),
            // complementary to the filesystem-consistency report above.
            let affidavit_receipt = affidavit::build_deletion_affidavit(&receipt_data);
            let verdict = affidavit::certify(&affidavit_receipt);

            // If a sealed affidavit file was persisted alongside this receipt
            // (by `receipt certify`), compare its stored chain_hash against
            // the one we just recomputed. A prior hand-edit of that file
            // (or the receipt it was sealed from) must surface as a
            // chain_integrity failure rather than being silently ignored.
            let affidavit_path = receipt.with_extension("affidavit.json");
            let stored_chain_hash = std::fs::read_to_string(&affidavit_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("chain_hash").and_then(|h| h.as_str().map(str::to_string)));
            let verdict = affidavit::verify_chain_hash(
                verdict,
                affidavit_receipt.chain_hash.as_hex(),
                stored_chain_hash.as_deref(),
            );
            print_verdict(&verdict);

            if report.is_consistent && verdict.accepted {
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
                if !verdict.accepted {
                    anyhow::bail!(
                        "Receipt verification failed: affidavit chain_integrity rejected — {}\n\nSuggestions:\n  - Review the issues listed above for details\n  - Do not treat this receipt as certified\n  - Re-run `oclnr delete execute` from a fresh plan if the receipt is unrecoverable",
                        verdict.reason
                    );
                }
                anyhow::bail!(
                    "Receipt verification failed with {} consistency issues.\n\nSuggestions:\n  - Review the issues listed above for details\n  - Verify no external process modified the filesystem during deletion\n  - Re-run `oclnr delete execute` from a fresh plan if the receipt is unrecoverable",
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
            println!("  Receipt Version:   {}", receipt_data.execution_record.version);
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
            println!("  Total Items:       {}", receipt_data.execution_record.results.len());

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
        ReceiptAction::Certify { receipt, out } => {
            let content = std::fs::read_to_string(&receipt)?;
            let receipt_data: DeletionReceipt = serde_json::from_str(&content)?;

            let affidavit_receipt = affidavit::build_deletion_affidavit(&receipt_data);
            let verdict = affidavit::certify(&affidavit_receipt);

            // If a sealed affidavit file already exists at the canonical
            // location (from a prior `receipt certify` run), treat its
            // chain_hash as the provenance claim and compare it against what
            // we just recomputed. A mismatch means the receipt or the
            // affidavit file was hand-edited after the original seal, and
            // must reject rather than silently re-certifying the tamper.
            let canonical_affidavit_path = receipt.with_extension("affidavit.json");
            let stored_chain_hash = std::fs::read_to_string(&canonical_affidavit_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("chain_hash").and_then(|h| h.as_str().map(str::to_string)));
            let verdict = affidavit::verify_chain_hash(
                verdict,
                affidavit_receipt.chain_hash.as_hex(),
                stored_chain_hash.as_deref(),
            );

            println!("Certifying affidavit provenance for: {}", receipt.display());
            println!("  Chain hash (core/v1):  {}", affidavit_receipt.chain_hash);
            println!("  Content address:       {}", affidavit::content_address(&affidavit_receipt));
            println!("  Events:                {}", affidavit_receipt.events.len());
            print_verdict(&verdict);

            if let Some(out_path) = out {
                // Persist canonical (sorted-key) JSON so the receipt is
                // byte-stable and re-verifiable by upstream `affi verify`.
                let bytes = affidavit::serialize_receipt(&affidavit_receipt);
                std::fs::write(&out_path, &bytes)?;
                println!("\nAffidavit receipt written to: {}", out_path.display());
            }

            if verdict.accepted {
                Ok(())
            } else {
                anyhow::bail!(
                    "Affidavit certification REJECTED: {}\n\nSuggestions:\n  - Review the rejection reason above\n  - Re-run `oclnr receipt verify` to check receipt consistency\n  - Do not treat the deletion as certified until this is resolved",
                    verdict.reason
                );
            }
        }
    }
}

/// Render an affidavit [`Verdict`](crate::domain::affidavit::Verdict) as a
/// per-stage table plus the final ACCEPT/REJECT line.
fn print_verdict(verdict: &affidavit::Verdict) {
    println!("\n==================================================");
    println!("        AFFIDAVIT CERTIFICATION ({})        ", verdict.profile.as_str());
    println!("==================================================");
    for outcome in &verdict.outcomes {
        let mark = if outcome.passed { "✅" } else { "❌" };
        println!("  {} {:<18} {}", mark, outcome.stage, outcome.detail);
    }
    println!("--------------------------------------------------");
    if verdict.accepted {
        println!("  VERDICT: ✅ ACCEPTED — {}", verdict.reason);
    } else {
        println!("  VERDICT: ❌ REJECTED — {}", verdict.reason);
    }
    println!("==================================================");
}
