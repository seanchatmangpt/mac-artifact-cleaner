//! Homebrew package manager integration layer.

use std::process::Command;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrewCleanupPreview {
    pub stale_formulae: Vec<String>,
    pub stale_casks: Vec<String>,
    pub cache_files: Vec<BrewCacheEntry>,
    pub reclaimable_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrewCacheEntry {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrewAutoremovePreview {
    pub orphan_formulae: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrewCacheSize {
    pub path: String,
    pub bytes: u64,
}

/// Returns true if `brew` is available on PATH.
pub fn is_brew_available() -> bool {
    Command::new("which").arg("brew").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Runs `brew cleanup --dry-run` and parses the output into a [`BrewCleanupPreview`].
///
/// Lines starting with "Would remove:" are treated as cache file entries.
/// A summary line of the form "This operation would free X bytes of disk space."
/// is parsed for [`BrewCleanupPreview::reclaimable_bytes`].
pub fn brew_cleanup_dry_run() -> Result<BrewCleanupPreview> {
    let output = Command::new("brew").args(["cleanup", "--dry-run"]).output()?;

    // brew writes dry-run output to stderr; merge both streams (we ran with 2>&1 equivalent)
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    let mut cache_files: Vec<BrewCacheEntry> = Vec::new();
    let mut reclaimable_bytes: u64 = 0;

    for line in combined.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("Would remove: ") {
            cache_files.push(BrewCacheEntry { path: rest.trim().to_string(), bytes: 0 });
            continue;
        }

        // "This operation would free XXX bytes of disk space."
        // brew may also say "XXX MB" or "XXX GB" — normalise to bytes.
        if trimmed.contains("would free") && trimmed.contains("disk space") {
            reclaimable_bytes = parse_free_bytes(trimmed);
        }
    }

    Ok(BrewCleanupPreview {
        stale_formulae: vec![],
        stale_casks: vec![],
        cache_files,
        reclaimable_bytes,
    })
}

/// Runs `brew autoremove --dry-run` and returns the list of orphaned formulae.
pub fn brew_autoremove_dry_run() -> Result<BrewAutoremovePreview> {
    let output = Command::new("brew").args(["autoremove", "--dry-run"]).output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    let orphan_formulae: Vec<String> = combined
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("==>") && !l.starts_with("Warning:"))
        .map(|l| l.to_string())
        .collect();

    Ok(BrewAutoremovePreview { orphan_formulae })
}

/// Returns the size of the Homebrew cache directory at `~/Library/Caches/Homebrew`.
pub fn brew_cache_size() -> Result<BrewCacheSize> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
    let cache_path = format!("{}/Library/Caches/Homebrew", home);

    if !std::path::Path::new(&cache_path).exists() {
        return Ok(BrewCacheSize { path: "~/Library/Caches/Homebrew".to_string(), bytes: 0 });
    }

    // `du -sk` reports kilobytes; multiply by 1024 to get bytes.
    let output = Command::new("du").args(["-sk", &cache_path]).output()?;

    let text = String::from_utf8_lossy(&output.stdout);
    let bytes = text
        .split_whitespace()
        .next()
        .and_then(|kb| kb.parse::<u64>().ok())
        .unwrap_or(0)
        .saturating_mul(1024);

    Ok(BrewCacheSize { path: cache_path, bytes })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse a brew summary line like "This operation would free 2.30 GB of disk space."
/// Returns the value in bytes (u64), or 0 if unparsable.
///
/// This is a thin alias over the single shared implementation in
/// [`crate::integration::progress::parse_human_size`].
fn parse_free_bytes(line: &str) -> u64 {
    crate::integration::progress::parse_human_size(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_free_bytes_gb() {
        let line = "This operation would free 2.30 GB of disk space.";
        let bytes = parse_free_bytes(line);
        // 2.30 * 1024^3 ≈ 2_469_606_195
        assert!(bytes > 2_000_000_000, "expected > 2 GB, got {}", bytes);
    }

    #[test]
    fn parse_free_bytes_mb() {
        let line = "This operation would free 512 MB of disk space.";
        let bytes = parse_free_bytes(line);
        assert_eq!(bytes, 512 * 1_024 * 1_024);
    }

    #[test]
    fn parse_free_bytes_unknown_unit() {
        let line = "This operation would free 512 QUUX of disk space.";
        assert_eq!(parse_free_bytes(line), 0);
    }

    #[test]
    fn brew_cleanup_dry_run_parses_would_remove() {
        // Simulate the kind of output brew produces.
        let fake_output = "\
Would remove: /Users/john/Library/Caches/Homebrew/downloads/foo-1.0.tar.gz
Would remove: /Users/john/Library/Caches/Homebrew/downloads/bar-2.1.tar.gz
This operation would free 1.50 GB of disk space.
";
        let mut cache_files: Vec<BrewCacheEntry> = Vec::new();
        let mut reclaimable_bytes: u64 = 0;
        for line in fake_output.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Would remove: ") {
                cache_files.push(BrewCacheEntry { path: rest.trim().to_string(), bytes: 0 });
            } else if trimmed.contains("would free") && trimmed.contains("disk space") {
                reclaimable_bytes = parse_free_bytes(trimmed);
            }
        }
        assert_eq!(cache_files.len(), 2);
        assert!(reclaimable_bytes > 1_000_000_000);
    }
}
