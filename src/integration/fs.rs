#![allow(
    clippy::manual_flatten,
    clippy::collapsible_if,
    clippy::unnecessary_sort_by,
    clippy::redundant_pattern_matching
)]
//! Filesystem traversal, size estimation, and plan-bound deletion.
//!
//! **Integration layer rule**: All `std::fs` and OS calls live here.
//! Domain functions receive inert DTOs; this module builds those DTOs
//! from live OS handles.

use dashmap::DashMap;
use ignore::{WalkBuilder, WalkState};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::domain::artifact::{
    artifact_candidates_from_snapshot, cache_hit, detect_project_from_snapshot, is_global_cache,
    is_macos_os_dir, is_traversal_barrier_name, ArgsSnapshot, CachedDirEntry, Candidate,
    DirSnapshot, EntryKind, EntrySnapshot,
};
use crate::domain::audit::Stats;
use crate::domain::tool_roots::{ToolRootAcc, ToolRootDef};
use crate::integration::scan_cache::{child_names_hash, ScanCache};
use anyhow::Context;

// ── DirSnapshot builder ────────────────────────────────────────────────────────

/// Reads the immediate children of `dir` and constructs an inert `DirSnapshot`.
///
/// This is the integration layer's responsibility: it reads the filesystem
/// once and passes the snapshot to pure domain functions.
pub fn read_dir_snapshot(dir: &Path) -> DirSnapshot {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return DirSnapshot::default();
    };

    let mut children = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        let kind = match entry.file_type() {
            Ok(ft) if ft.is_dir() => EntryKind::Dir,
            Ok(ft) if ft.is_file() => EntryKind::File,
            _ => {
                // `entry.file_type()` only fails to resolve to file/dir here on
                // genuine `DT_UNKNOWN` results (rare — FUSE and a handful of
                // other filesystems don't populate `d_type`), not as a routine
                // path. Fall back to a `stat` via `Path::is_dir` in that case.
                if path.is_dir() {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                }
            }
        };
        children.push(EntrySnapshot::new(path, file_name, extension, kind));
    }

    DirSnapshot { children }
}

// ── Disk-full-safe writer ──────────────────────────────────────────────────────

/// Writes `contents` to `path`, but if the write fails because the volume is
/// full (`ENOSPC`), dumps the contents to stdout and returns `Ok` instead.
///
/// A plan or receipt is evidence we must not lose to the very condition the tool
/// exists to fix: at ~0 bytes free, `std::fs::write` of a multi-KB JSON fails,
/// and silently losing it is how the original deadlock happened. Here we surface
/// it so the user can capture it and run `oclnr emergency` to recover space.
pub fn write_or_dump_on_full(path: &Path, contents: &str, label: &str) -> anyhow::Result<()> {
    match std::fs::write(path, contents) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::StorageFull => {
            eprintln!(
                "⚠️  Disk full — could not write {} to {}.",
                label,
                path.display()
            );
            eprintln!("    Dumping it below; save it elsewhere, then run `oclnr emergency --yes`.");
            println!("{}", contents);
            Ok(())
        }
        Err(e) => Err(e).with_context(|| format!("writing {} to {}", label, path.display())),
    }
}

// ── Volume free-space query ──────────────────────────────────────────────────────

/// Inert snapshot of a volume's capacity, as reported by `statvfs(2)`.
///
/// All values are in bytes. `available` is the space usable by an unprivileged
/// process (`f_bavail`) and is the right number to report as "free" to a user;
/// `free` (`f_bfree`) includes root-reserved blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeSpace {
    pub total: u64,
    pub free: u64,
    pub available: u64,
}

impl VolumeSpace {
    /// Percentage of capacity used, 0–100. Returns 0 if `total` is 0.
    pub fn percent_used(&self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        let used = self.total.saturating_sub(self.available);
        ((used as f64 / self.total as f64) * 100.0).round() as u8
    }
}

