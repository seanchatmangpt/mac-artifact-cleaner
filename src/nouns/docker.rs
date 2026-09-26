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
        docker_receipt::DockerPruneReceipt,
    },
    integration::{
        docker::{
            colima_fstrim, colima_host_footprint, colima_prune, docker_disk_usage,
            docker_host_footprint, docker_prune_preview, docker_system_prune, is_colima_available,
            is_docker_available,
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
