//! Docker noun implementation.
//!
//! Routes Docker subcommands, formats output, and delegates to
//! `integration::docker` for all subprocess interaction.

use std::path::PathBuf;

use clap::Subcommand;

use crate::{
    domain::{
        docker_host::{
            prune_host_outcome, space_returned_to_host, DockerHostFootprint, PruneHostOutcome,
        },
        docker_receipt::{
            ColimaRecreateReceipt, DockerPruneReceipt, LimaDiskReapEntry, LimaDiskReapReceipt,
        },
    },
    integration::{
        docker::{
            colima_delete_default, colima_fstrim, colima_host_footprint, colima_prune,
            colima_start, docker_disk_usage, docker_host_footprint, docker_prune_preview,
            docker_system_prune, is_colima_available, is_docker_available, lima_disk_entries,
            reap_orphan_lima_disks, volume_free_bytes,
        },
        progress::human_bytes as fmt_bytes,
    },
};

#[derive(Subcommand, Debug)]
pub enum DockerAction {
    /// Scan Docker for disk usage (images, volumes, build cache)
    Scan,
    /// Preview what Docker prune would reclaim
    Plan,
    /// Show Docker disk usage summary
    Summary,
    /// Actually prune Docker (and, unless --skip-colima, Colima's cached VM
    /// assets) to reclaim space. Destructive — requires --confirm.
    Prune {
        /// Required to actually run the prune; without it, this only prints
        /// what would happen (same as `plan`).
        #[arg(long)]
        confirm: bool,
        /// Skip `colima prune` even if Colima is available.
        #[arg(long)]
        skip_colima: bool,
        /// Optional path to write a plain JSON receipt (before/after usage,
        /// reclaimed bytes, whether Colima was pruned). Not affidavit-sealed
        /// — see `domain::docker_receipt` for why.
        #[arg(long)]
        receipt: Option<PathBuf>,
    },
    /// Run `fstrim` inside the Colima VM so freed guest blocks are returned
    /// to the host sparse disk images. Data-preserving (removes no image,
    /// container, or volume); measures host physical bytes before/after.
    /// Requires --confirm.
    Trim {
        /// Required to actually run fstrim.
        #[arg(long)]
        confirm: bool,
    },
    /// List Lima managed disks (`~/.colima/_lima/_disks/<name>/`) with
    /// registry state and orphan classification. Read-only.
    Disks,
    /// Delete orphaned Lima disk directories — named disks `colima delete`
    /// leaves behind with no registry entry and no live `in_use_by` symlink.
    /// Refuses when the `limactl` registry is unreadable (fail-closed:
    /// orphan classification cannot be trusted without it). Destructive —
    /// requires --confirm.
    ReapDisks {
        /// Required to actually delete; without it, this only prints what
        /// would be reaped.
        #[arg(long)]
        confirm: bool,
        /// Optional path to write a plain JSON receipt (which disks, bytes
        /// freed). Not affidavit-sealed — see `domain::docker_receipt`.
        #[arg(long)]
        receipt: Option<PathBuf>,
    },
    /// Recreate the default Colima VM at an explicit size: `colima delete`
    /// the existing instance, reap any orphaned Lima disks the delete leaves
    /// behind, and `colima start` with the requested resources. The missing
    /// piece this exists for: shrinking an over-provisioned VM (e.g. 40 GiB
    /// provisioned against ~1 GiB of real use) requires teardown — Colima
    /// cannot resize a disk in place — and `colima delete` alone leaves the
    /// orphaned `_disks` behind. Destructive — requires --confirm.
    Recreate {
        /// Disk size for the new VM, in GiB.
        #[arg(long)]
        disk_gib: u32,
        /// CPUs for the new VM. [default: 2]
        #[arg(long)]
        cpu: Option<u32>,
        /// Memory for the new VM, in GiB. [default: 4]
        #[arg(long)]
        memory_gib: Option<u32>,
        /// Virtual machine type passed to `colima start --vm-type`
        /// (e.g. `vz`, `qemu`). Omitted when not given.
        #[arg(long)]
        vm_type: Option<String>,
        /// Required to actually tear down and recreate; without it, this
        /// only prints what would happen.
        #[arg(long)]
        confirm: bool,
        /// Skip the orphaned-disk reap between delete and start.
        #[arg(long)]
        skip_reap: bool,
        /// Optional path to write a plain JSON receipt.
        #[arg(long)]
        receipt: Option<PathBuf>,
    },
}