/// Queries the capacity of the volume containing `path` via `statvfs(2)`.
///
/// This is the integration layer's responsibility: it performs the OS call and
/// returns an inert [`VolumeSpace`] DTO for the domain/noun layers to format.
pub fn volume_space(path: &Path) -> anyhow::Result<VolumeSpace> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("path contains NUL byte: {}", path.display()))?;

    // SAFETY: `stat` is zeroed and `c_path` is a valid NUL-terminated C string
    // that outlives the call. `statvfs` only writes into `stat` on success.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("statvfs failed for {}", path.display()));
    }

    // `f_frsize` is the fundamental block size used by the block counts.
    let frsize = stat.f_frsize as u64;
    Ok(VolumeSpace {
        total: frsize * stat.f_blocks as u64,
        free: frsize * stat.f_bfree as u64,
        available: frsize * stat.f_bavail as u64,
    })
}

// ── Size estimation ────────────────────────────────────────────────────────────

/// Estimates size of a path recursively.
pub fn estimate_size(path: &Path, stats: Arc<Stats>) -> u64 {
    if path.is_file() {
        return std::fs::metadata(path)
            .map(|m| {
                stats
                    .bytes_seen
                    .fetch_add(m.blocks() * 512, Ordering::Relaxed);
                stats.files_seen.fetch_add(1, Ordering::Relaxed);
                m.blocks() * 512
            })
            .unwrap_or(0);
    }

    if !path.is_dir() {
        return 0;
    }

    let mut size = 0;
    let mut builder = WalkBuilder::new(path);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .same_file_system(true);

    for result in builder.build() {
        match result {
            Ok(entry) => {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        let physical = meta.blocks() * 512;
                        size += physical;
                        stats.bytes_seen.fetch_add(physical, Ordering::Relaxed);
                        stats.files_seen.fetch_add(1, Ordering::Relaxed);
                    } else if meta.is_dir() {
                        stats.dirs_seen.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Err(err) => {
                stats.errors.fetch_add(1, Ordering::Relaxed);
                let path = match &err {
                    ignore::Error::WithPath { path, .. } => path.clone(),
                    ignore::Error::Loop { child, .. } => child.clone(),
                    _ => PathBuf::new(),
                };
                stats
                    .error_details
                    .lock()
                    .unwrap()
                    .push((path, err.to_string()));
            }
        }
    }
    size
}

/// Performs a fast, bounded traversal to see if any file within the project
/// has been modified recently. This correctly identifies active development
/// because a directory's `mtime` only updates on direct child changes.
fn is_recently_active(project_root: &Path, hours: u64) -> bool {
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(hours * 3600);

    let mut builder = WalkBuilder::new(project_root);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .max_depth(Some(4)); // Limit depth for speed

    for result in builder.build() {
        if let Ok(entry) = result {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    if let Ok(modified) = meta.modified() {
                        if modified > cutoff {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

// ── Parallel traversal ─────────────────────────────────────────────────────────

/// Recursively traverses a root path to find candidate files/folders.
///
/// For each directory encountered, this function builds an inert `DirSnapshot`
/// and passes it to the pure domain functions `detect_project_from_snapshot`
/// and `artifact_candidates_from_snapshot`.
pub fn scan_root(
    root: &Path,
    args: &ArgsSnapshot,
    candidates: Arc<DashMap<PathBuf, Candidate>>,
    stats: Arc<Stats>,
    known_tool_defs: &[ToolRootDef],
    tool_accs: Arc<DashMap<PathBuf, ToolRootAcc>>,
    scan_cache: Option<Arc<ScanCache>>,
) -> anyhow::Result<()> {
    if !root.exists() {
        if args.verbose {
            eprintln!("Skipping missing root: {}", root.display());
        }
        return Ok(());
    }

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);

    let root_for_filter = root.to_path_buf();
    let stats_for_filter = stats.clone();
    let scan_cache_for_filter = scan_cache.clone();
    let candidates_for_filter = candidates.clone();

    // Memoizes a directory's freshly-read `DirSnapshot` between `filter_entry`
    // (which reads it once to compute the early-cutoff hash when a cached
    // entry's mtime has changed) and the main visitor closure below (which
    // needs the same snapshot again to detect projects/candidates). Without
    // this, both call sites independently called `read_dir_snapshot`, doing
    // the same `std::fs::read_dir` twice for every directory whose mtime had
    // changed but which turned out to be a real, walkable miss.
    let snapshot_memo: Arc<DashMap<PathBuf, DirSnapshot>> = Arc::new(DashMap::new());
    let snapshot_memo_for_filter = snapshot_memo.clone();

    // Bookkeeping consumed by `aggregate_subtrees` after the walk completes:
    // one `DirRecord` per freshly-visited directory (its own, non-recursive
    // stats + the candidates found directly within it), and one
    // `CachedDirEntry` per directory pruned via a scan-cache hit (already a
    // full recursive aggregate from a prior scan). Folding these bottom-up
    // yields true recursive subtree totals instead of the previous
    // shallow/immediate-children-only counts.
    let dir_records: Arc<Mutex<Vec<(PathBuf, DirRecord)>>> = Arc::new(Mutex::new(Vec::new()));
    let cache_hits: Arc<Mutex<Vec<(PathBuf, CachedDirEntry)>>> = Arc::new(Mutex::new(Vec::new()));
    let cache_hits_for_filter = cache_hits.clone();

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .same_file_system(true)
        .threads(threads);

    builder.filter_entry(move |entry| {
        let path = entry.path();
        if path == root_for_filter {
            return true;
        }

        if is_macos_os_dir(path) {
            stats_for_filter.pruned_dirs.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        if is_global_cache(path) {
            stats_for_filter.pruned_dirs.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if is_traversal_barrier_name(name) {
                stats_for_filter.pruned_dirs.fetch_add(1, Ordering::Relaxed);
                return false;
            }
        }

        // Cache-based early cutoff: only directories are cached. A hit means
        // this directory's children (as of last scan) are unchanged, so we
        // fold the previously-recorded, true-recursive counts into `stats`,
        // re-insert its cached candidates into the live set, and prune
        // descent into the subtree instead of re-walking it.
        if let Some(cache) = &scan_cache_for_filter {
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    if let Ok(Some(prev)) = cache.get(path) {
                        let mtime = meta.mtime();
                        if prev.mtime == mtime {
                            fold_cached_stats(&stats_for_filter, &candidates_for_filter, &prev);
                            cache_hits_for_filter
                                .lock()
                                .unwrap()
                                .push((path.to_path_buf(), prev));
                            return false;
                        }
                        // mtime changed — early-cutoff: check whether the
                        // child listing itself is actually unchanged (e.g. a
                        // benign atime-only touch or backup-tool utimes call).
                        let snap = read_dir_snapshot(path);
                        let hash = child_names_hash(&snap);
                        if cache_hit(&prev, mtime, hash) {
                            fold_cached_stats(&stats_for_filter, &candidates_for_filter, &prev);
                            cache_hits_for_filter
                                .lock()
                                .unwrap()
                                .push((path.to_path_buf(), prev));
                            return false;
                        }
                        // Real change — this directory will be visited
                        // normally below. Stash the snapshot already read
                        // here so the main closure doesn't read it again.
                        snapshot_memo_for_filter.insert(path.to_path_buf(), snap);
                    }
                }
            }
        }
        true
    });

    let walker = builder.build_parallel();
    let args_snapshot = *args;
    let known_tool_defs: Arc<[ToolRootDef]> = Arc::from(known_tool_defs.to_vec());

    walker.run(|| {
        let candidates = candidates.clone();
        let stats = stats.clone();
        let known_tool_defs = known_tool_defs.clone();
        let tool_accs = tool_accs.clone();
        let scan_cache = scan_cache.clone();
        let snapshot_memo = snapshot_memo.clone();
        let dir_records = dir_records.clone();

        Box::new(move |result| {
            let entry = match result {
                Ok(e) => e,
                Err(err) => {
                    stats.errors.fetch_add(1, Ordering::Relaxed);
                    let path = match &err {
                        ignore::Error::WithPath { path, .. } => path.clone(),
                        ignore::Error::Loop { child, .. } => child.clone(),
                        _ => PathBuf::new(),
                    };
                    stats
                        .error_details
                        .lock()
                        .unwrap()
                        .push((path, err.to_string()));
                    return WalkState::Continue;
                }
            };

            let path = entry.path();
            if args_snapshot.verbose {
                eprintln!("Visiting: {}", path.display());
            }

            let meta = entry.metadata().ok();
            let mut is_dir = false;
            if let Some(meta) = &meta {
                if meta.is_file() {
                    let physical = meta.blocks() * 512;
                    stats.files_seen.fetch_add(1, Ordering::Relaxed);
                    stats.bytes_seen.fetch_add(physical, Ordering::Relaxed);
                    if args_snapshot.tool_roots {
                        record_tool_root_file(
                            path,
                            physical,
                            meta.mtime(),
                            &known_tool_defs,
                            &tool_accs,
                        );
                    }
                } else if meta.is_dir() {
                    is_dir = true;
                    stats.dirs_seen.fetch_add(1, Ordering::Relaxed);
                    if args_snapshot.tool_roots {
                        record_tool_root_dir(path, &known_tool_defs, &tool_accs);
                    }
                }
            }

            if !is_dir {
                return WalkState::Continue;
            }

            // Build an inert snapshot of this directory's children and pass it
            // to pure domain functions. No live OS handles are passed into domain.
            // Reuse the snapshot already read in `filter_entry`'s early-cutoff
            // check, if any, instead of reading this directory a second time.
            let snap = snapshot_memo
                .remove(path)
                .map(|(_, s)| s)
                .unwrap_or_else(|| read_dir_snapshot(path));
            let mut own_candidates: Vec<Candidate> = Vec::new();
            if let Some(project) = detect_project_from_snapshot(&snap) {
                stats.projects_seen.fetch_add(1, Ordering::Relaxed);

                let found =
                    artifact_candidates_from_snapshot(path, &project, &args_snapshot, &snap);
                if !found.is_empty() {
                    stats
                        .candidates_seen
                        .fetch_add(found.len(), Ordering::Relaxed);

                    for c in found {
                        if args_snapshot.ignore_recent_hours > 0 {
                            if is_recently_active(&c.path, args_snapshot.ignore_recent_hours) {
                                continue;
                            }
                        }
                        candidates.insert(c.path.clone(), c.clone());
                        own_candidates.push(c);
                    }
                }
            }

            if scan_cache.is_some() {
                let (files, bytes) = shallow_dir_stats(&snap);
                dir_records.lock().unwrap().push((
                    path.to_path_buf(),
                    DirRecord {
                        mtime: meta.as_ref().map(|m| m.mtime()).unwrap_or(0),
                        child_names_hash: child_names_hash(&snap),
                        own_files: files,
                        own_bytes: bytes,
                        own_candidates,
                    },
                ));
            }

            WalkState::Continue
        })
    });

    if let Some(cache) = &scan_cache {
        let records = std::mem::take(&mut *dir_records.lock().unwrap());
        let hits = std::mem::take(&mut *cache_hits.lock().unwrap());
        let staged = aggregate_subtrees(records, hits);
        if let Err(e) = cache.insert_batch(&staged) {
            // A cache write failure must never fail the scan itself.
            eprintln!("warning: failed to write scan cache: {e}");
        }
    }

    Ok(())
}

/// Per-directory bookkeeping collected during the parallel walk for every
/// freshly-visited (non-cache-hit) directory: its own, non-recursive stats
/// and the candidates found directly within it, plus enough identity
/// (mtime/hash) to write a cache entry once its subtree total is known.
/// Consumed by `aggregate_subtrees` after the walk completes.
struct DirRecord {
    mtime: i64,
    child_names_hash: u64,
    own_files: u64,
    own_bytes: u64,
    own_candidates: Vec<Candidate>,
}

/// Folds a cached directory's previously-recorded, true-recursive subtree
/// counts into `stats`, and re-inserts its cached candidates into the live
/// `candidates` set — used when a cache hit lets `scan_root` skip
/// re-descending into that subtree, so the pruned subtree still contributes
/// to totals and output exactly as a fresh scan would have.
fn fold_cached_stats(
    stats: &Stats,
    candidates: &DashMap<PathBuf, Candidate>,
    prev: &CachedDirEntry,
) {
    stats
        .dirs_seen
        .fetch_add(prev.dirs as usize, Ordering::Relaxed);
    stats
        .files_seen
        .fetch_add(prev.files as usize, Ordering::Relaxed);
    stats.bytes_seen.fetch_add(prev.bytes, Ordering::Relaxed);
    stats
        .candidates_seen
        .fetch_add(prev.candidates as usize, Ordering::Relaxed);
    for c in &prev.candidates_list {
        candidates.insert(c.path.clone(), c.clone());
    }
}

/// Computes true recursive subtree aggregates for every directory visited
/// during a `scan_root` call and returns the batch of `(path,
/// CachedDirEntry)` pairs to persist.
///
/// `records` holds one entry per freshly-visited directory with its own
/// (non-recursive) stats; `hits` holds one entry per directory pruned
/// because it hit the scan cache, whose `CachedDirEntry` is already a full
/// recursive aggregate from a prior scan. Every entry is folded into its
/// parent's running total, processing deepest paths first so a directory is
/// only finalized after every one of its descendants has already
/// contributed to it — a post-order reduction expressed as a single
/// depth-sorted pass rather than as recursion. Cache-hit directories
/// contribute to their ancestors' totals but are not re-emitted (nothing
/// about them changed); only freshly-visited directories get a new
/// `CachedDirEntry`.
fn aggregate_subtrees(
    records: Vec<(PathBuf, DirRecord)>,
    hits: Vec<(PathBuf, CachedDirEntry)>,
) -> Vec<(PathBuf, CachedDirEntry)> {
    struct Agg {
        dirs: u64,
        files: u64,
        bytes: u64,
        candidates: Vec<Candidate>,
        /// `Some((mtime, child_names_hash))` for freshly-visited directories,
        /// which need a new cache entry written; `None` for cache-hit
        /// directories, which only need to contribute to their ancestors.
        own: Option<(i64, u64)>,
    }

    let mut agg_map: std::collections::HashMap<PathBuf, Agg> = std::collections::HashMap::new();

    for (path, rec) in records {
        agg_map.insert(
            path,
            Agg {
                dirs: 1,
                files: rec.own_files,
                bytes: rec.own_bytes,
                candidates: rec.own_candidates,
                own: Some((rec.mtime, rec.child_names_hash)),
            },
        );
    }
    for (path, prev) in hits {
        agg_map.insert(
            path,
            Agg {
                dirs: prev.dirs,
                files: prev.files,
                bytes: prev.bytes,
                candidates: prev.candidates_list,
                own: None,
            },
        );
    }

    let mut paths: Vec<PathBuf> = agg_map.keys().cloned().collect();
    // Deepest paths first, so a directory only rolls up into its parent
    // once every one of its own descendants has already rolled up into it.
    paths.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    for path in &paths {
        let Some(parent) = path.parent() else {
            continue;
        };
        let (c_dirs, c_files, c_bytes, c_candidates) = {
            let Some(child) = agg_map.get(path) else {
                continue;
            };
            (
                child.dirs,
                child.files,
                child.bytes,
                child.candidates.clone(),
            )
        };
        if let Some(parent_agg) = agg_map.get_mut(parent) {
            parent_agg.dirs += c_dirs;
            parent_agg.files += c_files;
            parent_agg.bytes += c_bytes;
            parent_agg.candidates.extend(c_candidates);
        }
    }

    agg_map
        .into_iter()
        .filter_map(|(path, agg)| {
            let (mtime, child_names_hash) = agg.own?;
            Some((
                path,
                CachedDirEntry {
                    mtime,
                    child_names_hash,
                    files: agg.files,
                    bytes: agg.bytes,
                    dirs: agg.dirs,
                    candidates: agg.candidates.len() as u64,
                    candidates_list: agg.candidates,
                },
            ))
        })
        .collect()
}

/// Computes file count / physical byte total for a directory's *immediate*
/// file children only (non-recursive) — the cheap counts available from a
/// `DirSnapshot` plus a `stat` per file. `scan_root` combines this per-directory
/// "own" total with descendant totals in `aggregate_subtrees` to produce the
/// true recursive aggregate stored in `CachedDirEntry`.
fn shallow_dir_stats(snap: &DirSnapshot) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    for child in &snap.children {
        if child.is_file() {
            if let Ok(meta) = std::fs::metadata(&child.path) {
                files += 1;
                bytes += meta.blocks() * 512;
            }
        }
    }
    (files, bytes)
}

// ── Disk breakdown by top-level directory ─────────────────────────────────────

/// Walks `root` in parallel and returns total byte usage bucketed by each
/// immediate child of `root`, sorted largest-first.
///
/// Hidden directories are included. No artifact-specific pruning is applied —
/// every file under every child is counted.
pub fn breakdown_sizes(root: &Path) -> anyhow::Result<Vec<(PathBuf, u64)>> {
    let buckets: Arc<DashMap<PathBuf, AtomicU64>> = Arc::new(DashMap::new());

    // Pre-populate one bucket per immediate child of root.
    for entry in std::fs::read_dir(root)?.flatten() {
        buckets.insert(entry.path(), AtomicU64::new(0));
    }

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .same_file_system(true)
        .threads(threads);

    let root_pb = root.to_path_buf();
    let buckets_w = buckets.clone();

    builder.build_parallel().run(|| {
        let buckets = buckets_w.clone();
        let root = root_pb.clone();
        Box::new(move |result| {
            let entry = match result {
                Ok(e) => e,
                Err(_) => return WalkState::Continue,
            };
            let Ok(meta) = entry.metadata() else {
                return WalkState::Continue;
            };
            if !meta.is_file() {
                return WalkState::Continue;
            }
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(&root) else {
                return WalkState::Continue;
            };
            if let Some(first) = rel.components().next() {
                let top = root.join(first);
                if let Some(bucket) = buckets.get(&top) {
                    // Physical allocation, not logical size (handles sparse files).
                    bucket.fetch_add(meta.blocks() * 512, Ordering::Relaxed);
                }
            }
            WalkState::Continue
        })
    });

    let mut results: Vec<(PathBuf, u64)> = buckets
        .iter()
        .map(|e| (e.key().clone(), e.value().load(Ordering::Relaxed)))
        .collect();

    results.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(results)
}

// ── Cargo target-dir discovery ────────────────────────────────────────────────

/// Walks `root` and returns every `target/` directory whose parent contains
/// `Cargo.toml` or `Cargo.lock`, together with its physical size on disk.
///
/// Uses jwalk's `process_read_dir` to prune `target/` dirs from descent so
/// their contents are never walked — only the top-level entry is inspected.
/// Size is computed with a second parallel walk over each found dir.
pub fn find_cargo_target_dirs(root: &Path) -> anyhow::Result<Vec<(PathBuf, u64)>> {
    use jwalk::{Parallelism, WalkDir};
    use rayon::prelude::*;

    let found: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
    let found_w = found.clone();

    WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(Parallelism::RayonNewPool(0))
        .process_read_dir(move |_, parent, _, children| {
            let is_cargo_project =
                parent.join("Cargo.toml").exists() || parent.join("Cargo.lock").exists();

            if is_cargo_project {
                children.retain(|e| {
                    let Ok(e) = e else { return true };
                    let fname = e.file_name().to_string_lossy();
                    if e.file_type().is_dir() && (fname == "target" || fname.starts_with("target_"))
                    {
                        found_w.lock().unwrap().push(parent.join(e.file_name()));
                        return false; // prune — don't descend into target/
                    }
                    true
                });
            }
        })
        .into_iter()
        .for_each(|_| {});

    let dirs = {
        let mut guard = found.lock().unwrap();
        std::mem::take(&mut *guard)
    };

    // Compute physical sizes in parallel across all found target dirs.
    let mut with_sizes: Vec<(PathBuf, u64)> = dirs
        .into_par_iter()
        .map(|dir| {
            let size = physical_dir_size(&dir);
            (dir, size)
        })
        .collect();

    with_sizes.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    Ok(with_sizes)
}

/// Computes physical disk usage of a directory tree (blocks × 512).
pub fn physical_dir_size(path: &Path) -> u64 {
    use jwalk::{Parallelism, WalkDir};
    WalkDir::new(path)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(Parallelism::RayonNewPool(0))
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.blocks() * 512)
        .sum()
}

// ── Fast large-file search ─────────────────────────────────────────────────────

/// Finds every file >= `min_bytes` under `root` as fast as possible.
///
/// Uses `jwalk` (rayon work-stealing + `fstatat`) which is ~2-3× faster than
/// `ignore::WalkBuilder` for raw traversal because there is no gitignore
/// overhead. Directories on a different device than `root` are pruned before
/// descent to avoid `/dev`, network mounts, and APFS snapshot volumes.
///
/// Returns results sorted largest-first. `progress` is called with
/// `(files_scanned, large_files_found)` periodically so the caller can drive
/// a live display.
pub fn find_large_files(
    root: &Path,
    min_bytes: u64,
    progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> anyhow::Result<Vec<(PathBuf, u64)>> {
    use jwalk::{Parallelism, WalkDir};
    use std::os::unix::fs::MetadataExt as _;

    let root_dev = std::fs::metadata(root)
        .with_context(|| format!("Cannot stat root: {}", root.display()))?
        .dev();

    let files_scanned = Arc::new(AtomicU64::new(0));
    let large_found = Arc::new(AtomicU64::new(0));

    let files_scanned_cb = files_scanned.clone();
    let large_found_cb = large_found.clone();

    // Spawn a progress-reporting thread that fires every 250 ms.
    let progress = Arc::new(progress);
    let progress_cb = progress.clone();
    let files_for_thread = files_scanned.clone();
    let large_for_thread = large_found.clone();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if stop_rx.try_recv().is_ok() {
            break;
        }
        progress_cb(
            files_for_thread.load(Ordering::Relaxed),
            large_for_thread.load(Ordering::Relaxed),
        );
    });

    let mut results: Vec<(PathBuf, u64)> = WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(Parallelism::RayonNewPool(0)) // 0 = use all cores
        .process_read_dir(move |_, _, _, children| {
            // Prune entries on a different device before we ever descend.
            children.retain(|e| {
                let Ok(e) = e else { return true };
                // DirEntry from jwalk carries metadata already read by fstatat.
                e.metadata().map(|m| m.dev() == root_dev).unwrap_or(true)
            });
        })
        .into_iter()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let ft = entry.file_type();
            if !ft.is_file() {
                return None;
            }
            let meta = entry.metadata().ok()?;
            // Physical bytes on disk, not logical file size.
            // APFS sparse files (Docker.raw, VM images) report a huge `len()`
            // but only allocate the blocks they actually wrote.
            let physical = meta.blocks() * 512;
            files_scanned_cb.fetch_add(1, Ordering::Relaxed);
            if physical >= min_bytes {
                large_found_cb.fetch_add(1, Ordering::Relaxed);
                Some((entry.path(), physical))
            } else {
                None
            }
        })
        .collect();

    let _ = stop_tx.send(());

    // Final progress tick.
    progress(
        files_scanned.load(Ordering::Relaxed),
        large_found.load(Ordering::Relaxed),
    );

    results.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    Ok(results)
}

