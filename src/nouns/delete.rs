//! Delete CLI noun implementation.
//!
//! **Noun layer rule**: This module parses, routes, and formats output only.
//! All destructive filesystem operations are delegated to `integration::fs`.

use std::path::PathBuf;

use clap::Subcommand;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use wasm4pm_compat::{admission::Admit, evidence::Evidence, state::Raw};

use crate::{
    domain::{
        dcm::Reversibility,
        delete::{
            classify_nonmutating_outcome, require_plan_approved, DeletionPlanAdjudicator,
            PlanSafetyWitness,
        },
        plan::{partition_nested_items, DeletionPlan, PlanItemKind},
        receipt::{check_reclaim, DeletionReceipt, DeletionResult, DeletionStatus, ReclaimCheck},
    },
    integration::{
        fs::{
            delete_dir_all_with_progress, delete_file, generate_manifest, refuse_if_symlink,
            volume_space, write_or_dump_on_full, WriteOutcome,
        },
        progress::human_bytes,
    },
};

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
        /// Actually delete; default is a dry-run preview
        #[arg(long)]
        yes: bool,
        /// Bound concurrent deletion to this many threads (default: rayon's
        /// global pool width, typically the number of CPUs).
        #[arg(long)]
        max_concurrent: Option<usize>,
    },
}

/// Log-line prefix tag for an item whose reversibility is not
/// [`Reversibility::Reversible`].
///
/// The actual deletion loop otherwise dispatches purely on `item.kind` and
/// `item.path.exists()`, so an `Unknown`/`Compensatable`/`Irreversible` item
/// (a hand-edited `node_modules`, a venv with unpushed local changes) was
/// deleted with exactly the same code path and the same log/progress-bar
/// treatment as a `Reversible` one (rust `target/`) — the only place
/// reversibility was surfaced to a human was `plan(inspect)`/
/// `plan(validate)`, both optional calls a caller can skip entirely between
/// `plan build` and `delete execute`. Tagging the live per-item message
/// here means an operator watching the run (the progress bar's `{msg}`,
/// the dry-run preview, and the post-execution summary) sees the
/// distinction as it happens, not only in a pre-approval report they may
/// never have requested.
fn reversibility_tag(reversibility: Reversibility) -> &'static str {
    match reversibility {
        Reversibility::Reversible => "",
        Reversibility::Compensatable => "[COMPENSATABLE] ",
        Reversibility::Unknown => "[UNKNOWN-REVERSIBILITY] ",
        Reversibility::Irreversible => "[IRREVERSIBLE] ",
    }
}