/// Prints a Docker disk usage table and returns `Ok(())`.
fn print_disk_usage() -> anyhow::Result<()> {
    if !is_docker_available() {
        println!("Docker not available or not running.");
        return Ok(());
    }

    let usage = docker_disk_usage()?;

    println!("Docker Disk Usage (VM-internal, per `docker system df`)");
    println!("  Images:      {} ({})", usage.images_count, fmt_bytes(usage.images_bytes));
    println!("  Containers:  {} ({})", usage.containers_count, fmt_bytes(usage.containers_bytes));
    println!("  Volumes:     {} ({})", usage.volumes_count, fmt_bytes(usage.volumes_bytes));
    println!("  Build cache: {} ({})", usage.build_cache_count, fmt_bytes(usage.build_cache_bytes));
    println!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    println!("  Total:            {}", fmt_bytes(usage.total_bytes));

    print_host_footprint(&usage.total_bytes);
    print_colima_footprint(&usage.total_bytes);

    Ok(())
}

/// Prints the host-side `Docker.raw` section: the sparse image's physical
/// allocation (what actually consumes host disk), its logical (sparse)
/// apparent size, and how many host blocks the VM-internal accounting above
/// does not explain. Silent when no image is measurable.
fn print_host_footprint(vm_total_bytes: &u64) {
    if let Some(fp) = docker_host_footprint() {
        println!();
        println!("Host-side footprint (Docker.raw sparse image)");
        println!("  Images found:        {}", fp.docker_raw_count);
        println!(
            "  Physical allocation: {}  <- blocks actually consumed on the host volume",
            fmt_bytes(fp.docker_raw_physical_bytes)
        );
        println!("  Logical (sparse) size: {}", fmt_bytes(fp.docker_raw_logical_bytes));
        let pinned = fp.host_pinned_beyond_vm(*vm_total_bytes);
        println!("  Pinned beyond VM-visible usage: {}", fmt_bytes(pinned));
        if pinned > 0 {
            println!(
                "  Note: this is space `docker system df` cannot see. Freeing it needs the VM-side \
                 prune plus image compaction (Docker Desktop restart or a lower disk-image size \
                 limit) — deleting files inside containers alone will not return it."
            );
        }
    }
}

/// Prints the Colima section: physical/logical bytes of the Lima VM disk
/// images, and how much of the physical allocation the VM-visible Docker
/// usage does not explain (the `docker trim` target).
fn print_colima_footprint(vm_total_bytes: &u64) {
    if let Some(fp) = colima_host_footprint() {
        println!();
        println!("Host-side footprint (Colima/Lima VM disk images)");
        println!("  Images found:        {}", fp.docker_raw_count);
        println!(
            "  Physical allocation: {}  <- blocks actually consumed on the host volume",
            fmt_bytes(fp.docker_raw_physical_bytes)
        );
        println!("  Logical (sparse) size: {}", fmt_bytes(fp.docker_raw_logical_bytes));
        let pinned = fp.host_pinned_beyond_vm(*vm_total_bytes);
        println!("  Pinned beyond VM-visible usage: {}", fmt_bytes(pinned));
        if pinned > 0 {
            println!(
                "  Note: freed-but-untrimmed guest blocks are reclaimable without deleting \
                 anything via `oclnr docker trim --confirm` (guest fstrim)."
            );
        }
    }
}

