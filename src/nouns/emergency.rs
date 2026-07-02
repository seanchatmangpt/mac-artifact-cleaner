//! Emergency low-disk reclaim CLI noun.
//!
//! **Noun layer rule**: this module parses, routes, and formats output only.
//! All filesystem mutations and OS calls are delegated to `integration`.
//!
//! This path exists for the case the rest of the pipeline cannot handle: the
//! volume is so full that even writing a `cleanup-plan.json` or a receipt fails
//! with `ENOSPC`. It therefore:
//!   * requires **no** file outputs (everything streams to stdout; a receipt is
//!     written only if `--receipt` is given and space allows afterwards),
//!   * reclaims the cheapest, biggest wins first (APFS snapshots — which on a
//!     snapshot-based boot volume hold the blocks of already-deleted files),
//!   * then sweeps a *curated allowlist* of regenerable caches discovered with
//!     cheap `stat`s rather than a full scan.

use std::path::{Path, PathBuf};

use crate::{
    domain::{
        artifact::{global_cache_candidates, is_macos_os_dir},
        receipt::{check_reclaim, ReclaimCheck},
        time::select_oldest_snapshots,
    },
    integration::{
        fs::{delete_dir_all, physical_dir_size, volume_space},
        progress::human_bytes,
        tmutil::{delete_local_snapshot, list_local_snapshots, thin_local_snapshots},
    },
};

/// Prints free space and returns available bytes (for before/after deltas).
fn report_space(mount: &str) -> Option<u64> {
    match volume_space(Path::new(mount)) {
        Ok(vs) => {
            println!(
                "  Disk {}: {} free of {} ({}% used)",
                mount,
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

pub fn handle(mount: String, yes: bool, receipt: Option<PathBuf>) -> anyhow::Result<()> {
    println!("==================================================");
    println!("            EMERGENCY DISK RECLAIM                ");
    println!("  Mode: {}", if yes { "EXECUTE (--yes)" } else { "DRY-RUN (pass --yes to reclaim)" });
    println!("==================================================");

    println!("Starting free space:");
    let start_avail = report_space(&mount);

    // ── Step 1: APFS local snapshots ────────────────────────────────────────
    // On a snapshot-based boot volume this is the highest-leverage action and
    // needs no large temp writes — exactly what we want at ~0 bytes free.
    let snaps = list_local_snapshots(&mount).unwrap_or_default();
    println!("\n[1] Local APFS snapshots: {} present", snaps.len());
    if yes && !snaps.is_empty() {
        // Ask macOS to thin as much as it can (huge byte target, max urgency).
        match thin_local_snapshots(&mount, u64::MAX, 4) {
            Ok(out) if !out.trim().is_empty() => println!("    {}", out.trim()),
            Ok(_) => {}
            Err(e) => eprintln!("    thin failed: {}", e),
        }
        // Then delete any that survived, oldest first.
        let remaining = list_local_snapshots(&mount).unwrap_or_default();
        for date in select_oldest_snapshots(&remaining, remaining.len()) {
            match delete_local_snapshot(&date) {
                Ok(_) => println!("    deleted snapshot {}", date),
                Err(e) => eprintln!("    could not delete {}: {}", date, e),
            }
        }
        println!("    After snapshot reclaim:");
        report_space(&mount);
    } else if !snaps.is_empty() {
        println!("    (dry-run) would thin + delete {} snapshot(s)", snaps.len());
    }

    // ── Step 2: curated cache sweep ─────────────────────────────────────────
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("home directory not found"))?;
    println!("\n[2] Regenerable caches:");
    let mut swept_bytes: u64 = 0;
    for (path, _reason) in global_cache_candidates(&home) {
        if !path.exists() {
            continue;
        }
        // Hard safety: never touch a system directory, whatever the allowlist says.
        if is_macos_os_dir(&path) {
            eprintln!("    refused (system dir): {}", path.display());
            continue;
        }
        let size = physical_dir_size(&path);
        if yes {
            match delete_dir_all(&path) {
                Ok(()) => {
                    swept_bytes += size;
                    println!("    cleared {:>10}  {}", human_bytes(size), path.display());
                }
                Err(e) => eprintln!("    failed {}: {}", path.display(), e),
            }
        } else {
            swept_bytes += size;
            println!("    would clear {:>10}  {}", human_bytes(size), path.display());
        }
    }
    println!("    {} {}", if yes { "Cleared" } else { "Reclaimable" }, human_bytes(swept_bytes));

    // ── Summary ─────────────────────────────────────────────────────────────
    println!("\n==================================================");
    println!("Ending free space:");
    let end_avail = report_space(&mount);
    if let (Some(start), Some(end)) = (start_avail, end_avail) {
        println!("Total reclaimed this run: {}", human_bytes(end.saturating_sub(start)));
    }
    if !yes {
        println!("\nThis was a DRY-RUN. Re-run with --yes to reclaim the space above.");
    }

    // Witness the reclaim claim against the measured volume delta — the SAME law
    // receipt `verify()` applies after `delete`. Only meaningful when we actually
    // deleted (`--yes`); a dry-run claims nothing. `swept_bytes` is a lower bound
    // on what should have been freed (snapshot reclaim is extra), so a Shortfall
    // means even the caches we removed did not return to free space — typically
    // APFS snapshots still pinning the blocks.
    let reclaim = if yes {
        check_reclaim(swept_bytes, start_avail, end_avail)
    } else {
        ReclaimCheck::NotApplicable
    };
    if let ReclaimCheck::Shortfall { claimed, measured } = reclaim {
        println!(
            "⚠️  Reclaim NOT witnessed: cleared {} of caches but free space rose only {} — \
             blocks are likely pinned by APFS snapshots. Run `oclnr snapshot delete --which oldest`.",
            human_bytes(claimed),
            human_bytes(measured.max(0) as u64)
        );
    }
    println!("==================================================");

    // Receipt is best-effort and optional: only attempted now that space may
    // exist again. A write failure here must not mask the reclaim we just did.
    if let Some(r_path) = receipt {
        let body = serde_json::json!({
            "mode": if yes { "execute" } else { "dry-run" },
            "mount": mount,
            "snapshots_seen": snaps.len(),
            "cache_bytes": swept_bytes,
            "start_available": start_avail,
            "end_available": end_avail,
            "reclaim_witnessed": matches!(reclaim, ReclaimCheck::Witnessed),
            "reclaim_shortfall": matches!(reclaim, ReclaimCheck::Shortfall { .. }),
        });
        match serde_json::to_string_pretty(&body)
            .map_err(anyhow::Error::from)
            .and_then(|s| std::fs::write(&r_path, s).map_err(anyhow::Error::from))
        {
            Ok(()) => println!("Wrote emergency receipt to: {}", r_path.display()),
            Err(e) => eprintln!("warning: could not write receipt (disk still tight?): {}", e),
        }
    }

    Ok(())
}
