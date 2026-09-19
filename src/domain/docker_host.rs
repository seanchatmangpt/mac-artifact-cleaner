//! Docker host-side (sparse disk image) accounting.
//!
//! `docker system df` — the only thing the docker noun used to report — sees
//! the world from *inside* the Docker VM: images, volumes, containers, build
//! cache. On macOS, all of that lives inside a sparse host file
//! (`~/Library/Containers/com.docker.docker/Data/vms/<n>/data/Docker.raw`,
//! logical size often 64 GB–1 TB) whose *physical* block allocation is what
//! actually consumes host disk. The two views can diverge by an order of
//! magnitude: on the machine this gap was diagnosed on, `docker system df`
//! reported 20.61 GB while `Docker.raw` pinned 129 GB of host blocks (62% of
//! all reclaimable space). No tool surface measured the second number, and
//! `docker prune` verified nothing on the host volume — a prune can be fully
//! successful VM-side while the host file never returns a block (Docker
//! Desktop compacts lazily, typically on restart).
//!
//! This module is the pure math + DTO layer for closing that gap: the
//! integration layer measures the files, this module computes what the
//! numbers mean. Zero `std::fs`/`std::process` in here.

use serde::{Deserialize, Serialize};

/// Host-side measurement of Docker Desktop's sparse disk image(s).
///
/// `docker_raw_count` is the number of `Docker.raw` files found (normally 1;
/// more than one means multiple VM data dirs). Logical bytes are the file's
/// apparent size (what `ls -l` shows); physical bytes are blocks × 512 — the
/// space actually pinned on the host volume, which is the only number that
/// matters for disk cleanup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerHostFootprint {
    pub docker_raw_count: usize,
    pub docker_raw_logical_bytes: u64,
    pub docker_raw_physical_bytes: u64,
}

impl DockerHostFootprint {
    /// Returns the host blocks pinned by Docker that `docker system df`'s
    /// VM-internal accounting does not explain: `physical - vm_total`,
    /// saturating at zero.
    ///
    /// This is an *observation*, not a reclaim estimate: the delta includes
    /// VM filesystem overhead and free-but-uncompacted space inside the
    /// image, some of which only a Docker Desktop restart (or a smaller
    /// disk-image size setting) returns to the host.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::domain::docker_host::DockerHostFootprint;
    ///
    /// let fp = DockerHostFootprint {
    ///     docker_raw_count: 1,
    ///     docker_raw_logical_bytes: 1_100_000_000_000,
    ///     docker_raw_physical_bytes: 138_000_000_000,
    /// };
    ///
    /// // Positive: the real-machine shape — 129 GB physical, ~21 GB explained
    /// // by VM objects → ~117 GB pinned but unaccounted.
    /// assert_eq!(fp.host_pinned_beyond_vm(21_000_000_000), 117_000_000_000);
    ///
    /// // Negative: VM accounting larger than physical (concurrent prune, or
    /// // thin-provisioned overlap) saturates at 0, never underflows.
    /// assert_eq!(fp.host_pinned_beyond_vm(999_000_000_000), 0);
    /// ```
    pub fn host_pinned_beyond_vm(&self, vm_total_bytes: u64) -> u64 {
        self.docker_raw_physical_bytes.saturating_sub(vm_total_bytes)
    }
}

/// Host-space delta across a destructive operation: how many physical bytes
/// were actually returned to the host volume, saturating at zero (a prune
/// that coincided with other writes can legitimately show no gain).
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::docker_host::space_returned_to_host;
///
/// // Positive: 129 GB → 96 GB after a prune + compaction returned 33 GB.
/// assert_eq!(space_returned_to_host(129_000_000_000, 96_000_000_000), 33_000_000_000);
///
/// // Negative: no shrink (Docker.raw kept its blocks) returns 0, not a
/// // negative number.
/// assert_eq!(space_returned_to_host(129_000_000_000, 129_000_000_000), 0);
/// ```
pub fn space_returned_to_host(before_physical: u64, after_physical: u64) -> u64 {
    before_physical.saturating_sub(after_physical)
}

/// Decides what to tell the operator after a prune, given how much space the
/// host actually got back versus how much the VM said it freed.
///
/// Law: never claim the prune reclaimed host space the host never received.
/// Returns a typed outcome (not a formatted string — formatting lives in the
/// CLI layer) so the noun can render it with the shared `human_bytes`.
///
/// # Examples
///
/// ```
/// use osx_clnr::domain::docker_host::{prune_host_outcome, PruneHostOutcome};
///
/// // Case 1: VM freed nothing and nothing was pinned — plain success.
/// assert_eq!(prune_host_outcome(0, 0, 0), PruneHostOutcome::NothingToReclaim);
///
/// // Case 2: host blocks came back — report the measured return.
/// assert_eq!(
///     prune_host_outcome(5_000_000_000, 5_000_000_000, 0),
///     PruneHostOutcome::HostReturned(5_000_000_000),
/// );
///
/// // Case 3 (the trap): VM-side success, zero host return — must name the
/// // still-pinned remainder so the caller can advise a restart, not report
/// // success.
/// assert_eq!(
///     prune_host_outcome(5_000_000_000, 0, 117_000_000_000),
///     PruneHostOutcome::HostStillPinned { still_pinned_bytes: 117_000_000_000 },
/// );
///
/// // Edge: nothing pinned to begin with and nothing reclaimed is still
/// // NothingToReclaim, even if the VM reports a stray reclaimable figure.
/// assert_eq!(prune_host_outcome(0, 0, 0), PruneHostOutcome::NothingToReclaim);
/// ```
pub fn prune_host_outcome(
    vm_reclaimed_bytes: u64,
    host_returned_bytes: u64,
    host_still_pinned_bytes: u64,
) -> PruneHostOutcome {
    if host_returned_bytes > 0 {
        return PruneHostOutcome::HostReturned(host_returned_bytes);
    }
    if vm_reclaimed_bytes == 0 {
        return PruneHostOutcome::NothingToReclaim;
    }
    PruneHostOutcome::HostStillPinned { still_pinned_bytes: host_still_pinned_bytes }
}

/// Typed post-prune host outcome; the CLI layer renders it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneHostOutcome {
    /// Neither the VM nor the host had anything to give back.
    NothingToReclaim,
    /// `Docker.raw` physically shrank by this many bytes.
    HostReturned(u64),
    /// The VM freed space but the host image returned none of it; this many
    /// host bytes remain pinned inside the uncompacted image.
    HostStillPinned { still_pinned_bytes: u64 },
}