pub fn handle(action: DockerAction) -> anyhow::Result<()> {
    match action {
        DockerAction::Scan | DockerAction::Summary => print_disk_usage(),
        DockerAction::Plan => {
            if !is_docker_available() {
                println!("Docker not available or not running.");
                return Ok(());
            }

            let preview = docker_prune_preview()?;

            println!("Docker Prune Preview (dry run)");
            println!("  Reclaimable images:      {}", fmt_bytes(preview.images_reclaimable_bytes));
            println!("  Reclaimable volumes:     {}", fmt_bytes(preview.volumes_reclaimable_bytes));
            println!(
                "  Reclaimable build cache: {}",
                fmt_bytes(preview.build_cache_reclaimable_bytes)
            );
            println!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
            println!("  Total reclaimable:       {}", fmt_bytes(preview.total_reclaimable_bytes));
            println!();
            // The prune preview is VM-internal. Say so next to the host
            // footprint so nobody reads "Total reclaimable" as host space
            // they will see after pruning.
            if let Some(fp) = docker_host_footprint() {
                println!(
                    "  Host-side Docker.raw physical allocation: {} (logical/sparse: {})",
                    fmt_bytes(fp.docker_raw_physical_bytes),
                    fmt_bytes(fp.docker_raw_logical_bytes)
                );
                println!(
                    "  Prune frees VM-side usage only — host blocks are returned when Docker \
                     Desktop compacts the image (restart it, or lower its disk-image size)."
                );
            }
            println!();
            println!(
                "Run 'oclnr docker prune --confirm' or 'docker system prune -a --volumes' to \
                 actually reclaim this space."
            );

            Ok(())
        }
        DockerAction::Trim { confirm } => {
            let before = colima_host_footprint();
            let Some(before_fp) = before else {
                println!("No Colima VM disk image found — nothing to trim.");
                return Ok(());
            };
            println!(
                "Colima disk images before trim: {} physical / {} logical",
                fmt_bytes(before_fp.docker_raw_physical_bytes),
                fmt_bytes(before_fp.docker_raw_logical_bytes)
            );
            if !confirm {
                println!("Refusing to run guest fstrim without --confirm.");
                return Ok(());
            }
            let out = colima_fstrim()?;
            println!("Guest fstrim:");
            println!("{out}");
            let after_physical =
                colima_host_footprint().map(|fp| fp.docker_raw_physical_bytes).unwrap_or(0);
            let returned =
                space_returned_to_host(before_fp.docker_raw_physical_bytes, after_physical);
            println!(
                "Host: {} -> {} physical; {} returned to host volume",
                fmt_bytes(before_fp.docker_raw_physical_bytes),
                fmt_bytes(after_physical),
                fmt_bytes(returned)
            );
            Ok(())
        }
        DockerAction::Prune { confirm, skip_colima, receipt } => {
            if !is_docker_available() {
                println!("Docker not available or not running.");
                return Ok(());
            }

            if !confirm {
                println!("Refusing to prune without --confirm. Preview:");
                return handle(DockerAction::Plan);
            }

            // Host-side sample BEFORE pruning, so the receipt can prove what
            // the host volume actually got back (the VM-internal delta alone
            // routinely overstates host reclaim: Docker.raw keeps its blocks
            // until Docker Desktop compacts the image).
            let host_before: Option<DockerHostFootprint> = docker_host_footprint();
            let host_before_physical = host_before.as_ref().map(|fp| fp.docker_raw_physical_bytes);

            let result = docker_system_prune()?;
            println!("Docker Prune");
            println!("  Before: {}", fmt_bytes(result.before.total_bytes));
            println!("  After:  {}", fmt_bytes(result.after.total_bytes));
            println!("  Reclaimed: {}", fmt_bytes(result.reclaimed_bytes));

            let host_after_physical =
                docker_host_footprint().as_ref().map(|fp| fp.docker_raw_physical_bytes);
            let host_returned = match (host_before_physical, host_after_physical) {
                (Some(b), Some(a)) => Some(space_returned_to_host(b, a)),
                _ => None,
            };
            match host_returned {
                Some(returned) => {
                    let still_pinned = host_after_physical
                        .map(|after| after.saturating_sub(result.after.total_bytes))
                        .unwrap_or(0);
                    match prune_host_outcome(result.reclaimed_bytes, returned, still_pinned) {
                        PruneHostOutcome::NothingToReclaim => {
                            println!("  Host: nothing to reclaim this run");
                        }
                        PruneHostOutcome::HostReturned(bytes) => {
                            println!(
                                "  Host: {} returned to host volume (Docker.raw shrank)",
                                fmt_bytes(bytes)
                            );
                        }
                        PruneHostOutcome::HostStillPinned { still_pinned_bytes } => {
                            println!(
                                "  Host: 0 bytes returned — Docker.raw still pins {} of host \
                                 blocks. Restart Docker Desktop (or lower its disk-image size \
                                 limit) to compact the image and release them.",
                                fmt_bytes(still_pinned_bytes)
                            );
                        }
                    }
                }
                None => {
                    println!(
                        "  Host: no Docker.raw image measurable — nothing to verify host-side"
                    );
                }
            }

            let mut colima_pruned: Option<bool> = None;
            if !skip_colima && is_colima_available() {
                match colima_prune() {
                    Ok(out) => {
                        colima_pruned = Some(true);
                        println!("\nColima Prune");
                        if out.is_empty() {
                            println!("  (nothing to prune)");
                        } else {
                            println!("{out}");
                        }
                    }
                    Err(e) => {
                        colima_pruned = Some(false);
                        println!("\nColima prune skipped: {e}");
                    }
                }
            }

            if let Some(receipt_path) = receipt {
                let mut docker_receipt = DockerPruneReceipt::new(
                    result.before.images_bytes,
                    result.after.images_bytes,
                    result.before.containers_bytes,
                    result.after.containers_bytes,
                    result.before.volumes_bytes,
                    result.after.volumes_bytes,
                    result.before.build_cache_bytes,
                    result.after.build_cache_bytes,
                    colima_pruned,
                );
                docker_receipt.with_host_physical(host_before_physical, host_after_physical);
                match serde_json::to_string_pretty(&docker_receipt)
                    .map_err(anyhow::Error::from)
                    .and_then(|json| {
                        std::fs::write(&receipt_path, json).map_err(anyhow::Error::from)
                    }) {
                    Ok(()) => {
                        println!("\nWrote docker prune receipt to: {}", receipt_path.display())
                    }
                    Err(e) => eprintln!("\nwarning: could not write docker prune receipt: {e}"),
                }
            }

            Ok(())
        }
        DockerAction::Disks => print_disks(),
        DockerAction::ReapDisks { confirm, receipt } => reap_disks(confirm, receipt),
        DockerAction::Recreate {
            disk_gib,
            cpu,
            memory_gib,
            vm_type,
            confirm,
            skip_reap,
            receipt,
        } => recreate(
            disk_gib,
            cpu.unwrap_or(2),
            memory_gib.unwrap_or(4),
            vm_type.as_deref(),
            confirm,
            skip_reap,
            receipt,
        ),
    }
}