// ── macOS-aware force deletion ─────────────────────────────────────────────────

/// Removes a directory tree, clearing macOS immutable flags (`UF_IMMUTABLE`)
/// and making all entries user-writable before deletion.
///
/// Regular `std::fs::remove_dir_all` fails on paths like `~/.npm/_cacache`
/// because npm sets the `uchg` immutable flag on blobs. This function does a
/// two-pass fix — flags then permissions — before the final remove.
///
/// Returns an error with a hint to use `sudo` if files are root-owned.
pub fn force_remove_dir_all(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if !path.exists() {
        return Ok(());
    }

    // Pass 1 — clear macOS immutable flags (nouchg = user immutable, noschg = sys immutable).
    // Ignore errors: chflags will fail on root-owned files; we surface that later.
    let _ = std::process::Command::new("chflags")
        .args(["-R", "nouchg,noschg"])
        .arg(path)
        .output();

    // Pass 2 — make every entry user-writable so remove_dir_all can proceed.
    let mut builder = WalkBuilder::new(path);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .same_file_system(true);

    for result in builder.build() {
        let Ok(entry) = result else { continue };
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let mode = if is_dir { 0o700u32 } else { 0o600u32 };
        let _ = std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(mode));
    }

    // Final removal.
    std::fs::remove_dir_all(path).with_context(|| {
        format!(
            "Could not remove {}. Some entries may be root-owned — try: sudo rm -rf {}",
            path.display(),
            path.display()
        )
    })
}

