//! Docker noun implementation.
//!
//! Routes Docker subcommands, formats output, and delegates to
//! `integration::docker` for all subprocess interaction.

use crate::integration::docker::{docker_disk_usage, docker_prune_preview, is_docker_available};
use crate::integration::progress::human_bytes as fmt_bytes;
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum DockerAction {
    /// Scan Docker for disk usage (images, volumes, build cache)
    Scan,
    /// Preview what Docker prune would reclaim
    Plan,
    /// Show Docker disk usage summary
    Summary,
}

/// Prints a Docker disk usage table and returns `Ok(())`.
fn print_disk_usage() -> anyhow::Result<()> {
    if !is_docker_available() {
        println!("Docker not available or not running.");
        return Ok(());
    }

    let usage = docker_disk_usage()?;

    println!("Docker Disk Usage");
    println!(
        "  Images:      {} ({})",
        usage.images_count,
        fmt_bytes(usage.images_bytes)
    );
    println!(
        "  Containers:  {} ({})",
        usage.containers_count,
        fmt_bytes(usage.containers_bytes)
    );
    println!(
        "  Volumes:     {} ({})",
        usage.volumes_count,
        fmt_bytes(usage.volumes_bytes)
    );
    println!(
        "  Build cache: {} ({})",
        usage.build_cache_count,
        fmt_bytes(usage.build_cache_bytes)
    );
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
            println!(
                "  Reclaimable images:      {}",
                fmt_bytes(preview.images_reclaimable_bytes)
            );
            println!(
                "  Reclaimable volumes:     {}",
                fmt_bytes(preview.volumes_reclaimable_bytes)
            );
            println!(
                "  Reclaimable build cache: {}",
                fmt_bytes(preview.build_cache_reclaimable_bytes)
            );
            println!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
            println!(
                "  Total reclaimable:       {}",
                fmt_bytes(preview.total_reclaimable_bytes)
            );
            println!();
            println!("Run 'docker system prune -a --volumes' to actually reclaim this space.");
            println!("(osx-clnr does not execute Docker prune \u{2014} use Docker directly)");

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