/// Prints the Lima managed-disk table: registry state, `in_use_by` target,
/// logical vs physical bytes, and the fail-closed orphan classification.
fn print_disks() -> anyhow::Result<()> {
    let (registry_known, entries) = lima_disk_entries()?;
    if !registry_known {
        println!("limactl registry unreadable — orphan classification unavailable (read-only).");
    }
    if entries.is_empty() {
        println!("No Lima managed disks found under ~/.colima/_lima/_disks.");
        return Ok(());
    }
    println!("Lima managed disks (~/.colima/_lima/_disks)");
    for e in &entries {
        let state = if e.orphan {
            "ORPHAN"
        } else if e.registered {
            "registered"
        } else if e.in_use_by.is_some() {
            "in-use"
        } else {
            "unclassified"
        };
        println!(
            "  {:<16} {:>11} logical / {:>11} physical  [{}]{}",
            e.name,
            fmt_bytes(e.datadisk_logical_bytes),
            fmt_bytes(e.datadisk_physical_bytes),
            state,
            e.in_use_by.as_ref().map(|t| format!(" -> {t}")).unwrap_or_default()
        );
    }
    let orphans: u64 = entries.iter().filter(|e| e.orphan).map(|e| e.datadisk_physical_bytes).sum();
    println!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    println!(
        "  Orphaned physical: {} — reclaim with 'oclnr docker reap-disks --confirm'",
        fmt_bytes(orphans)
    );
    Ok(())
}