pub fn handle(action: DeleteAction) -> anyhow::Result<()> {
    match action {
        DeleteAction::Execute { plan: plan_path, receipt: receipt_path, yes, max_concurrent } => {
            let content = std::fs::read_to_string(&plan_path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to read plan file {}: {}\n\nSuggestions:\n  - Check that {} was created by `oclnr plan build`\n  - Re-run `oclnr plan build` to regenerate the plan\n  - Check file permissions on {}",
                    plan_path.display(),
                    e,
                    plan_path.display(),
                    plan_path.display()
                )
            })?;
            let plan: DeletionPlan = serde_json::from_str(&content).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to parse plan file {}: {}\n\nSuggestions:\n  - Check that {} is valid JSON\n  - Re-run `oclnr plan build` to regenerate the plan",
                    plan_path.display(),
                    e,
                    plan_path.display()
                )
            })?;

            // Validation step — transition from Raw to Admitted using Evidence typestates.
            let raw_evidence = Evidence::<_, Raw, PlanSafetyWitness>::raw(plan);
            let admitted_plan = match DeletionPlanAdjudicator::admit(raw_evidence) {
                Ok(admitted) => admitted.into_evidence(),
                Err(refusal) => anyhow::bail!(
                    "Plan validation failed: {}\n\nSuggestions:\n  - Call `oclnr plan inspect --plan {}` to review the plan\n  - Rebuild the plan with `oclnr plan build`\n  - Verify no external processes modified the filesystem since the plan was created",
                    refusal.reason,
                    plan_path.display()
                ),
            };

            // Rebind plan to the value inside the Admitted evidence to prove it's safe to use.
            let plan = admitted_plan.into_inner();

            // Require a valid, untampered approval signature bound to this
            // exact plan content before any destructive execution. Preview
            // (dry-run, `!yes`) is still allowed on an unapproved plan so
            // `plan_inspect`/`delete_dry_run` can review before approving —
            // only the actual deletion is gated. Refuses plans that were
            // never approved via `plan_approve`, and refuses plans that were
            // hand-edited (items appended/substituted) after approval —
            // closing the gap where `delete execute --yes` would otherwise
            // delete whatever the plan file currently says regardless of
            // what was reviewed and signed.
            if yes {
                let secret = crate::integration::config::approval_secret()
                    .map_err(|e| anyhow::anyhow!("cannot source plan-approval secret: {}", e))?;
                if let Err(reason) = require_plan_approved(&plan, &secret) {
                    anyhow::bail!(
                        "Plan approval check failed: {}\n\nSuggestions:\n  - Call `plan_approve` on this exact plan file before executing\n  - Re-run `oclnr plan build` and re-approve if the plan was intentionally changed\n  - Do not hand-edit cleanup-plan.json after approval",
                        reason
                    );
                }
            }

            if !yes {
                println!("==================================================");
                println!("             DELETION DRY-RUN PREVIEW             ");
                println!("==================================================");
                println!("  Mode: DRY-RUN (pass --yes to execute deletion)");
                println!("  Plan: {}", plan_path.display());
                println!("  Items that would be deleted: {}", plan.items.len());
                let mut total_bytes: u64 = 0;
                let mut non_reversible = 0usize;
                for item in &plan.items {
                    total_bytes += item.bytes;
                    if item.reversibility != Reversibility::Reversible {
                        non_reversible += 1;
                    }
                    println!(
                        "    - {}{} ({}) [{}]",
                        reversibility_tag(item.reversibility),
                        item.path.display(),
                        human_bytes(item.bytes),
                        item.reversibility.label()
                    );
                }
                println!("  Total (planned): {}", human_bytes(total_bytes));
                if non_reversible > 0 {
                    println!(
                        "  WARNING: {} item(s) are not classified Reversible (unknown/compensatable/irreversible) — review with `plan(validate)` before approving.",
                        non_reversible
                    );
                }
                println!("==================================================");
                println!("\nNo files were deleted. Re-run with --yes to execute.");
                return Ok(());
            }

            println!("Executing deletion from plan: {}", plan_path.display());
            // Sample the volume that actually holds the plan's target paths, not
            // always the boot volume `/` — on macOS `/` is a sealed read-only
            // System-volume snapshot; real user data (and its free space) lives on
            // the Data volume (typically mounted at /System/Volumes/Data, firmlinked
            // into /Users). Hardcoding `/` here happened to read correctly on setups
            // where APFS shares free space across both volumes in one container, but
            // is not guaranteed and produced misleading before/after deltas.
            let volume_probe_path: std::path::PathBuf =
                plan.roots.first().cloned().unwrap_or_else(|| std::path::PathBuf::from("/"));
            let space_before = volume_space(&volume_probe_path).ok();
            let start_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // Sized to total planned bytes rather than item count, and advanced
            // incrementally (per file removed) even while a single item is a
            // large directory. Previously the bar was sized to item count and
            // only advanced once per item on whole-item completion, so a plan
            // with one 24GB `target/` left the bar frozen at the same
            // position for the entire `delete_dir_all` call — a user watching
            // it had no way to tell the process from a hang. {bytes} and
            // {total_bytes} are indicatif's built-in human-readable byte
            // templates, so the bar and its label move together.
            // Split the plan into top-level items (safe to delete
            // independently/concurrently) and items nested inside another
            // planned item. Deleting a parent directory removes its whole
            // subtree, so a nested item's own delete call can race against
            // a path its ancestor's deletion already removed — a TOCTOU
            // window between the `exists()` check and the actual delete
            // call, which otherwise surfaces as a spurious `Failed` entry
            // in the sealed receipt for what is actually a benign
            // already-gone-via-ancestor-deletion condition. Filtering
            // nested items out of the concurrent loop removes the race by
            // construction rather than reporting it after the fact; nested
            // items still get a truthful `SkippedMissing` receipt entry so
            // the evidentiary record is unchanged in size, only more
            // accurate in status.
            let (items_to_delete, nested_items) = partition_nested_items(plan.items.clone());
            let nested_results: Vec<DeletionResult> = nested_items
                .into_iter()
                .map(|item| DeletionResult {
                    path: item.path,
                    status: DeletionStatus::SkippedMissing,
                    error: Some(
                        "path is nested inside another planned item; removed as part of that \
                         item's deletion rather than independently"
                            .to_string(),
                    ),
                    blake3_hash: None,
                    bytes_freed: 0,
                    reversibility: item.reversibility,
                })
                .collect();

            // Surface non-Reversible items up front, once, before the bar
            // starts ticking — an operator scrolling back through a long
            // run should not have to hunt through interleaved per-item
            // messages to find out which paths were not provably safe.
            let non_reversible_items: Vec<_> = items_to_delete
                .iter()
                .filter(|item| item.reversibility != Reversibility::Reversible)
                .collect();
            if !non_reversible_items.is_empty() {
                println!(
                    "\nWARNING: {} of {} item(s) about to be deleted are not classified Reversible:",
                    non_reversible_items.len(),
                    items_to_delete.len()
                );
                for item in &non_reversible_items {
                    println!(
                        "  {}{} [{}] — {}",
                        reversibility_tag(item.reversibility),
                        item.path.display(),
                        item.reversibility.label(),
                        item.reason
                    );
                }
                println!();
            }

            let total_planned_bytes: u64 = items_to_delete.iter().map(|item| item.bytes).sum();
            let pb = ProgressBar::new(total_planned_bytes);
            pb.set_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({percent}%) {msg}",
                )
                .expect("static progress template is valid")
                .progress_chars("#>-"),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            // Concurrent deletion using Rayon, optionally bounded to
            // `max_concurrent` threads. Building a scoped pool and calling
            // `.install()` confines this deletion's parallelism without
            // touching rayon's global pool (which other calls in-process
            // may still be using with its default width).
            let delete_body = || -> Vec<DeletionResult> {
                items_to_delete
                    .par_iter()
                    .map(|item| {
                        pb.set_message(format!(
                            "{}Deleting {} ...",
                            reversibility_tag(item.reversibility),
                            item.path.display()
                        ));
                        // Bytes already reported to `pb` for *this* item via a
                        // sub-item progress callback (currently only the Dir
                        // arm below reports mid-flight); the top-off after the
                        // match makes up the rest of `item.bytes` so the bar's
                        // total always reaches `total_planned_bytes` exactly
                        // once, regardless of which arm ran or its outcome.
                        let item_bytes_reported = std::cell::Cell::new(0u64);

                        let path_exists = item.path.exists();
                        let res = if let Some((status, bytes_freed)) =
                            classify_nonmutating_outcome(item, path_exists)
                        {
                            // Shared with MCP `delete_dry_run`'s preview via
                            // `classify_nonmutating_outcome` so the non-mutating
                            // branches (missing path, GitHub-kind items) cannot
                            // structurally diverge between preview and execute again.
                            DeletionResult {
                                path: item.path.clone(),
                                status,
                                error: if status == DeletionStatus::Failed {
                                    Some(
                                        "GitHub resources must be deleted using the github command"
                                            .to_string(),
                                    )
                                } else {
                                    None
                                },
                                blake3_hash: None,
                                bytes_freed,
                                reversibility: item.reversibility,
                            }
                        } else {
                            // Delegate all filesystem mutations to the integration layer.
                            // On success, the planned physical size is what was reclaimed.
                            match item.kind {
                                PlanItemKind::File => {
                                    // Generate cryptographic manifest before deletion. A
                                    // failure here (permission error, race with another
                                    // process, unreadable file) must not be silently
                                    // conflated with "hashing was never attempted" — the
                                    // receipt's evidentiary value depends on knowing why
                                    // a hash is missing.
                                    // Refuse a swapped-in symlink BEFORE ever reading/hashing
                                    // its content — generate_manifest opens and BLAKE3-hashes
                                    // whatever the path resolves to, so this check must run
                                    // first or a symlink planted after plan approval (e.g.
                                    // pointing at ~/.ssh/id_rsa) gets its target's content
                                    // hashed into the sealed receipt.
                                    let (hash, hash_err) = match refuse_if_symlink(&item.path) {
                                        Err(e) => (None, Some(e.to_string())),
                                        Ok(()) => match generate_manifest(&item.path) {
                                            Ok(h) => (Some(h), None),
                                            Err(e) => (None, Some(format!("hash failed: {}", e))),
                                        },
                                    };

                                    // Measure the file's real on-disk size right before
                                    // it is removed — this is what "bytes freed" means
                                    // for a single file, not the plan's stale `item.bytes`
                                    // snapshot from `plan build` time (the file may have
                                    // grown or shrunk since). Falls back to the planned
                                    // size only if the stat itself fails (e.g. a race
                                    // removed the file between the `.exists()` check
                                    // above and here).
                                    let measured_bytes = std::fs::metadata(&item.path)
                                        .map(|m| m.len())
                                        .unwrap_or(item.bytes);

                                    match delete_file(&item.path) {
                                        Ok(()) => DeletionResult {
                                            path: item.path.clone(),
                                            status: DeletionStatus::Deleted,
                                            error: hash_err,
                                            blake3_hash: hash,
                                            bytes_freed: measured_bytes,
                                            reversibility: item.reversibility,
                                        },
                                        Err(e) => DeletionResult {
                                            path: item.path.clone(),
                                            status: DeletionStatus::Failed,
                                            error: Some(match hash_err {
                                                Some(he) => format!("{}; {}", he, e),
                                                None => e.to_string(),
                                            }),
                                            blake3_hash: hash,
                                            bytes_freed: 0,
                                            reversibility: item.reversibility,
                                        },
                                    }
                                }
                                PlanItemKind::Dir => {
                                    // Report incremental progress (bytes
                                    // removed so far) as the directory is
                                    // walked instead of the bar sitting
                                    // frozen at this item's starting position
                                    // for the entire call — the "looks hung"
                                    // symptom this fix targets for a single
                                    // large `target/`-sized item. The shared
                                    // top-off below (keyed on
                                    // `item_bytes_reported`) makes up any gap
                                    // between real bytes removed and this
                                    // item's planned weight once the call
                                    // returns.
                                    let dir_result = delete_dir_all_with_progress(
                                        &item.path,
                                        |freed| {
                                            item_bytes_reported
                                                .set(item_bytes_reported.get() + freed);
                                            pb.inc(freed);
                                        },
                                    );
                                    match dir_result {
                                        Ok(()) => DeletionResult {
                                            path: item.path.clone(),
                                            status: DeletionStatus::Deleted,
                                            error: None,
                                            blake3_hash: None,
                                            // Measured, not planned: `reported` is the
                                            // real sum of per-file sizes the
                                            // `delete_dir_all_with_progress` callback
                                            // observed while actually removing this
                                            // directory's contents, not the stale
                                            // `item.bytes` recorded at plan-build time.
                                            bytes_freed: item_bytes_reported.get(),
                                            reversibility: item.reversibility,
                                        },
                                        Err(e) => {
                                            // `remove_dir_all`/`force_remove_dir_all` abort on
                                            // the first unremovable entry, but everything
                                            // removed before that point is gone for real —
                                            // `reported` (accumulated by the per-file `on_bytes`
                                            // callback above) is the true physical reclaim,
                                            // never 0 just because the subtree as a whole didn't
                                            // finish. Collapsing a partial deletion's
                                            // bytes_freed to 0 would understate the receipt's
                                            // own evidentiary record of what actually happened
                                            // on disk — the opposite of "never increase
                                            // destructive power without increasing receipts."
                                            DeletionResult {
                                            path: item.path.clone(),
                                            status: DeletionStatus::Failed,
                                            error: Some(format!(
                                                "{} (partial deletion: {} reclaimed before failure)",
                                                e,
                                                human_bytes(item_bytes_reported.get())
                                            )),
                                            blake3_hash: None,
                                            bytes_freed: item_bytes_reported.get(),
                                            reversibility: item.reversibility,
                                            }
                                        }
                                    }
                                }
                                // Unreachable: `classify_nonmutating_outcome` above always
                                // returns `Some(..)` for every Github* kind (regardless of
                                // `path_exists`), so control never reaches this match arm
                                // with one of those variants. Kept as a loud panic rather
                                // than a silent `_` arm, so a future `PlanItemKind` handled
                                // in only one of the two functions fails fast instead of
                                // quietly diverging again (the exact bug this refactor
                                // closed for dry-run vs execute).
                                PlanItemKind::GithubRepo
                                | PlanItemKind::GithubBranch
                                | PlanItemKind::GithubRun
                                | PlanItemKind::GithubRelease
                                | PlanItemKind::GithubCache
                                | PlanItemKind::GithubIssue
                                | PlanItemKind::GithubPr
                                | PlanItemKind::GithubReleaseAsset => unreachable!(
                                    "classify_nonmutating_outcome must handle all Github* kinds"
                                ),
                            }
                        };
                        // Top off this item's slot on the bar to its full planned
                        // weight. For File/GitHub/missing-path arms this is the
                        // entire `pb.inc` call for the item; for Dir it makes up
                        // whatever the per-file callback did not already report
                        // (e.g. a failed/partial delete, or physical vs. logical
                        // size differences), so a single item never gets
                        // double-counted and the bar still reaches its total.
                        let reported = item_bytes_reported.get();
                        if item.bytes > reported {
                            pb.inc(item.bytes - reported);
                        }
                        res
                    })
                    .collect()
            };
            let mut results: Vec<DeletionResult> = match max_concurrent {
                Some(n) if n > 0 => rayon::ThreadPoolBuilder::new()
                    .num_threads(n)
                    .build()
                    .map_err(|e| anyhow::anyhow!("could not build a {n}-thread pool: {e}"))?
                    .install(delete_body),
                _ => delete_body(),
            };
            results.extend(nested_results);

            pb.finish_with_message("Deletion execution complete.");

            let end_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // Sample free space once after execution; reuse for both the receipt
            // REALITY law and the printed delta below.
            let available_after: Option<u64> =
                volume_space(&volume_probe_path).ok().map(|v| v.available);

            let receipt = DeletionReceipt::new(
                plan.created_unix,
                start_time,
                end_time,
                results,
                space_before.map(|v| v.available),
                available_after,
            );
            let serialized_receipt = serde_json::to_string_pretty(&receipt)?;
            let receipt_write =
                write_or_dump_on_full(&receipt_path, &serialized_receipt, "deletion receipt")?;

            // Emit a sealed affidavit core/v1 provenance receipt alongside the
            // deletion receipt and certify it. Increasing destructive power
            // (the deletion just performed) must come with increased receipts.
            let affidavit_receipt =
                crate::domain::affidavit_integration::build_deletion_affidavit(&receipt)?;
            let verdict = crate::domain::affidavit_integration::certify(&affidavit_receipt);
            let affidavit_path = receipt_path.with_extension("affidavit.json");
            let affidavit_json = String::from_utf8(
                crate::domain::affidavit_integration::serialize_receipt(&affidavit_receipt),
            )
            .unwrap_or_default();
            let affidavit_write =
                write_or_dump_on_full(&affidavit_path, &affidavit_json, "affidavit receipt")?;

            let mut deleted_count = 0;
            let mut skipped_count = 0;
            let mut failed_count = 0;
            let mut bytes_freed_total: u64 = 0;
            let mut failures = Vec::new();
            let mut non_reversible_deleted = 0usize;

            for r in &receipt.execution_record.results {
                bytes_freed_total += r.bytes_freed;
                if r.status == DeletionStatus::Deleted
                    && r.reversibility != Reversibility::Reversible
                {
                    non_reversible_deleted += 1;
                }
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
            if non_reversible_deleted > 0 {
                println!(
                    "  Deleted with non-Reversible classification: {} (see `reversibility` field in the receipt)",
                    non_reversible_deleted
                );
            }
            // Measured, not planned: each result's `bytes_freed` is now the
            // real size stat'd/summed at delete time (see the per-item Dir
            // and File branches above), so this total is a measurement.
            println!("  Freed:       {} (measured)", human_bytes(bytes_freed_total));
            println!("  Elapsed:     {} seconds", end_time - start_time);
            if let (Some(before), Some(after)) =
                (space_before.map(|v| v.available), available_after)
            {
                let actual = after.saturating_sub(before);
                println!("  Actual free-space delta on /: {}", human_bytes(actual));
            }
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

            match receipt_write {
                WriteOutcome::Written => {
                    println!("\nReceipt written to: {}", receipt_path.display());
                }
                WriteOutcome::DumpedToStdout => {
                    println!(
                        "\n⚠️  Receipt could NOT be written to disk — dumped to stdout above, no file exists at {}",
                        receipt_path.display()
                    );
                }
            }
            match affidavit_write {
                WriteOutcome::Written => {
                    println!(
                        "Affidavit receipt written to: {} (core/v1 chain {}, certify: {})",
                        affidavit_path.display(),
                        affidavit_receipt.chain_hash,
                        if verdict.accepted { "✅ ACCEPTED" } else { "❌ REJECTED" }
                    );
                }
                WriteOutcome::DumpedToStdout => {
                    println!(
                        "⚠️  Affidavit receipt could NOT be written to disk — dumped to stdout above, no file exists at {} (core/v1 chain {}, certify: {})",
                        affidavit_path.display(),
                        affidavit_receipt.chain_hash,
                        if verdict.accepted { "✅ ACCEPTED" } else { "❌ REJECTED" }
                    );
                }
            }

            if let ReclaimCheck::Shortfall { claimed, measured } =
                check_reclaim(bytes_freed_total, space_before.map(|v| v.available), available_after)
            {
                let measured_unsigned = if measured < 0 { 0 } else { measured as u64 };
                // Non-fatal, matching `DeletionReceipt::verify()`'s treatment of the
                // same `check_reclaim` witness (a `VerificationIssue`, not a hard
                // error). A real, common, non-bug cause of this shortfall on macOS:
                // local APFS/Time Machine snapshots retain the freed blocks until
                // thinned (`tmutil thinlocalsnapshots`), so files can be genuinely,
                // fully deleted (per-item bytes_freed is a real stat/sum at delete
                // time, not an estimate) while `statvfs` free space barely moves.
                // Bailing here previously turned every real, successful deletion
                // into a nonzero exit — which the MCP server reported as "Subprocess
                // 'oclnr delete execute' failed" even though the receipt (written
                // and affidavit-certified above) was completely valid. Warn instead
                // so the caller can inspect the receipt and thin snapshots if
                // needed, rather than losing the whole run to a false negative.
                println!(
                    "\n⚠️  Space verification shortfall: receipt claims {} bytes freed but volume free-space delta measured only {} bytes (floor={:.0}%).",
                    human_bytes(claimed),
                    human_bytes(measured_unsigned),
                    crate::domain::receipt::RECLAIM_TOLERANCE * 100.0
                );
                println!(
                    "    This commonly means local APFS/Time Machine snapshots are retaining the freed blocks — run `oclnr snapshot thin` (or the `snapshot` MCP tool) to reclaim visible free space. The receipt above is still valid: per-item bytes were measured, not estimated."
                );
            }
        }
    }
    Ok(())
}
