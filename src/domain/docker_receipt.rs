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
    /// Host-side physical allocation of Docker Desktop's sparse disk
    /// image(s), sampled immediately before the prune. `None` when no image
    /// was measurable (Docker Desktop absent / different storage driver) —
    /// distinct from `Some(n)`, which asserts a real sample was taken.
    ///
    /// VM-internal `reclaimed_bytes` alone can overstate host reclaim: the
    /// `Docker.raw` sparse file frequently keeps its blocks until Docker
    /// Desktop compacts it (typically on restart). These two fields let the
    /// receipt prove what the *host* actually got back.
    #[serde(default)]
    pub host_physical_bytes_before: Option<u64>,
    /// Host-side physical allocation after the prune (same sampling rules as
    /// `host_physical_bytes_before`).
    #[serde(default)]
    pub host_physical_bytes_after: Option<u64>,
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
            host_physical_bytes_before: None,
            host_physical_bytes_after: None,
        }
    }

    /// Records host-side `Docker.raw` physical allocation samples on an
    /// already-built receipt, and returns the bytes actually returned to the
    /// host (before − after, saturating; `None` when either sample is
    /// missing).
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::docker_receipt::DockerPruneReceipt;
    ///
    /// let mut receipt = DockerPruneReceipt::new(
    ///     10_000, 4_000, // images before/after
    ///     0, 0, 0, 0, 0, 0,
    ///     None,
    /// );
    ///
    /// // Positive: both samples present → host return is their delta.
    /// let returned = receipt.with_host_physical(Some(129_000), Some(96_000));
    /// assert_eq!(returned, Some(33_000));
    /// assert_eq!(receipt.host_physical_bytes_before, Some(129_000));
    /// assert_eq!(receipt.host_physical_bytes_after, Some(96_000));
    ///
    /// // Negative: a missing sample reports None, never a fabricated 0 delta.
    /// let mut bare = DockerPruneReceipt::new(0, 0, 0, 0, 0, 0, 0, 0, None);
    /// assert_eq!(bare.with_host_physical(None, Some(1)), None);
    /// ```
    pub fn with_host_physical(&mut self, before: Option<u64>, after: Option<u64>) -> Option<u64> {
        self.host_physical_bytes_before = before;
        self.host_physical_bytes_after = after;
        match (before, after) {
            (Some(b), Some(a)) => Some(b.saturating_sub(a)),
            _ => None,
        }
    }
}

/// One reaped Lima disk, as recorded by [`LimaDiskReapReceipt`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimaDiskReapEntry {
    pub name: String,
    pub path: String,
    pub logical_bytes: u64,
    /// Physical blocks actually returned to the host volume, measured
    /// (`st_blocks * 512`) immediately before removal.
    pub physical_bytes_freed: u64,
}

/// A plain, unsealed record of one `docker disks reap` execution: which
/// orphaned Lima disk directories were removed and what the host got back.
/// Same evidence-not-provenance contract as [`DockerPruneReceipt`] — see the
/// module doc for why these Docker-family receipts carry no affidavit seal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimaDiskReapReceipt {
    pub version: u32,
    pub executed_unix: i64,
    /// `false` when `limactl disk list` could not be read. Reaping refuses
    /// to run in that state, so a written receipt with `false` here must not
    /// exist; the field is recorded so a hand-crafted or future-variant
    /// receipt claiming a reap without a readable registry is self-evidently
    /// invalid.
    pub registry_known: bool,
    pub orphans_found: usize,
    pub entries: Vec<LimaDiskReapEntry>,
    pub total_physical_bytes_freed: u64,
}

impl LimaDiskReapReceipt {
    pub fn new(registry_known: bool, entries: Vec<LimaDiskReapEntry>) -> Self {
        let total = entries.iter().map(|e| e.physical_bytes_freed).fold(0u64, u64::saturating_add);
        Self {
            version: 1,
            executed_unix: chrono::Utc::now().timestamp(),
            registry_known,
            orphans_found: entries.len(),
            total_physical_bytes_freed: total,
            entries,
        }
    }
}

/// A plain, unsealed record of one `docker recreate` execution: the
/// requested VM shape, whether the old instance was deleted, how many
/// orphaned disks were reaped on the way, and host-side before/after
/// measurements proving what the operation actually returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColimaRecreateReceipt {
    pub version: u32,
    pub executed_unix: i64,
    pub cpu: u32,
    pub memory_gib: u32,
    pub disk_gib: u32,
    pub vm_type: Option<String>,
    /// `Some(true)` when `colima delete` ran and succeeded; `Some(false)`
    /// when it ran and failed; `None` when no instance existed so delete was
    /// never attempted. Same None-vs-false discipline as
    /// [`DockerPruneReceipt::colima_pruned`].
    pub instance_deleted: Option<bool>,
    pub disks_reaped: usize,
    pub reaped_physical_bytes_freed: u64,
    pub host_physical_bytes_before: Option<u64>,
    pub host_physical_bytes_after: Option<u64>,
    pub volume_free_bytes_before: Option<u64>,
    pub volume_free_bytes_after: Option<u64>,
    pub started: bool,
}

impl ColimaRecreateReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cpu: u32,
        memory_gib: u32,
        disk_gib: u32,
        vm_type: Option<String>,
        instance_deleted: Option<bool>,
        disks_reaped: usize,
        reaped_physical_bytes_freed: u64,
        started: bool,
    ) -> Self {
        Self {
            version: 1,
            executed_unix: chrono::Utc::now().timestamp(),
            cpu,
            memory_gib,
            disk_gib,
            vm_type,
            instance_deleted,
            disks_reaped,
            reaped_physical_bytes_freed,
            host_physical_bytes_before: None,
            host_physical_bytes_after: None,
            volume_free_bytes_before: None,
            volume_free_bytes_after: None,
            started,
        }
    }

    /// Records host-side samples (same sampling rules and None-vs-zero
    /// semantics as [`DockerPruneReceipt::with_host_physical`]).
    pub fn with_host_samples(
        &mut self,
        host_physical_before: Option<u64>,
        host_physical_after: Option<u64>,
        volume_free_before: Option<u64>,
        volume_free_after: Option<u64>,
    ) {
        self.host_physical_bytes_before = host_physical_before;
        self.host_physical_bytes_after = host_physical_after;
        self.volume_free_bytes_before = volume_free_before;
        self.volume_free_bytes_after = volume_free_after;
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
