//! Snapshot CLI noun implementation.

use std::path::{Path, PathBuf};

use clap::Subcommand;

use crate::{
    domain::{
        ocel::{build_snapshot_audit_ocel, build_snapshot_delete_ocel, build_snapshot_thin_ocel},
        time::{
            parse_size_in_bytes, select_oldest_snapshots, select_snapshots_to_keep_latest,
            SnapshotThinReceipt,
        },
    },
    integration::{
        fs::{volume_space, write_or_dump_on_full, WriteOutcome},
        progress::human_bytes,
        tmutil::{delete_local_snapshot, list_local_snapshots, thin_local_snapshots},
    },
};

/// Seals a [`SnapshotThinReceipt`] into an affidavit core/v1 provenance chain
/// alongside it, using `event_builder` to pick the truthful event type
/// (`snapshot_thin_requested` vs `snapshot_delete_requested`) for the
/// operation that produced it. Increasing destructive power (snapshots just
/// thinned/deleted) must come with increased receipts, same as `delete
/// execute`.
fn seal_snapshot_receipt(
    receipt_obj: &SnapshotThinReceipt,
    receipt_path: &Path,
    event_builder: fn(
        &SnapshotThinReceipt,
    ) -> anyhow::Result<crate::domain::affidavit_integration::Receipt>,
) -> anyhow::Result<()> {
    let affidavit_receipt = event_builder(receipt_obj)?;
    let verdict = crate::domain::affidavit_integration::certify(&affidavit_receipt);
    let affidavit_path = receipt_path.with_extension("affidavit.json");
    let affidavit_json = String::from_utf8(
        crate::domain::affidavit_integration::serialize_receipt(&affidavit_receipt),
    )
    .unwrap_or_default();
    let affidavit_write =
        write_or_dump_on_full(&affidavit_path, &affidavit_json, "affidavit receipt")?;

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
    Ok(())
}

