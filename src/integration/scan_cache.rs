//! Persistent, `sled`-backed cache of per-directory scan results.
//!
//! **Integration layer rule**: this module owns the `sled::Db` handle and all
//! serialization. `CachedDirEntry` and the pure `cache_hit` comparison live in
//! `crate::domain::artifact` (primitives only, no `sled` types).
//!
//! Cache location: `~/.oclnr/cache/<workspace-hash>/scan.sled` — global, keyed
//! by a hash of the workspace path. The scanner must never write into the trees
//! it audits (a per-repo `.oclnr-cache/` dir leaked into every scanned repo and
//! got committed downstream, 2026-10-10).

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use anyhow::Context;
use ignore::WalkBuilder;

use crate::domain::artifact::{CachedDirEntry, DirSnapshot};

/// Default on-disk cap for one scan.sled instance. sled's log is append-only
/// with no compaction API, so an uncapped cache only ever grows (measured
/// 2026-10-03: ~2.6–2.7 GB per workspace after a handful of whole-home scans,
/// across both the `$HOME`-anchored and repo-workspace instances).
pub const DEFAULT_SCAN_CACHE_MAX_BYTES: u64 = 1_073_741_824;

/// Environment variable overriding [`DEFAULT_SCAN_CACHE_MAX_BYTES`] (a positive
/// integer byte count).
pub const SCAN_CACHE_MAX_BYTES_ENV: &str = "OCLNR_SCAN_CACHE_MAX_BYTES";

/// The effective per-instance cache cap: `OCLNR_SCAN_CACHE_MAX_BYTES` when set
/// to a positive integer, else [`DEFAULT_SCAN_CACHE_MAX_BYTES`].
///
/// # Examples
///
/// ```
/// use osx_clnr::integration::scan_cache::{
///     effective_cache_cap, DEFAULT_SCAN_CACHE_MAX_BYTES, SCAN_CACHE_MAX_BYTES_ENV,
/// };
///
/// std::env::remove_var(SCAN_CACHE_MAX_BYTES_ENV);
/// assert_eq!(effective_cache_cap(), DEFAULT_SCAN_CACHE_MAX_BYTES);
///
/// std::env::set_var(SCAN_CACHE_MAX_BYTES_ENV, "5000");
/// assert_eq!(effective_cache_cap(), 5000);
///
/// // Garbage or non-positive values fall back to the default.
/// std::env::set_var(SCAN_CACHE_MAX_BYTES_ENV, "not-a-number");
/// assert_eq!(effective_cache_cap(), DEFAULT_SCAN_CACHE_MAX_BYTES);
/// std::env::remove_var(SCAN_CACHE_MAX_BYTES_ENV);
/// ```
pub fn effective_cache_cap() -> u64 {
    std::env::var(SCAN_CACHE_MAX_BYTES_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&b| b > 0)
        .unwrap_or(DEFAULT_SCAN_CACHE_MAX_BYTES)
}

