//! Time-based helpers and snapshot logic.
//!
//! Exposes structures and algorithms for parsing APFS snapshots,
//! querying Time Machine exclusions, and generating thinning receipts.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Converts SystemTime to a Unix timestamp.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::time::system_time_to_unix;
/// use std::time::{UNIX_EPOCH, Duration};
///
/// // Positive case: epoch corresponds to 0
/// assert_eq!(system_time_to_unix(UNIX_EPOCH), 0);
///
/// // Refusal case: time before UNIX_EPOCH is clamped to 0
/// let before_epoch = UNIX_EPOCH - Duration::from_secs(10);
/// assert_eq!(system_time_to_unix(before_epoch), 0);
/// ```
pub fn system_time_to_unix(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

/// Converts seconds to days.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::time::seconds_to_days;
///
/// // Positive case: 24 hours corresponds to 1 day
/// assert_eq!(seconds_to_days(86400), 1);
///
/// // Refusal case: negative seconds are clamped to 0
/// assert_eq!(seconds_to_days(-1000), 0);
/// ```
pub fn seconds_to_days(seconds: i64) -> i64 {
    seconds.max(0) / 86_400
}

/// Extracts the date string from a local snapshot name if it follows the standard pattern.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::time::parse_snapshot_date;
///
/// // Positive case
/// assert_eq!(
///     parse_snapshot_date("com.apple.TimeMachine.2026-05-26-135630.local"),
///     Some("2026-05-26-135630".to_string())
/// );
///
/// // Refusal case
/// assert_eq!(parse_snapshot_date("invalid-snapshot-name"), None);
/// ```
pub fn parse_snapshot_date(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split('.').collect();
    for part in parts {
        if part.len() == 17 && part.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Some(part.to_string());
        }
    }
    None
}

/// Compares two lists of snapshot names to identify which ones were thinned (removed).
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::time::identify_thinned_snapshots;
///
/// let before = vec![
///     "com.apple.TimeMachine.2026-05-26-135630.local".to_string(),
///     "com.apple.TimeMachine.2026-05-26-140000.local".to_string(),
/// ];
/// let after = vec![
///     "com.apple.TimeMachine.2026-05-26-140000.local".to_string(),
/// ];
///
/// // Positive case: one snapshot thinned
/// let thinned = identify_thinned_snapshots(&before, &after);
/// assert_eq!(thinned, vec!["com.apple.TimeMachine.2026-05-26-135630.local".to_string()]);
///
/// // Refusal case: no snapshots thinned
/// let thinned_none = identify_thinned_snapshots(&before, &before);
/// assert!(thinned_none.is_empty());
/// ```
pub fn identify_thinned_snapshots(before: &[String], after: &[String]) -> Vec<String> {
    let after_set: std::collections::HashSet<&String> = after.iter().collect();
    before
        .iter()
        .filter(|s| !after_set.contains(s))
        .cloned()
        .collect()
}

/// Parses a human-readable size string (e.g. "10GB", "500MB") into raw bytes.
/// Supports units: B, KB, MB, GB, TB (case-insensitive). Defaults to raw bytes if no unit is found.
///
/// # Examples
///
/// ```
/// use mac_artifact_cleaner::domain::time::parse_size_in_bytes;
///
/// // Positive cases
/// assert_eq!(parse_size_in_bytes("10GB"), Ok(10_000_000_000));
/// assert_eq!(parse_size_in_bytes("500mb"), Ok(500_000_000));
/// assert_eq!(parse_size_in_bytes("2048"), Ok(2048));
///
/// // Refusal cases
/// assert!(parse_size_in_bytes("invalid_size").is_err());
/// assert!(parse_size_in_bytes("").is_err());
/// assert!(parse_size_in_bytes("abcGB").is_err());
/// ```
pub fn parse_size_in_bytes(s: &str) -> Result<u64, String> {
    let trimmed = s.trim().to_lowercase();
    if trimmed.is_empty() {
        return Err("Empty size string".to_string());
    }

    let mut unit_idx = trimmed.len();
    for (i, c) in trimmed.char_indices() {
        if !c.is_numeric() && c != '.' {
            unit_idx = i;
            break;
        }
    }

    let val_str = trimmed[..unit_idx].trim();
    let unit_str = trimmed[unit_idx..].trim();

    let val: f64 = val_str
        .parse()
        .map_err(|e| format!("Invalid number: {}", e))?;

    // macOS Finder / disk utilities align to SI decimal units (1000 base)
    let multiplier = match unit_str {
        "" | "b" => 1u64,
        "kb" | "k" => 1_000u64,
        "mb" | "m" => 1_000_000u64,
        "gb" | "g" => 1_000_000_000u64,
        "tb" | "t" => 1_000_000_000_000u64,
        _ => return Err(format!("Unknown unit: {}", unit_str)),
    };

    Ok((val * multiplier as f64) as u64)
}

/// Represents the terminal receipt of a local snapshot thinning execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotThinReceipt {
    pub volume: String,
    pub requested_bytes: u64,
    pub timestamp_unix: i64,
    pub snapshots_before: Vec<String>,
    pub snapshots_after: Vec<String>,
    pub snapshots_thinned: Vec<String>,
}

impl SnapshotThinReceipt {
    /// Creates a new receipt for snapshot thinning.
    ///
    /// # Examples
    ///
    /// ```
    /// use mac_artifact_cleaner::domain::time::SnapshotThinReceipt;
    ///
    /// // Positive case: snapshot was thinned
    /// let receipt = SnapshotThinReceipt::new(
    ///     "/".to_string(),
    ///     1000000,
    ///     1716768000,
    ///     vec!["snap1".to_string(), "snap2".to_string()],
    ///     vec!["snap2".to_string()],
    /// );
    /// assert_eq!(receipt.snapshots_thinned, vec!["snap1".to_string()]);
    ///
    /// // Refusal case: no snapshots thinned
    /// let receipt_none = SnapshotThinReceipt::new(
    ///     "/".to_string(),
    ///     1000000,
    ///     1716768000,
    ///     vec!["snap1".to_string()],
    ///     vec!["snap1".to_string()],
    /// );
    /// assert!(receipt_none.snapshots_thinned.is_empty());
    /// ```
    pub fn new(
        volume: String,
        requested_bytes: u64,
        timestamp_unix: i64,
        snapshots_before: Vec<String>,
        snapshots_after: Vec<String>,
    ) -> Self {
        let snapshots_thinned = identify_thinned_snapshots(&snapshots_before, &snapshots_after);
        Self {
            volume,
            requested_bytes,
            timestamp_unix,
            snapshots_before,
            snapshots_after,
            snapshots_thinned,
        }
    }
}