// ── Plan-bound deletion ────────────────────────────────────────────────────────

/// Deletes a single file from the filesystem.
///
/// Returns `Ok(())` on success. Returns `Err` if the path is not a file or
/// if the OS-level deletion fails.
///
/// **Callers must hold a validated `DeletionPlan` before invoking this.**
pub fn delete_file(path: &Path) -> anyhow::Result<()> {
    if !path.is_file() {
        anyhow::bail!(
            "delete_file: expected a file but path is not a file: {}",
            path.display()
        );
    }
    std::fs::remove_file(path)?;
    Ok(())
}

/// Recursively deletes a directory and all its contents.
///
/// Returns `Ok(())` on success. Returns `Err` if the path is not a directory or
/// if the OS-level deletion fails.
///
/// **Callers must hold a validated `DeletionPlan` before invoking this.**
pub fn delete_dir_all(path: &Path) -> anyhow::Result<()> {
    if !path.is_dir() {
        anyhow::bail!(
            "delete_dir_all: expected a directory but path is not a dir: {}",
            path.display()
        );
    }
    // Try standard removal first as it's fastest.
    if let Err(_) = std::fs::remove_dir_all(path) {
        // Fallback to macOS-specific force removal if standard fails (e.g. immutable flags).
        force_remove_dir_all(path)?;
    }
    Ok(())
}