/// Recursive byte size of a directory's regular files (best effort: entries
/// that cannot be stat-ed contribute 0).
fn dir_size_bytes(path: &Path) -> u64 {
    let mut builder = WalkBuilder::new(path);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .same_file_system(true);
    builder
        .build()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// A persistent cache of per-directory scan results, backed by `sled`.
///
/// Any corrupt or undeserializable entry degrades to a cache miss (`Ok(None)`)
/// rather than panicking or returning an error — a stale/partial cache file
/// must never fail an audit scan.
pub struct ScanCache {
    /// Held for namespace inspection in tests; all reads/writes go via `tree`.
    #[cfg_attr(not(test), allow(dead_code))]
    db: sled::Db,
    tree: sled::Tree,
}

impl ScanCache {
    /// Opens (creating if necessary) the scan cache under `workspace`, scoped
    /// to the namespace `fingerprint` (see
    /// `crate::domain::artifact::scan_cache_fingerprint`), enforcing
    /// [`effective_cache_cap`] as the on-disk size cap.
    pub fn open(workspace: &Path, fingerprint: &str) -> anyhow::Result<Self> {
        Self::open_with_cap(workspace, fingerprint, effective_cache_cap())
    }

    /// [`ScanCache::open`] with an explicit cap — the testable core of the
    /// cap enforcement (env-free, so tests never race the process env).
    ///
    /// When the existing cache exceeds `max_bytes`, it is evicted (removed)
    /// and this open rebuilds it empty: sled's log is append-only with no
    /// compaction API, so eviction is the only way to reclaim bytes, and a
    /// cache is pure derived state — an audit must never fail over it.
    ///
    /// # Examples
    ///
    /// ```
    /// use osx_clnr::integration::scan_cache::ScanCache;
    ///
    /// let workspace = tempfile::tempdir().unwrap();
    /// let cache = ScanCache::open_with_cap(workspace.path(), "doctest-ns", u64::MAX).unwrap();
    /// assert_eq!(cache.get(&workspace.path().join("nowhere")).unwrap(), None);
    /// ```
    pub fn open_with_cap(
        workspace: &Path,
        fingerprint: &str,
        max_bytes: u64,
    ) -> anyhow::Result<Self> {
        // Global cache root keyed by workspace-path hash — never inside the
        // workspace itself (the scanner must not write into audited trees).
        let mut hasher = DefaultHasher::new();
        workspace.hash(&mut hasher);
        let dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory for scan cache"))?
            .join(".oclnr")
            .join("cache")
            .join(format!("{:016x}", hasher.finish()));
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating cache dir {}", dir.display()))?;
        let db_path = dir.join("scan.sled");
        if db_path.exists() {
            let size = dir_size_bytes(&db_path);
            if size > max_bytes {
                match crate::integration::fs::force_remove_dir_all(&db_path) {
                    Ok(()) => {
                        eprintln!(
                            "oclnr: evicted over-cap scan cache {} ({} bytes > {max_bytes} cap) — rebuilding empty",
                            db_path.display(),
                            size
                        );
                    }
                    Err(err) => {
                        // A cache problem must never fail a scan: keep the
                        // over-cap cache open rather than degrading the audit.
                        eprintln!(
                            "oclnr: scan cache {} is over cap ({} > {max_bytes} bytes) but eviction failed; continuing with the existing cache: {err:#}",
                            db_path.display(),
                            size
                        );
                    }
                }
            }
        }
        // sled holds an exclusive file lock that its background flusher can
        // keep for a moment after the previous handle drops, and concurrent
        // oclnr processes (MCP server, pressure daemon, autoclean) share this
        // one cache. Retry briefly instead of degrading straight to an
        // uncached scan on transient lock contention.
        let mut attempt = 0;
        let db = loop {
            match sled::open(&db_path) {
                Ok(db) => break db,
                Err(_) if attempt < 40 => {
                    attempt += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    return Err(anyhow::Error::new(e))
                        .with_context(|| format!("opening scan cache at {}", db_path.display()))
                }
            }
        };

        let prefix = crate::domain::artifact::scan_cache_revision_prefix();
        db.clear().context("clearing legacy unnamespaced scan cache")?;
        for name in db.tree_names() {
            let is_default = name.as_ref() == b"__sled__default";
            if !is_default && !name.starts_with(prefix.as_bytes()) {
                db.drop_tree(&name).context("dropping stale scan cache namespace")?;
            }
        }

        let tree = db
            .open_tree(fingerprint.as_bytes())
            .with_context(|| format!("opening scan cache namespace {fingerprint}"))?;
        Ok(Self { db, tree })
    }

    /// Looks up the cached entry for `path`, if any.
    ///
    /// Returns `Ok(None)` both when there is no entry for `path` *and* when the
    /// stored bytes fail to deserialize (a corrupt/partial cache write) — the
    /// caller should treat both the same way: scan the directory fresh.
    pub fn get(&self, path: &Path) -> anyhow::Result<Option<CachedDirEntry>> {
        let key = path_key(path);
        let Some(raw) = self.tree.get(key).context("reading from scan cache")? else {
            return Ok(None);
        };
        match serde_json::from_slice::<CachedDirEntry>(&raw) {
            Ok(entry) => Ok(Some(entry)),
            Err(_) => Ok(None),
        }
    }

    /// Writes a batch of `(path, entry)` pairs to the cache in a single
    /// `sled::Batch`, reducing write amplification versus per-directory writes.
    pub fn insert_batch(&self, entries: &[(PathBuf, CachedDirEntry)]) -> anyhow::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut batch = sled::Batch::default();
        for (path, entry) in entries {
            let value = serde_json::to_vec(entry).context("serializing cached dir entry")?;
            batch.insert(path_key(path), value);
        }
        self.tree.apply_batch(batch).context("applying scan cache batch")?;
        Ok(())
    }
}

fn path_key(path: &Path) -> Vec<u8> {
    path.as_os_str().as_encoded_bytes().to_vec()
}

