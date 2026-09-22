//! Duplicate-file measurement for APFS clone-based deduplication (read-only).
//!
//! Pure domain logic: no filesystem, process, or OS calls. The integration
//! layer (`integration::dedupe`) walks roots, stats files, hashes
//! size-collision candidates, and queries each file's APFS *private size*
//! (`ATTR_CMNEXT_PRIVATESIZE`); it hands this module inert [`FileStat`] and
//! [`DupFile`] DTOs.
//!
//! # What is measured
//!
//! A duplicate group is a set of regular files with identical content
//! (same size, same BLAKE3 hash) on the same volume (`st_dev`) — APFS clones
//! cannot cross volumes. Replacing every member except one *keeper* with an
//! APFS clone of the keeper would free each non-keeper's **private**
//! (unshared) bytes. Files that already share extents (e.g. made with
//! `cp -c`) have a private size near zero and therefore contribute near-zero
//! reclaim, which is what prevents overcounting.
//!
//! Keeper selection is deterministic: the member with the smallest private
//! size, ties broken by the lexicographically smallest path. Choosing the
//! least-private member maximizes the achievable reclaim
//! (`sum(private) - private(keeper)`); when every member is fully private
//! (the common case), this reduces to the lexicographically smallest path.
//!
//! If a member's private size could not be obtained, its logical size is used
//! instead and the group (and report) is flagged `estimate: true`.
//!
//! Blocks still referenced by local APFS/Time Machine snapshots are not freed
//! until those snapshots are thinned; the reported number is the bytes that
//! become unreferenced by the live filesystem.
//!
//! # Execute step: UNSUPPORTED
//!
//! `dedupe execute` (actually replacing duplicates with `clonefile(2)` clones)
//! is **UNSUPPORTED** in this release — status `UNSUPPORTED(dedupe-execute)`.
//! Only measurement exists. See [`EXECUTE_STATUS`].

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

/// Typed status of the (unimplemented) clone-replacement step.
pub const EXECUTE_STATUS: &str =
    "UNSUPPORTED(dedupe-execute): clone replacement is not implemented; scan is read-only";

/// Default minimum file size considered for dedupe (64 KiB).
pub const DEFAULT_MIN_SIZE: u64 = 64 * 1024;

/// Inert stat record for one regular file, produced by the walker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStat {
    pub path: PathBuf,
    pub size: u64,
    pub dev: u64,
    pub ino: u64,
}

/// A hashed, size-annotated file that is a dedupe candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DupFile {
    pub path: PathBuf,
    /// Logical size in bytes (`st_size`).
    pub size: u64,
    /// Private (unshared) allocated bytes, or `None` if the attribute was unavailable.
    pub private_size: Option<u64>,
    /// Volume device id (`st_dev`).
    pub dev: u64,
    /// Hex content hash (BLAKE3).
    pub hash: String,
}

impl DupFile {
    /// Bytes this file would free if replaced by a clone: private size when
    /// known, else logical size (an estimate).
    ///
    /// ```
    /// use osx_clnr::domain::dedupe::DupFile;
    /// let mut f = DupFile { path: "/a".into(), size: 100, private_size: Some(40), dev: 1, hash: "h".into() };
    /// assert_eq!(f.reclaim_weight(), 40);
    /// f.private_size = None; // negative: attribute unavailable -> logical fallback
    /// assert_eq!(f.reclaim_weight(), 100);
    /// ```
    pub fn reclaim_weight(&self) -> u64 {
        self.private_size.unwrap_or(self.size)
    }
}

/// Why a set of files was refused as a duplicate group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DedupeRefusal {
    /// Fewer than two members: nothing to dedupe.
    TooFewMembers(usize),
    /// Members live on different volumes; clones cannot cross volumes.
    CrossVolume,
    /// Members disagree on size or hash; not identical content.
    ContentMismatch,
}

impl std::fmt::Display for DedupeRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DedupeRefusal::TooFewMembers(n) => {
                write!(f, "refused: group has {n} member(s), need >= 2")
            }
            DedupeRefusal::CrossVolume => write!(f, "refused: members span multiple volumes"),
            DedupeRefusal::ContentMismatch => write!(f, "refused: members differ in size or hash"),
        }
    }
}

/// A validated set of identical files on one volume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DupGroup {
    pub hash: String,
    pub size: u64,
    pub dev: u64,
    /// Path of the file that would be kept (clone source).
    pub keeper: PathBuf,
    /// Members sorted by path; includes the keeper.
    pub members: Vec<DupFile>,
    /// Sum of reclaim weights of all members except the keeper.
    pub reclaimable_bytes: u64,
    /// True if any member's private size was unavailable.
    pub estimate: bool,
}

