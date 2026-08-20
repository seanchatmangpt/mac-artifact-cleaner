//! Docker noun implementation.
//!
//! Routes Docker subcommands, formats output, and delegates to
//! `integration::docker` for all subprocess interaction.

use std::path::PathBuf;

use clap::Subcommand;

use crate::{
    domain::docker_receipt::DockerPruneReceipt,
    integration::{
        docker::{
            colima_prune, docker_disk_usage, docker_prune_preview, docker_system_prune,
            is_colima_available, is_docker_available,
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
}

/// Prints a Docker disk usage table and returns `Ok(())`.
fn print_disk_usage() -> anyhow::Result<()> {
    if !is_docker_available() {
        println!("Docker not available or not running.");
        return Ok(());
    }

    let usage = docker_disk_usage()?;

    println!("Docker Disk Usage");
    println!("  Images:      {} ({})", usage.images_count, fmt_bytes(usage.images_bytes));
    println!("  Containers:  {} ({})", usage.containers_count, fmt_bytes(usage.containers_bytes));
    println!("  Volumes:     {} ({})", usage.volumes_count, fmt_bytes(usage.volumes_bytes));
    println!("  Build cache: {} ({})", usage.build_cache_count, fmt_bytes(usage.build_cache_bytes));
    println!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    println!("  Total:            {}", fmt_bytes(usage.total_bytes));

    Ok(())
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
            println!(
                "Run 'oclnr docker prune --confirm' or 'docker system prune -a --volumes' to \
                 actually reclaim this space."
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

            let result = docker_system_prune()?;
            println!("Docker Prune");
            println!("  Before: {}", fmt_bytes(result.before.total_bytes));
            println!("  After:  {}", fmt_bytes(result.after.total_bytes));
            println!("  Reclaimed: {}", fmt_bytes(result.reclaimed_bytes));

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
                let docker_receipt = DockerPruneReceipt::new(
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
