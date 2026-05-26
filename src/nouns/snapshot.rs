//! Snapshot CLI noun implementation.

use crate::domain::ocel::{build_snapshot_audit_ocel, build_snapshot_thin_ocel};
use crate::domain::time::{parse_size_in_bytes, SnapshotThinReceipt};
use crate::integration::tmutil::{list_local_snapshots, thin_local_snapshots};
use clap::Subcommand;
use std::path::PathBuf;

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
    }
    Ok(())
}