impl DupGroup {
    /// Validates members and computes keeper + reclaim.
    ///
    /// ```
    /// use osx_clnr::domain::dedupe::{DupFile, DupGroup, DedupeRefusal};
    /// let f = |p: &str, private: Option<u64>, dev: u64| DupFile {
    ///     path: p.into(), size: 100, private_size: private, dev, hash: "h".into(),
    /// };
    ///
    /// // Positive: three fully-private copies -> keep "/a", reclaim 200.
    /// let g = DupGroup::try_new(vec![f("/c", Some(100), 1), f("/a", Some(100), 1), f("/b", Some(100), 1)]).unwrap();
    /// assert_eq!(g.keeper, std::path::PathBuf::from("/a"));
    /// assert_eq!(g.reclaimable_bytes, 200);
    /// assert!(!g.estimate);
    ///
    /// // Negative: an existing clone pair shares extents -> ~0 reclaim.
    /// let g = DupGroup::try_new(vec![f("/a", Some(0), 1), f("/b", Some(0), 1)]).unwrap();
    /// assert_eq!(g.reclaimable_bytes, 0);
    ///
    /// // Keeper prefers the least-private member: {A,B clones, C independent} -> reclaim C.
    /// let g = DupGroup::try_new(vec![f("/a", Some(0), 1), f("/b", Some(0), 1), f("/0c", Some(100), 1)]).unwrap();
    /// assert_eq!(g.keeper, std::path::PathBuf::from("/a"));
    /// assert_eq!(g.reclaimable_bytes, 100);
    ///
    /// // Estimate: unknown private size falls back to logical size.
    /// let g = DupGroup::try_new(vec![f("/a", None, 1), f("/b", None, 1)]).unwrap();
    /// assert!(g.estimate);
    /// assert_eq!(g.reclaimable_bytes, 100);
    ///
    /// // Refusals.
    /// assert_eq!(DupGroup::try_new(vec![f("/a", Some(1), 1)]), Err(DedupeRefusal::TooFewMembers(1)));
    /// assert_eq!(DupGroup::try_new(vec![f("/a", Some(1), 1), f("/b", Some(1), 2)]), Err(DedupeRefusal::CrossVolume));
    /// let mut other = f("/b", Some(1), 1);
    /// other.hash = "different".into();
    /// assert_eq!(DupGroup::try_new(vec![f("/a", Some(1), 1), other]), Err(DedupeRefusal::ContentMismatch));
    /// ```
    pub fn try_new(mut members: Vec<DupFile>) -> Result<DupGroup, DedupeRefusal> {
        if members.len() < 2 {
            return Err(DedupeRefusal::TooFewMembers(members.len()));
        }
        let first = &members[0];
        let (dev, size, hash) = (first.dev, first.size, first.hash.clone());
        if members.iter().any(|m| m.dev != dev) {
            return Err(DedupeRefusal::CrossVolume);
        }
        if members.iter().any(|m| m.size != size || m.hash != hash) {
            return Err(DedupeRefusal::ContentMismatch);
        }
        members.sort_by(|a, b| a.path.cmp(&b.path));
        let keeper_idx = members
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.reclaim_weight().cmp(&b.reclaim_weight()).then_with(|| a.path.cmp(&b.path))
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        let reclaimable_bytes = members
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != keeper_idx)
            .map(|(_, m)| m.reclaim_weight())
            .sum();
        let estimate = members.iter().any(|m| m.private_size.is_none());
        Ok(DupGroup {
            hash,
            size,
            dev,
            keeper: members[keeper_idx].path.clone(),
            members,
            reclaimable_bytes,
            estimate,
        })
    }
}

