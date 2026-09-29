//! Cache-entry accounting for `scan_root`, with a real tree, a real on-disk
//! `ScanCache`, and state-based assertions (no doubles).
//!
//! The scan cache stores, per directory, a true-recursive `files`/`bytes`
//! aggregate. Those numbers are fed from the stat the walker already performs
//! on every entry (previously a second `stat` of every file). This file pins
//! the counting rule against an independent oracle computed with `std::fs`
//! over the same tree:
//!
//! * a regular file counts once per name (hardlinks included) at
//!   `blocks * 512` physical bytes;
//! * a symlink to a file counts through its target (following `stat`);
//! * a broken symlink and a symlink to a directory count nothing;
//! * directories themselves count nothing;
//! * any entry whose name is a traversal barrier (`target`, ...) is never
//!   walked, so it and its subtree count nothing -- the same rule the cold
//!   scan's `files_seen` follows.
//!
//! It also pins the scan's result equivalence: the candidate set found with a
//! cache attached equals the set found without one.

use std::{
    fs::{self, File},
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
};

use dashmap::DashMap;
use osx_clnr::{
    domain::{
        artifact::{is_traversal_barrier_name, ArgsSnapshot, Candidate},
        audit::Stats,
    },
    integration::{fs::scan_root, scan_cache::ScanCache},
};

fn args() -> ArgsSnapshot {
    ArgsSnapshot {
        deps: true,
        aggressive: true,
        verbose: false,
        tool_roots: false,
        ignore_recent_hours: 0,
        all_filesystems: false,
    }
}

fn write_bytes(path: &Path, n: usize) {
    let mut f = File::create(path).unwrap();
    f.write_all(&vec![7u8; n]).unwrap();
    f.sync_all().unwrap();
}

/// Oracle: (files, bytes) under `dir` by the counting rule in the module doc.
fn oracle(dir: &Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if is_traversal_barrier_name(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let lmeta = fs::symlink_metadata(&path).unwrap();
        if lmeta.is_dir() {
            let (f, b) = oracle(&path);
            files += f;
            bytes += b;
        } else if let Ok(m) = fs::metadata(&path) {
            if !m.is_dir() {
                files += 1;
                bytes += m.blocks() * 512;
            }
        }
    }
    (files, bytes)
}

fn scan(root: &Path, cache: Option<Arc<ScanCache>>) -> (Vec<PathBuf>, Arc<Stats>) {
    let candidates: Arc<DashMap<PathBuf, Candidate>> = Arc::new(DashMap::new());
    let stats = Arc::new(Stats::default());
    scan_root(
        root,
        &args(),
        candidates.clone(),
        stats.clone(),
        &[],
        Arc::new(DashMap::new()),
        cache,
    )
    .unwrap();
    let mut found: Vec<PathBuf> = candidates.iter().map(|e| e.key().clone()).collect();
    found.sort();
    (found, stats)
}

#[test]
fn cache_entry_counts_match_independent_oracle() {
    let cache_tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let cache = Arc::new(ScanCache::open(cache_tmp.path(), "scan-own-stats").unwrap());

    let root_tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let root = root_tmp.path().canonicalize().unwrap();

    let proj = root.join("a").join("proj");
    fs::create_dir_all(proj.join("target").join("deep")).unwrap();
    File::create(proj.join("Cargo.toml")).unwrap();
    write_bytes(&proj.join("src.rs"), 10_000);
    write_bytes(&proj.join("target").join("out.bin"), 50_000);
    write_bytes(&proj.join("target").join("deep").join("x.o"), 9_000);

    // Hardlink pair: two names, one inode — one file per name.
    fs::hard_link(proj.join("src.rs"), proj.join("src-link.rs")).unwrap();
    // Symlink to a file (counts via target), to a directory and a broken one
    // (count nothing).
    std::os::unix::fs::symlink(proj.join("src.rs"), proj.join("file-sym")).unwrap();
    std::os::unix::fs::symlink(proj.join("target"), proj.join("dir-sym")).unwrap();
    std::os::unix::fs::symlink(root.join("nope"), proj.join("broken-sym")).unwrap();
    fs::create_dir(root.join("empty")).unwrap();

    let (cold_cands, _) = scan(&root, Some(cache.clone()));
    let (plain_cands, _) = scan(&root, None);
    assert_eq!(cold_cands, vec![proj.join("target")], "fixture must nominate the rust target");
    assert_eq!(cold_cands, plain_cands, "cache must not change the candidate set");

    for dir in [&root, &root.join("a"), &proj, &root.join("empty")] {
        let entry = cache
            .get(dir)
            .unwrap()
            .unwrap_or_else(|| panic!("no cache entry written for {}", dir.display()));
        let (files, bytes) = oracle(dir);
        assert_eq!(
            (entry.files, entry.bytes),
            (files, bytes),
            "own-stats drift at {}",
            dir.display()
        );
    }

    // A warm scan is served by cache hits on root's children, so its
    // `files_seen` is exactly the oracle-checked count it replays.
    let (warm_cands, warm_stats) = scan(&root, Some(cache.clone()));
    assert_eq!(warm_cands, cold_cands);
    let (oracle_files, _) = oracle(&root);
    assert_eq!(warm_stats.files_seen.load(Ordering::Relaxed) as u64, oracle_files);
}
