//! Docker/Colima prune receipt.
//!
//! Partial closure of a gap disclosed in the `docker` MCP tool's own
//! description: `docker prune`/`colima prune` had no receipt at all — not
//! even a plain JSON record of what was reclaimed, let alone an affidavit
//! seal. This module adds the plain JSON record. It deliberately does
//! **not** add an affidavit seal (BLAKE3 rolling chain via
//! `crate::domain::affidavit_integration`, as `DeletionReceipt` has) —
//! that would require binding a Docker prune to a plan/approval concept
//! Docker cleanup doesn't have (there is no `docker plan approve`), and
//! doing that properly is a separate piece of work, not a drive-by
//! addition. This receipt is evidence of what happened; it is not yet
//! cryptographically provenance-sealed the way `DeletionReceipt` is.

use serde::{Deserialize, Serialize};

/// A plain, unsealed record of one `docker prune` (+ optional `colima
/// prune`) execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerPruneReceipt {
    pub version: u32,
    pub executed_unix: i64,
    pub images_bytes_before: u64,
    pub images_bytes_after: u64,
    pub containers_bytes_before: u64,
    pub containers_bytes_after: u64,
    pub volumes_bytes_before: u64,
    pub volumes_bytes_after: u64,
    pub build_cache_bytes_before: u64,
    pub build_cache_bytes_after: u64,
    pub total_bytes_before: u64,
    pub total_bytes_after: u64,
    pub reclaimed_bytes: u64,
    /// `None` if Colima prune was skipped or Colima wasn't available;
    /// `Some(true)` if it ran and returned success; `Some(false)` if it ran
    /// and failed (the failure text, if any, is not retained here — see
    /// the CLI's own stderr output for that, this is a summary record).
    pub colima_pruned: Option<bool>,
}

impl DockerPruneReceipt {
    /// Builds a receipt from an [`integration::docker::DockerPruneResult`]-
    /// shaped set of before/after usage figures. Takes the individual
    /// fields rather than the integration-layer struct directly, keeping
    /// this constructor free of any dependency on `integration::docker`
    /// (domain purity: this module has zero `std::fs`/`std::process`, and
    /// depending on an integration-layer type here would blur that
    /// boundary even without I/O).
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::docker_receipt::DockerPruneReceipt;
    ///
    /// let receipt = DockerPruneReceipt::new(
    ///     10_000_000_000, 4_000_000_000, // images before/after
    ///     0, 0,                          // containers before/after
    ///     500_000_000, 100_000_000,      // volumes before/after
    ///     2_000_000_000, 0,              // build cache before/after
    ///     Some(true),
    /// );
    /// assert_eq!(receipt.total_bytes_before, 12_500_000_000);
    /// assert_eq!(receipt.total_bytes_after, 4_100_000_000);
    /// assert_eq!(receipt.reclaimed_bytes, 8_400_000_000);
    /// assert_eq!(receipt.colima_pruned, Some(true));
    ///
    /// // Negative: reclaimed_bytes never underflows even if "after" exceeds
    /// // "before" (e.g. a concurrent `docker pull` ran between samples).
    /// let grew = DockerPruneReceipt::new(
    ///     1_000, 5_000,
    ///     0, 0,
    ///     0, 0,
    ///     0, 0,
    ///     None,
    /// );
    /// assert_eq!(grew.reclaimed_bytes, 0);
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        images_bytes_before: u64,
        images_bytes_after: u64,
        containers_bytes_before: u64,
        containers_bytes_after: u64,
        volumes_bytes_before: u64,
        volumes_bytes_after: u64,
        build_cache_bytes_before: u64,
        build_cache_bytes_after: u64,
        colima_pruned: Option<bool>,
    ) -> Self {
        let total_bytes_before = images_bytes_before
            .saturating_add(containers_bytes_before)
            .saturating_add(volumes_bytes_before)
            .saturating_add(build_cache_bytes_before);
        let total_bytes_after = images_bytes_after
            .saturating_add(containers_bytes_after)
            .saturating_add(volumes_bytes_after)
            .saturating_add(build_cache_bytes_after);
        Self {
            version: 1,
            executed_unix: chrono::Utc::now().timestamp(),
            images_bytes_before,
            images_bytes_after,
            containers_bytes_before,
            containers_bytes_after,
            volumes_bytes_before,
            volumes_bytes_after,
            build_cache_bytes_before,
            build_cache_bytes_after,
            total_bytes_before,
            total_bytes_after,
            reclaimed_bytes: total_bytes_before.saturating_sub(total_bytes_after),
            colima_pruned,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reclaimed_bytes_is_the_before_after_delta() {
        let r = DockerPruneReceipt::new(100, 40, 10, 10, 20, 5, 30, 0, Some(false));
        assert_eq!(r.total_bytes_before, 160);
        assert_eq!(r.total_bytes_after, 55);
        assert_eq!(r.reclaimed_bytes, 105);
    }

    #[test]
    fn colima_not_run_is_none_not_false() {
        // None ("wasn't run") must stay distinct from Some(false) ("ran and
        // failed") — collapsing them would silently misreport a skip as a
        // failure or vice versa.
        let r = DockerPruneReceipt::new(0, 0, 0, 0, 0, 0, 0, 0, None);
        assert_eq!(r.colima_pruned, None);
    }
}