/// Reaps orphaned Lima disks: prints the plan without `--confirm`, deletes
/// exactly the orphan-classified dirs with it, and writes the plain JSON
/// receipt when a path was given.
fn reap_disks(confirm: bool, receipt: Option<PathBuf>) -> anyhow::Result<()> {
    let (registry_known, entries) = lima_disk_entries()?;
    let orphans: Vec<_> = entries.iter().filter(|e| e.orphan).collect();

    if orphans.is_empty() {
        println!("No orphaned Lima disks — nothing to reap.");
        return Ok(());
    }
    let total: u64 = orphans.iter().map(|e| e.datadisk_physical_bytes).sum();
    println!("Orphaned Lima disks queued for reap:");
    for e in &orphans {
        println!(
            "  {:<16} {} physical ({})",
            e.name,
            fmt_bytes(e.datadisk_physical_bytes),
            e.dir.display()
        );
    }
    println!("  Total physical: {}", fmt_bytes(total));

    if !confirm {
        println!("Refusing to reap without --confirm.");
        return Ok(());
    }

    let reaped = reap_orphan_lima_disks(registry_known, &entries)?;
    let freed: u64 = reaped.iter().map(|r| r.physical_bytes_freed).sum();
    println!("Reaped {} disk(s); {} returned to host volume", reaped.len(), fmt_bytes(freed));

    if let Some(path) = receipt {
        let entries_json = reaped
            .iter()
            .map(|r| LimaDiskReapEntry {
                name: r.name.clone(),
                path: r.path.display().to_string(),
                logical_bytes: r.logical_bytes,
                physical_bytes_freed: r.physical_bytes_freed,
            })
            .collect();
        write_plain_receipt(
            &path,
            &LimaDiskReapReceipt::new(registry_known, entries_json),
            "lima disk reap",
        )?;
    }
    Ok(())
}

