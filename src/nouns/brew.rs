//! Homebrew package manager CLI noun.

use clap::Subcommand;

use crate::integration::{
    brew::{brew_autoremove_dry_run, brew_cache_size, brew_cleanup_dry_run, is_brew_available},
    progress::human_bytes as format_bytes,
};

#[derive(Subcommand, Debug)]
pub enum BrewAction {
    /// Scan Homebrew for cleanup opportunities (dry-run)
    Scan,
    /// Show orphaned/unused formula that can be autoremoved
    Orphans,
    /// Show Homebrew cache size
    Cache,
}

pub fn handle(action: BrewAction) -> anyhow::Result<()> {
    match action {
        BrewAction::Scan => {
            if !is_brew_available() {
                println!("Homebrew not found. Install from https://brew.sh");
                return Ok(());
            }

            let preview = brew_cleanup_dry_run()?;
            let cache = brew_cache_size()?;

            let reclaimable_display = format_bytes(preview.reclaimable_bytes);
            let cache_display = format_bytes(cache.bytes);

            println!("Homebrew Cleanup Preview (dry run)");
            println!("  Cache files to remove: {}", preview.cache_files.len());
            println!("  Reclaimable:           {}", reclaimable_display);
            println!();
            println!(
                "  Cache directory: {} ({} total)",
                friendly_cache_path(&cache.path),
                cache_display
            );
            println!();
            println!("Run 'brew cleanup' to actually remove old versions and cache files.");
        }

        BrewAction::Orphans => {
            if !is_brew_available() {
                println!("Homebrew not found. Install from https://brew.sh");
                return Ok(());
            }

            let preview = brew_autoremove_dry_run()?;

            if preview.orphan_formulae.is_empty() {
                println!("No orphaned formulae found.");
            } else {
                println!("Orphaned Homebrew Formulae (unused dependencies)");
                for formula in &preview.orphan_formulae {
                    println!("  - {}", formula);
                }
                println!();
                println!("Run 'brew autoremove' to remove these.");
            }
        }

        BrewAction::Cache => {
            let cache = brew_cache_size()?;
            println!(
                "Homebrew cache: {} ({})",
                friendly_cache_path(&cache.path),
                format_bytes(cache.bytes)
            );
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

/// Replace the home directory prefix with `~` for display.
fn friendly_cache_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(stripped) = path.strip_prefix(&home) {
            return format!("~{}", stripped);
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_gb() {
        assert_eq!(format_bytes(2_469_606_195), "2.47 GB");
    }

    #[test]
    fn format_bytes_mb() {
        assert_eq!(format_bytes(536_870_912), "536.87 MB");
    }

    #[test]
    fn format_bytes_kb() {
        assert_eq!(format_bytes(2_048), "2.05 KB");
    }

    #[test]
    fn format_bytes_b() {
        assert_eq!(format_bytes(512), "512.00 B");
    }

    #[test]
    fn friendly_cache_path_replaces_home() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/test".to_string());
        let full = format!("{}/Library/Caches/Homebrew", home);
        let friendly = friendly_cache_path(&full);
        assert!(friendly.starts_with('~'), "expected ~-prefixed path, got {}", friendly);
    }
}