// ── Tool-root accounting ───────────────────────────────────────────────────────

fn record_tool_root_dir(path: &Path, defs: &[ToolRootDef], accs: &DashMap<PathBuf, ToolRootAcc>) {
    for def in defs {
        if path.starts_with(&def.path) {
            if let Some(acc) = accs.get(&def.path) {
                acc.dirs.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

fn record_tool_root_file(
    path: &Path,
    bytes: u64,
    mtime: i64,
    defs: &[ToolRootDef],
    accs: &DashMap<PathBuf, ToolRootAcc>,
) {
    for def in defs {
        if path.starts_with(&def.path) {
            if let Some(acc) = accs.get(&def.path) {
                acc.files.fetch_add(1, Ordering::Relaxed);
                acc.bytes.fetch_add(bytes, Ordering::Relaxed);

                let mut current = acc.newest_mtime.load(Ordering::Relaxed);
                while mtime > current {
                    match acc.newest_mtime.compare_exchange(
                        current,
                        mtime,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => {
                            *acc.newest_mtime_path.lock().unwrap() = Some(path.to_path_buf());
                            break;
                        }
                        Err(actual) => current = actual,
                    }
                }
            }
        }
    }
}

/// Populates the tool root directories' own metadata fields inside `ToolRootAcc`
/// by executing live OS/filesystem calls in the integration layer.
pub fn populate_tool_roots_metadata(defs: &[ToolRootDef], accs: &DashMap<PathBuf, ToolRootAcc>) {
    for def in defs {
        if let Some(acc) = accs.get(&def.path) {
            if let Ok(meta) = std::fs::symlink_metadata(&def.path) {
                if let Ok(created) = meta.created() {
                    acc.created_unix.store(
                        crate::domain::time::system_time_to_unix(created),
                        Ordering::Relaxed,
                    );
                }
                if let Ok(accessed) = meta.accessed() {
                    acc.accessed_unix.store(
                        crate::domain::time::system_time_to_unix(accessed),
                        Ordering::Relaxed,
                    );
                }
                if let Ok(modified) = meta.modified() {
                    acc.modified_unix.store(
                        crate::domain::time::system_time_to_unix(modified),
                        Ordering::Relaxed,
                    );
                }
                let ctime = std::os::unix::fs::MetadataExt::ctime(&meta);
                acc.ctime_unix.store(ctime, Ordering::Relaxed);
            }
        }
    }
}
