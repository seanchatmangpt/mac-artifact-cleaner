//! Snapshot CLI noun implementation.

use crate::domain::ocel::{
    build_snapshot_audit_ocel, build_snapshot_delete_ocel, build_snapshot_thin_ocel,
};
use crate::domain::time::{parse_size_in_bytes, select_oldest_snapshots, SnapshotThinReceipt};
use crate::integration::fs::volume_space;
use crate::integration::progress::human_bytes;
use crate::integration::tmutil::{
    delete_local_snapshot, list_local_snapshots, thin_local_snapshots,
};
use clap::Subcommand;
use std::path::{Path, PathBuf};

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
    },
    /// Delete specific local APFS snapshots (by name/date, or the oldest N)
    Delete {
        /// Volume mount point (defaults to "/")
        #[arg(long, default_value = "/")]
        mount: String,
        /// Snapshot name or date suffix to delete, or "oldest" / "all"
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
    },
}

pub fn handle(action: SnapshotAction) -> anyhow::Result<()> {
    match action {
        SnapshotAction::Audit { mount, ocel } => {
            println!("Auditing local snapshots for: {}", mount);
            let snapshots = list_local_snapshots(&mount)?;
            println!("Found {} local APFS snapshots:", snapshots.len());
            for s in &snapshots {
                println!("  - {}", s);
            }

            if let Some(o_path) = ocel {
                let ocel_log = build_snapshot_audit_ocel(&mount, &snapshots);
                let serialized = serde_json::to_string_pretty(&ocel_log)?;
                std::fs::write(&o_path, serialized)?;
                println!("Wrote snapshot audit OCEL v2 log to: {}", o_path.display());
            }
        }
        SnapshotAction::Thin {
            mount,
            bytes,
            receipt,
            ocel,
        } => {
            let parsed_bytes = parse_size_in_bytes(&bytes)
                .map_err(|e| anyhow::anyhow!("Invalid size format: {}", e))?;

            println!(
                "Thinning local snapshots on {} to reclaim {} bytes...",
                mount, parsed_bytes
            );

            let before = list_local_snapshots(&mount)?;
            let output = thin_local_snapshots(&mount, parsed_bytes, 1)?;
            println!("{}", output);

            let after = list_local_snapshots(&mount)?;

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            let receipt_obj = SnapshotThinReceipt::new(
                mount.clone(),
                parsed_bytes,
                now,
                before.clone(),
                after.clone(),
            );

            println!(
                "Thinned {} snapshots successfully.",
                receipt_obj.snapshots_thinned.len()
            );
            for s in &receipt_obj.snapshots_thinned {
                println!("  - {}", s);
            }

            if let Some(r_path) = receipt {
                let serialized = serde_json::to_string_pretty(&receipt_obj)?;
                std::fs::write(&r_path, serialized)?;
                println!("Wrote thinning receipt to: {}", r_path.display());
            }

            if let Some(o_path) = ocel {
                let ocel_log = build_snapshot_thin_ocel(
                    &mount,
                    parsed_bytes,
                    &before,
                    &after,
                    &receipt_obj.snapshots_thinned,
                );
                let serialized = serde_json::to_string_pretty(&ocel_log)?;
                std::fs::write(&o_path, serialized)?;
                println!("Wrote snapshot thin OCEL v2 log to: {}", o_path.display());
            }
        }
        SnapshotAction::Delete {
            mount,
            which,
            oldest_n,
            receipt,
            ocel,
        } => {
            let before = list_local_snapshots(&mount)?;

            // Resolve `which` into a concrete list of date suffixes to delete.
            let targets: Vec<String> = match which.as_str() {
                "oldest" => select_oldest_snapshots(&before, oldest_n),
                "all" => before
                    .iter()
                    .filter_map(|s| crate::domain::time::parse_snapshot_date(s))
                    .collect(),
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

            println!(
                "Deleted {} snapshot(s) successfully.",
                receipt_obj.snapshots_thinned.len()
            );

            if let Some(r_path) = receipt {
                let serialized = serde_json::to_string_pretty(&receipt_obj)?;
                std::fs::write(&r_path, serialized)?;
                println!("Wrote deletion receipt to: {}", r_path.display());
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
                std::fs::write(&o_path, serialized)?;
                println!("Wrote snapshot delete OCEL v2 log to: {}", o_path.display());
            }
        }
    }
    Ok(())
}
