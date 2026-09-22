//! Persistent, `sled`-backed cache of per-directory scan results.
//!
//! **Integration layer rule**: this module owns the `sled::Db` handle and all
//! serialization. `CachedDirEntry` and the pure `cache_hit` comparison live in
//! `crate::domain::artifact` (primitives only, no `sled` types).
//!
//! Cache location: `workspace.join(".oclnr-cache/scan.sled")`, matching the
//! existing workspace-relative convention used for `disk-audit.jsonocel` /
//! `cleanup-plan.json`.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use anyhow::Context;

use crate::domain::artifact::{CachedDirEntry, DirSnapshot};

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
    /// `crate::domain::artifact::scan_cache_fingerprint`).
    ///
    /// Cached entries hold *classified* candidates, so entries written under a
    /// different classifier revision or nomination flags are unsound to replay.
    /// Each fingerprint gets its own `sled` tree; on open, the legacy
    /// unnamespaced default tree and every tree from an older classifier
    /// revision are dropped so stale candidates can never resurface.
    pub fn open(workspace: &Path, fingerprint: &str) -> anyhow::Result<Self> {
        let dir = workspace.join(".oclnr-cache");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating cache dir {}", dir.display()))?;
        let db_path = dir.join("scan.sled");
        let db = sled::open(&db_path)
            .with_context(|| format!("opening scan cache at {}", db_path.display()))?;

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
}
