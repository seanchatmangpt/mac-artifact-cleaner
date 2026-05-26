//! Disk auditing statistics.

use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::Mutex;

/// Tracks counts and total sizes observed during auditing.
pub struct Stats {
    pub files_seen: AtomicUsize,
    pub dirs_seen: AtomicUsize,
    pub bytes_seen: AtomicU64,
    pub projects_seen: AtomicUsize,
    pub candidates_seen: AtomicUsize,
    pub pruned_dirs: AtomicUsize,
    pub errors: AtomicUsize,
    pub phase: Mutex<String>,
    pub error_details: Mutex<Vec<(std::path::PathBuf, String)>>,
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

impl Stats {
    /// Creates a new Stats container initialized with zeroed counters.
    ///
    /// # Examples
    ///
    /// ```
    /// use mac_artifact_cleaner::domain::audit::Stats;
    /// use std::sync::atomic::Ordering;
    ///
    /// // Positive case: verify counters are initialized to zero
    /// let stats = Stats::new();
    /// assert_eq!(stats.files_seen.load(Ordering::Relaxed), 0);
    /// assert_eq!(stats.dirs_seen.load(Ordering::Relaxed), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            files_seen: AtomicUsize::new(0),
            dirs_seen: AtomicUsize::new(0),
            bytes_seen: AtomicU64::new(0),
            projects_seen: AtomicUsize::new(0),
            candidates_seen: AtomicUsize::new(0),
            pruned_dirs: AtomicUsize::new(0),
            errors: AtomicUsize::new(0),
            phase: Mutex::new("initialized".to_string()),
            error_details: Mutex::new(Vec::new()),
        }
    }
}