/// Computes the early-cutoff hash of a directory's immediate children from its
/// already-built `DirSnapshot`: the sorted `(name, kind)` pairs are hashed with
/// the standard library's `DefaultHasher` (no extra serialization dependency).
pub fn child_names_hash(snap: &DirSnapshot) -> u64 {
    let mut pairs: Vec<(&str, bool)> =
        snap.children.iter().map(|e| (e.file_name.as_str(), e.is_dir())).collect();
    pairs.sort_unstable();

    let mut hasher = DefaultHasher::new();
    for (name, is_dir) in pairs {
        name.hash(&mut hasher);
        is_dir.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::artifact::CachedDirEntry;

    fn sample_entry() -> CachedDirEntry {
        CachedDirEntry {
            mtime: 12345,
            child_names_hash: 999,
            files: 3,
            bytes: 4096,
            dirs: 1,
            candidates: 1,
            candidates_list: vec![crate::domain::artifact::Candidate {
                path: PathBuf::from("/tmp/some/project/target"),
                reason: "rust target".to_string(),
            }],
        }
    }

    #[test]
    fn round_trip_insert_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(dir.path(), "scan-test").unwrap();
        let path = dir.path().join("some/project");
        let entry = sample_entry();

        cache.insert_batch(&[(path.clone(), entry.clone())]).unwrap();

        let got = cache.get(&path).unwrap();
        assert_eq!(got, Some(entry));
    }

    #[test]
    fn batch_write_multiple_entries() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(dir.path(), "scan-test").unwrap();

        let entries: Vec<(PathBuf, CachedDirEntry)> = (0..5)
            .map(|i| {
                let mut e = sample_entry();
                e.files = i;
                (dir.path().join(format!("dir{i}")), e)
            })
            .collect();

        cache.insert_batch(&entries).unwrap();

        for (path, expected) in &entries {
            let got = cache.get(path).unwrap();
            assert_eq!(got.as_ref(), Some(expected));
        }
    }

    #[test]
    fn corrupt_value_falls_back_to_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(dir.path(), "scan-test").unwrap();
        let path = dir.path().join("corrupt");

        // Write garbage bytes directly, bypassing the normal serde_json path.
        cache.tree.insert(path_key(&path), b"not valid json".to_vec()).unwrap();

        let got = cache.get(&path);
        assert!(got.is_ok());
        assert_eq!(got.unwrap(), None);
    }

    #[test]
    fn namespaces_are_isolated() {
        // A hit written under one classifier/flag namespace must be a miss
        // under another — the stale-candidate replay this guards against.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proj");
        {
            let deps_cache = ScanCache::open(dir.path(), "scan-a").unwrap();
            deps_cache.insert_batch(&[(path.clone(), sample_entry())]).unwrap();
            assert_eq!(deps_cache.get(&path).unwrap(), Some(sample_entry()));
        }
        let other = ScanCache::open(dir.path(), "scan-b").unwrap();
        assert_eq!(other.get(&path).unwrap(), None);
    }

    #[test]
    fn stale_revision_namespaces_are_dropped_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proj");
        let current = crate::domain::artifact::scan_cache_revision_prefix() + "x";
        {
            let stale = ScanCache::open(dir.path(), "scan-r0-v0-old").unwrap();
            stale.insert_batch(&[(path.clone(), sample_entry())]).unwrap();
            // Legacy unnamespaced write (pre-fingerprint binaries).
            stale.db.insert(path_key(&path), serde_json::to_vec(&sample_entry()).unwrap()).unwrap();
            stale.db.flush().unwrap();
        }
        let cache = ScanCache::open(dir.path(), &current).unwrap();
        assert!(!cache.db.tree_names().iter().any(|n| n.as_ref() == b"scan-r0-v0-old"));
        assert_eq!(cache.db.get(path_key(&path)).unwrap(), None);
        assert_eq!(cache.get(&path).unwrap(), None);
    }

    #[test]
    fn missing_key_is_a_plain_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ScanCache::open(dir.path(), "scan-test").unwrap();
        let got = cache.get(&dir.path().join("never-inserted")).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn over_cap_cache_is_evicted_and_rebuilt_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proj");
        // Revision-prefixed namespace so the open-time stale-revision drop
        // doesn't clear the tree for us — the cap must be what evicts it.
        let ns = format!("{}cap-test", crate::domain::artifact::scan_cache_revision_prefix());
        {
            let cache = ScanCache::open_with_cap(dir.path(), &ns, u64::MAX).unwrap();
            cache.insert_batch(&[(path.clone(), sample_entry())]).unwrap();
            cache.db.flush().unwrap();
        }
        // A 1-byte cap evicts anything; the reopen must succeed with a miss,
        // never fail — eviction is the degrade path, not an error path.
        let reopened = ScanCache::open_with_cap(dir.path(), &ns, 1).unwrap();
        assert_eq!(reopened.get(&path).unwrap(), None, "over-cap cache must be evicted");
    }

    #[test]
    fn under_cap_cache_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proj");
        let ns = format!("{}keep-test", crate::domain::artifact::scan_cache_revision_prefix());
        {
            let cache = ScanCache::open_with_cap(dir.path(), &ns, u64::MAX).unwrap();
            cache.insert_batch(&[(path.clone(), sample_entry())]).unwrap();
            cache.db.flush().unwrap();
        }
        let reopened = ScanCache::open_with_cap(dir.path(), &ns, u64::MAX).unwrap();
        assert_eq!(reopened.get(&path).unwrap(), Some(sample_entry()));
    }

    #[test]
    fn dir_size_counts_regular_files_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let tree = dir.path().join("cache-tree");
        std::fs::create_dir_all(tree.join("nested")).unwrap();
        std::fs::write(tree.join("a"), vec![0u8; 100]).unwrap();
        std::fs::write(tree.join("nested").join("b"), vec![0u8; 250]).unwrap();
        assert_eq!(dir_size_bytes(&tree), 350);
        assert_eq!(dir_size_bytes(&dir.path().join("absent")), 0);
    }
}