/// Recreates the default Colima VM at the requested size. Order matters:
/// sample host state → delete instance → reap orphans (the piece
/// `colima delete` leaves behind) → start the new VM → sample again. Every
/// stage's outcome is printed and, when `receipt` is given, recorded.
#[allow(clippy::too_many_arguments)]
fn recreate(
    disk_gib: u32,
    cpu: u32,
    memory_gib: u32,
    vm_type: Option<&str>,
    confirm: bool,
    skip_reap: bool,
    receipt: Option<PathBuf>,
) -> anyhow::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    let before_free = volume_free_bytes(&home).ok();
    let before_fp = colima_host_footprint();

    println!(
        "Colima recreate plan: cpu={cpu} memory={memory_gib}GiB disk={disk_gib}GiB{}",
        vm_type.map(|v| format!(" vm-type={v}")).unwrap_or_default()
    );
    if let Some(fp) = &before_fp {
        println!(
            "  Existing Colima images: {} physical / {} logical",
            fmt_bytes(fp.docker_raw_physical_bytes),
            fmt_bytes(fp.docker_raw_logical_bytes)
        );
    }
    if let Some(free) = before_free {
        println!("  Volume free before: {}", fmt_bytes(free));
    }
    if !confirm {
        println!(
            "Refusing to recreate without --confirm (this tears down the VM and everything in it)."
        );
        return Ok(());
    }

    let mut rec = ColimaRecreateReceipt::new(
        cpu,
        memory_gib,
        disk_gib,
        vm_type.map(str::to_string),
        None,
        0,
        0,
        false,
    );

    let deleted = colima_delete_default()?;
    rec.instance_deleted = Some(deleted);
    println!("  colima delete: {}", if deleted { "ran" } else { "no instance existed" });

    if !skip_reap {
        let (registry_known, entries) = lima_disk_entries()?;
        let orphans: Vec<_> = entries.iter().filter(|e| e.orphan).collect();
        if orphans.is_empty() {
            println!("  orphaned disks: none found");
        } else {
            let reaped = reap_orphan_lima_disks(registry_known, &entries)?;
            let freed: u64 = reaped.iter().map(|r| r.physical_bytes_freed).sum();
            rec.disks_reaped = reaped.len();
            rec.reaped_physical_bytes_freed = freed;
            println!(
                "  reaped {} orphaned disk(s): {} returned to host volume",
                reaped.len(),
                fmt_bytes(freed)
            );
        }
    } else {
        println!("  orphaned disks: reap skipped (--skip-reap)");
    }

    println!("  starting new VM (this can take a minute)...");
    colima_start(cpu, memory_gib, disk_gib, vm_type)?;
    rec.started = true;
    println!("  colima start: done");

    let after_free = volume_free_bytes(&home).ok();
    let after_fp = colima_host_footprint();
    let after_physical = after_fp.as_ref().map(|fp| fp.docker_raw_physical_bytes);
    rec.with_host_samples(
        before_fp.as_ref().map(|fp| fp.docker_raw_physical_bytes),
        after_physical,
        before_free,
        after_free,
    );

    println!("Colima recreate summary");
    match (before_fp.as_ref().map(|fp| fp.docker_raw_physical_bytes), after_physical) {
        (Some(b), Some(a)) => println!(
            "  Colima physical: {} -> {} ({} returned)",
            fmt_bytes(b),
            fmt_bytes(a),
            fmt_bytes(space_returned_to_host(b, a))
        ),
        _ => println!("  Colima physical: (no images measurable)"),
    }
    if let (Some(b), Some(a)) = (before_free, after_free) {
        println!(
            "  Volume free: {} -> {} (+{})",
            fmt_bytes(b),
            fmt_bytes(a),
            fmt_bytes(a.saturating_sub(b))
        );
    }

    if let Some(path) = receipt {
        write_plain_receipt(&path, &rec, "colima recreate")?;
    }
    Ok(())
}

/// Serializes a plain JSON receipt to `path`, printing success or a warning
/// (never failing the operation the receipt merely describes).
fn write_plain_receipt<T: serde::Serialize>(
    path: &PathBuf,
    receipt: &T,
    label: &str,
) -> anyhow::Result<()> {
    match serde_json::to_string_pretty(receipt)
        .map_err(anyhow::Error::from)
        .and_then(|json| std::fs::write(path, json).map_err(anyhow::Error::from))
    {
        Ok(()) => {
            println!("\nWrote {label} receipt to: {}", path.display());
            Ok(())
        }
        Err(e) => {
            eprintln!("\nwarning: could not write {label} receipt: {e}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_bytes_zero() {
        assert_eq!(fmt_bytes(0), "0.00 B");
    }

    #[test]
    fn fmt_bytes_kilobytes() {
        assert_eq!(fmt_bytes(1_024), "1.02 KB");
    }

    #[test]
    fn fmt_bytes_megabytes() {
        assert_eq!(fmt_bytes(1_048_576), "1.05 MB");
    }

    #[test]
    fn fmt_bytes_gigabytes() {
        assert_eq!(fmt_bytes(1_073_741_824), "1.07 GB");
    }

    #[test]
    fn fmt_bytes_fractional_gb() {
        // 2.5 GiB, rendered via decimal-based human_bytes
        let b = (2.5_f64 * 1_073_741_824_f64) as u64;
        assert_eq!(fmt_bytes(b), "2.68 GB");
    }
}