/// Prints the volume's available space, returning it for before/after deltas.
fn report_space(mount: &str) -> Option<u64> {
    match volume_space(Path::new(mount)) {
        Ok(vs) => {
            println!(
                "  {} free of {} ({}% used)",
                human_bytes(vs.available),
                human_bytes(vs.total),
                vs.percent_used()
            );
            Some(vs.available)
        }
        Err(e) => {
            eprintln!("  warning: could not read free space: {}", e);
            None
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum SnapshotAction {
    /// List all local APFS snapshots
    Audit {
        /// Volume mount point (defaults to "/")
        #[arg(long, default_value = "/")]
        mount: String,
        /// Path to write OCEL v2 JSON evidence
        #[arg(long)]
        ocel: Option<PathBuf>,
        /// Redact absolute paths and credential-shaped strings (via
        /// `domain::redaction::redact_content`) before writing `--ocel`.
        #[arg(long)]
        redact: bool,
    },
    /// Thin local APFS snapshots to reclaim bytes
    Thin {
        /// Volume mount point (defaults to "/")
        #[arg(long, default_value = "/")]
        mount: String,
        /// Size to reclaim (e.g. "10GB", "500MB", or raw bytes)
        #[arg(long)]
        bytes: String,
        /// Path to write the thinning receipt
        #[arg(short, long)]
        receipt: Option<PathBuf>,
        /// Path to write OCEL v2 JSON evidence
        #[arg(long)]
        ocel: Option<PathBuf>,
        /// Redact absolute paths and credential-shaped strings (via
        /// `domain::redaction::redact_content`) before writing `--ocel`.
        /// Does not apply to `--receipt` or its sealed affidavit sidecar,
        /// which are content-addressed and would fail chain-hash
        /// verification if redacted after the fact.
        #[arg(long)]
        redact: bool,
    },
    /// Delete specific local APFS snapshots (by name/date, or the oldest N)
    Delete {
        /// Volume mount point (defaults to "/")
        #[arg(long, default_value = "/")]
        mount: String,
        /// Snapshot name or date suffix to delete, "oldest" / "all", or
        /// "keep-latest" (delete every snapshot except the single most
        /// recent — the default posture for any cleaning run per the
        /// user's standing disk-cleanup rule: don't let snapshots
        /// accumulate past what's needed to explain today's state)
        #[arg(long)]
        which: String,
        /// When --which oldest, how many of the oldest snapshots to delete
        #[arg(long, default_value = "1")]
        oldest_n: usize,
        /// Path to write the deletion receipt
        #[arg(short, long)]
        receipt: Option<PathBuf>,
        /// Path to write OCEL v2 JSON evidence
        #[arg(long)]
        ocel: Option<PathBuf>,
        /// Redact absolute paths and credential-shaped strings (via
        /// `domain::redaction::redact_content`) before writing `--ocel`.
        /// Does not apply to `--receipt` or its sealed affidavit sidecar,
        /// which are content-addressed and would fail chain-hash
        /// verification if redacted after the fact.
        #[arg(long)]
        redact: bool,
    },
}

/// The `snapshot thin` operation as a reusable function: list → `tmutil
/// thinlocalsnapshots <mount> <bytes> <urgency>` → list, then write and seal
/// the [`SnapshotThinReceipt`] (affidavit core/v1 sidecar) and optional OCEL
/// log. `snapshot thin` (urgency 1) and the pressure monitor (`monitor
/// --reclaim snapshots`, urgency configurable) both go through this, so a
/// pressure-triggered thin carries exactly the receipts a manual one does.
pub fn thin_and_seal(
    mount: &str,
    parsed_bytes: u64,
    urgency: u8,
    receipt: Option<&Path>,
    ocel: Option<&Path>,
    redact: bool,
    grant: &str,
) -> anyhow::Result<SnapshotThinReceipt> {
    println!(
        "Thinning local snapshots on {} to reclaim {} bytes (urgency {})...",
        mount, parsed_bytes, urgency
    );

    let before = list_local_snapshots(mount)?;
    let output = thin_local_snapshots(mount, parsed_bytes, urgency)?;
    println!("{}", output);

    let after = list_local_snapshots(mount)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let receipt_obj = SnapshotThinReceipt::new(
        mount.to_string(),
        parsed_bytes,
        now,
        before.clone(),
        after.clone(),
    );

    println!("Thinned {} snapshots successfully.", receipt_obj.snapshots_thinned.len());
    for s in &receipt_obj.snapshots_thinned {
        println!("  - {}", s);
    }

    if let Some(r_path) = receipt {
        let serialized = serde_json::to_string_pretty(&receipt_obj)?;
        std::fs::write(r_path, serialized)?;
        println!("Wrote thinning receipt to: {}", r_path.display());

        seal_snapshot_receipt(
            &receipt_obj,
            r_path,
            crate::domain::affidavit_integration::build_snapshot_thin_affidavit,
        )?;

        // Fleet R-projection (identity/authority/consequence/replay/standing)
        // beside the native receipt — see `domain::r_projection`.
        // Work order: the pressure policy when the monitor acted, otherwise the
        // operator's own invocation.
        let work_order = if grant.starts_with("pressure-policy") {
            format!("oclnr-pressure-policy:{mount}")
        } else {
            format!("oclnr-snapshot-thin:operator:{mount}")
        };
        let ctx =
            crate::integration::r_projection::context_for(r_path, "oclnr", grant, &work_order, 0)?;
        let r = crate::domain::r_projection::project_snapshot_thin(&receipt_obj, &ctx);
        let out = crate::integration::r_projection::write_projection(r_path, &r)?;
        println!("R-projection written to: {} (standing {})", out.display(), r.standing.value);
    }

    if let Some(o_path) = ocel {
        let ocel_log = build_snapshot_thin_ocel(
            mount,
            parsed_bytes,
            &before,
            &after,
            &receipt_obj.snapshots_thinned,
        );
        let serialized = serde_json::to_string_pretty(&ocel_log)?;
        let (_outcome, ledger) = crate::integration::fs::write_output_file(
            o_path,
            &serialized,
            redact,
            "snapshot thin OCEL log",
        )?;
        if let Some(ledger) = ledger {
            eprintln!("redacted {} item(s) in {}", ledger.entries.len(), o_path.display());
        }
        println!("Wrote snapshot thin OCEL v2 log to: {}", o_path.display());
    }

    Ok(receipt_obj)
}

pub fn handle(action: SnapshotAction) -> anyhow::Result<()> {
    match action {
        SnapshotAction::Audit { mount, ocel, redact } => {
            println!("Auditing local snapshots for: {}", mount);
            let snapshots = list_local_snapshots(&mount)?;
            println!("Found {} local APFS snapshots:", snapshots.len());
            for s in &snapshots {
                println!("  - {}", s);
            }

            if let Some(o_path) = ocel {
                let ocel_log = build_snapshot_audit_ocel(&mount, &snapshots);
                let serialized = serde_json::to_string_pretty(&ocel_log)?;
                let (_outcome, ledger) = crate::integration::fs::write_output_file(
                    &o_path,
                    &serialized,
                    redact,
                    "snapshot audit OCEL log",
                )?;
                if let Some(ledger) = ledger {
                    eprintln!("redacted {} item(s) in {}", ledger.entries.len(), o_path.display());
                }
                println!("Wrote snapshot audit OCEL v2 log to: {}", o_path.display());
            }
        }
        SnapshotAction::Thin { mount, bytes, receipt, ocel, redact } => {
            let parsed_bytes = parse_size_in_bytes(&bytes)
                .map_err(|e| anyhow::anyhow!("Invalid size format: {}", e))?;
            thin_and_seal(
                &mount,
                parsed_bytes,
                1,
                receipt.as_deref(),
                ocel.as_deref(),
                redact,
                "operator-invocation: `oclnr snapshot thin` run directly by its caller",
            )?;
        }
        SnapshotAction::Delete { mount, which, oldest_n, receipt, ocel, redact } => {
            let before = list_local_snapshots(&mount)?;

            // Resolve `which` into a concrete list of date suffixes to delete.
            let targets: Vec<String> = match which.as_str() {
                "oldest" => select_oldest_snapshots(&before, oldest_n),
                "all" => before
                    .iter()
                    .filter_map(|s| crate::domain::time::parse_snapshot_date(s))
                    .collect(),
                "keep-latest" => select_snapshots_to_keep_latest(&before),
                explicit => vec![explicit.to_string()],
            };

            if targets.is_empty() {
                println!("No matching snapshots to delete for --which {}.", which);
                return Ok(());
            }

            println!("Deleting {} snapshot(s) on {}:", targets.len(), mount);
            println!("Free space before:");
            let before_avail = report_space(&mount);

            for t in &targets {
                println!("  - {}", t);
                let out = delete_local_snapshot(t)?;
                let trimmed = out.trim();
                if !trimmed.is_empty() {
                    println!("    {}", trimmed);
                }
            }

            let after = list_local_snapshots(&mount)?;

            println!("Free space after:");
            let after_avail = report_space(&mount);
            if let (Some(b), Some(a)) = (before_avail, after_avail) {
                println!("Reclaimed: {}", human_bytes(a.saturating_sub(b)));
            }

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            // `requested_bytes` is 0: selective delete is count-driven, not byte-driven.
            let receipt_obj =
                SnapshotThinReceipt::new(mount.clone(), 0, now, before.clone(), after.clone());

            println!("Deleted {} snapshot(s) successfully.", receipt_obj.snapshots_thinned.len());

            if let Some(r_path) = receipt {
                let serialized = serde_json::to_string_pretty(&receipt_obj)?;
                std::fs::write(&r_path, serialized)?;
                println!("Wrote deletion receipt to: {}", r_path.display());

                seal_snapshot_receipt(
                    &receipt_obj,
                    &r_path,
                    crate::domain::affidavit_integration::build_snapshot_delete_affidavit,
                )?;
            }

            if let Some(o_path) = ocel {
                // Truthful event type: a delete is not a thin (see
                // build_snapshot_delete_ocel) — the log must not conflate them.
                let ocel_log = build_snapshot_delete_ocel(
                    &mount,
                    &before,
                    &after,
                    &receipt_obj.snapshots_thinned,
                );
                let serialized = serde_json::to_string_pretty(&ocel_log)?;
                let (_outcome, ledger) = crate::integration::fs::write_output_file(
                    &o_path,
                    &serialized,
                    redact,
                    "snapshot delete OCEL log",
                )?;
                if let Some(ledger) = ledger {
                    eprintln!("redacted {} item(s) in {}", ledger.entries.len(), o_path.display());
                }
                println!("Wrote snapshot delete OCEL v2 log to: {}", o_path.display());
            }
        }
    }
    Ok(())
}