/// Removes hardlink aliases (same `(dev, ino)`), keeping the lexicographically
/// smallest path, then keeps only files that are at least `min_size` bytes and
/// share their `(dev, size)` with at least one other file — the only files
/// worth hashing.
///
/// ```
/// use osx_clnr::domain::dedupe::{size_collision_candidates, FileStat};
/// let s = |p: &str, size: u64, dev: u64, ino: u64| FileStat { path: p.into(), size, dev, ino };
///
/// // Positive: two same-size files on one volume are candidates.
/// let c = size_collision_candidates(vec![s("/a", 100, 1, 1), s("/b", 100, 1, 2), s("/u", 7000, 1, 3)], 10);
/// assert_eq!(c.len(), 2);
///
/// // Negative: same size on different volumes, or below min size, is not a candidate.
/// assert!(size_collision_candidates(vec![s("/a", 100, 1, 1), s("/b", 100, 2, 2)], 10).is_empty());
/// assert!(size_collision_candidates(vec![s("/a", 5, 1, 1), s("/b", 5, 1, 2)], 10).is_empty());
///
/// // Refusal: two hardlinks to one inode are one file, never a duplicate pair.
/// assert!(size_collision_candidates(vec![s("/a", 100, 1, 9), s("/b", 100, 1, 9)], 10).is_empty());
/// ```
pub fn size_collision_candidates(files: Vec<FileStat>, min_size: u64) -> Vec<FileStat> {
    let mut by_inode: BTreeMap<(u64, u64), FileStat> = BTreeMap::new();
    for f in files.into_iter().filter(|f| f.size >= min_size) {
        match by_inode.get(&(f.dev, f.ino)) {
            Some(existing) if existing.path <= f.path => {}
            _ => {
                by_inode.insert((f.dev, f.ino), f);
            }
        }
    }
    let mut by_size: BTreeMap<(u64, u64), Vec<FileStat>> = BTreeMap::new();
    for f in by_inode.into_values() {
        by_size.entry((f.dev, f.size)).or_default().push(f);
    }
    let mut out: Vec<FileStat> = by_size.into_values().filter(|v| v.len() >= 2).flatten().collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Full read-only dedupe measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupeReport {
    pub roots: Vec<PathBuf>,
    pub min_size: u64,
    /// Regular files seen by the walker (before size filtering).
    pub files_scanned: u64,
    /// Files hashed (size-collision candidates).
    pub candidates_hashed: u64,
    /// Candidates that could not be hashed (permission/IO errors).
    pub hash_errors: u64,
    /// Number of duplicate groups.
    pub group_count: u64,
    /// Files belonging to some duplicate group (keepers included).
    pub duplicate_files: u64,
    /// Logical bytes of all non-keeper members (upper bound ignoring existing sharing).
    pub logical_duplicate_bytes: u64,
    /// Bytes reclaimable by cloning, net of existing extent sharing.
    pub reclaimable_bytes: u64,
    /// True if any group fell back to logical size.
    pub estimate: bool,
    /// Typed status of the execute step.
    pub execute_status: String,
    /// Groups sorted by reclaimable bytes, descending (ties by keeper path).
    pub groups: Vec<DupGroup>,
}

/// Groups hashed candidates by `(dev, size, hash)` into validated duplicate
/// groups and totals the reclaim.
///
/// ```
/// use osx_clnr::domain::dedupe::{build_report, DupFile};
/// let f = |p: &str, hash: &str, private: Option<u64>| DupFile {
///     path: p.into(), size: 100, private_size: private, dev: 1, hash: hash.into(),
/// };
///
/// // Positive: a pair of copies + a same-size file with different content.
/// let r = build_report(vec![], 64, 3, vec![f("/a", "x", Some(100)), f("/b", "x", Some(100)), f("/c", "y", Some(100))], 0);
/// assert_eq!(r.group_count, 1);
/// assert_eq!(r.reclaimable_bytes, 100);
/// assert_eq!(r.logical_duplicate_bytes, 100);
/// assert!(r.execute_status.starts_with("UNSUPPORTED"));
///
/// // Negative: an existing clone pair reclaims nothing but is still reported.
/// let r = build_report(vec![], 64, 2, vec![f("/a", "x", Some(0)), f("/b", "x", Some(0))], 0);
/// assert_eq!((r.group_count, r.reclaimable_bytes, r.logical_duplicate_bytes), (1, 0, 100));
///
/// // Refusal: same hash on different volumes never forms a group.
/// let mut other = f("/b", "x", Some(100));
/// other.dev = 2;
/// let r = build_report(vec![], 64, 2, vec![f("/a", "x", Some(100)), other], 0);
/// assert_eq!(r.group_count, 0);
/// assert_eq!(r.reclaimable_bytes, 0);
/// ```
pub fn build_report(
    roots: Vec<PathBuf>,
    min_size: u64,
    files_scanned: u64,
    hashed: Vec<DupFile>,
    hash_errors: u64,
) -> DedupeReport {
    let candidates_hashed = hashed.len() as u64;
    let mut buckets: BTreeMap<(u64, u64, String), Vec<DupFile>> = BTreeMap::new();
    for f in hashed {
        buckets.entry((f.dev, f.size, f.hash.clone())).or_default().push(f);
    }
    let mut groups: Vec<DupGroup> =
        buckets.into_values().filter_map(|members| DupGroup::try_new(members).ok()).collect();
    groups.sort_by(|a, b| {
        b.reclaimable_bytes.cmp(&a.reclaimable_bytes).then_with(|| a.keeper.cmp(&b.keeper))
    });
    let duplicate_files = groups.iter().map(|g| g.members.len() as u64).sum();
    let logical_duplicate_bytes =
        groups.iter().map(|g| g.size * (g.members.len() as u64 - 1)).sum();
    let reclaimable_bytes = groups.iter().map(|g| g.reclaimable_bytes).sum();
    let estimate = groups.iter().any(|g| g.estimate);
    DedupeReport {
        roots,
        min_size,
        files_scanned,
        candidates_hashed,
        hash_errors,
        group_count: groups.len() as u64,
        duplicate_files,
        logical_duplicate_bytes,
        reclaimable_bytes,
        estimate,
        execute_status: EXECUTE_STATUS.to_string(),
        groups,
    }
}
