//! Filesystem traversal, size estimation, and plan-bound deletion.
//!
//! **Integration layer rule**: All `std::fs` and OS calls live here.
//! Domain functions receive inert DTOs; this module builds those DTOs
//! from live OS handles.

use dashmap::DashMap;
use ignore::{WalkBuilder, WalkState};
use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use crate::domain::artifact::{
    artifact_candidates_from_snapshot, detect_project_from_snapshot, is_global_cache,
    is_macos_os_dir, traversal_barrier_names, ArgsSnapshot, Candidate, DirSnapshot, EntryKind,
    EntrySnapshot,
};
use crate::domain::audit::Stats;
use crate::domain::tool_roots::{ToolRootAcc, ToolRootDef};

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
                // Symlinks and unknowns: check via metadata.
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

// ── Size estimation ────────────────────────────────────────────────────────────

/// Estimates size of a path recursively.
pub fn estimate_size(path: &Path, stats: Arc<Stats>) -> u64 {
    if path.is_file() {
        return std::fs::metadata(path)
            .map(|m| {
                stats.bytes_seen.fetch_add(m.len(), Ordering::Relaxed);
                stats.files_seen.fetch_add(1, Ordering::Relaxed);
                m.len()
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
                        size += meta.len();
                        stats.bytes_seen.fetch_add(meta.len(), Ordering::Relaxed);
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

// ── Parallel traversal ─────────────────────────────────────────────────────────

/// Recursively traverses a root path to find candidate files/folders.
///
/// For each directory encountered, this function builds an inert `DirSnapshot`
/// and passes it to the pure domain functions `detect_project_from_snapshot`
/// and `artifact_candidates_from_snapshot`.
pub fn scan_root(
    root: &Path,
    args: &ArgsSnapshot,
    candidates: Arc<Mutex<BTreeSet<Candidate>>>,
    stats: Arc<Stats>,
    known_tool_defs: &[ToolRootDef],
    tool_accs: Arc<DashMap<PathBuf, ToolRootAcc>>,
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

    let traversal_barriers = traversal_barrier_names();
    let root_for_filter = root.to_path_buf();
    let stats_for_filter = stats.clone();

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
            if traversal_barriers.contains(name) {
                stats_for_filter.pruned_dirs.fetch_add(1, Ordering::Relaxed);
                return false;
            }
        }
        true
    });

    let walker = builder.build_parallel();
    let args_snapshot = *args;
    let known_tool_defs = known_tool_defs.to_vec();

    walker.run(|| {
        let candidates = candidates.clone();
        let stats = stats.clone();
        let known_tool_defs = known_tool_defs.clone();
        let tool_accs = tool_accs.clone();

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

            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    stats.files_seen.fetch_add(1, Ordering::Relaxed);
                    stats.bytes_seen.fetch_add(meta.len(), Ordering::Relaxed);
                    if args_snapshot.tool_roots {
                        record_tool_root_file(
                            path,
                            meta.len(),
                            meta.mtime(),
                            &known_tool_defs,
                            &tool_accs,
                        );
                    }
                } else if meta.is_dir() {
                    stats.dirs_seen.fetch_add(1, Ordering::Relaxed);
                    if args_snapshot.tool_roots {
                        record_tool_root_dir(path, &known_tool_defs, &tool_accs);
                    }
                }
            }

            let is_dir = entry
                .file_type()
                .map(|ft| ft.is_dir())
                .unwrap_or_else(|| path.is_dir());

            if !is_dir {
                return WalkState::Continue;
            }

            // Build an inert snapshot of this directory's children and pass it
            // to pure domain functions. No live OS handles are passed into domain.
            let snap = read_dir_snapshot(path);
            if let Some(project) = detect_project_from_snapshot(&snap) {
                stats.projects_seen.fetch_add(1, Ordering::Relaxed);

                let found =
                    artifact_candidates_from_snapshot(path, &project, &args_snapshot, &snap);
                if !found.is_empty() {
                    stats
                        .candidates_seen
                        .fetch_add(found.len(), Ordering::Relaxed);

                    let mut lock = candidates.lock().unwrap();
                    for c in found {
                        lock.insert(c);
                    }
                }
            }
            WalkState::Continue
        })
    });

    Ok(())
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
            let is_cargo_project = parent.join("Cargo.toml").exists()
                || parent.join("Cargo.lock").exists();

            if is_cargo_project {
                children.retain(|e| {
                    let Ok(e) = e else { return true };
                    if e.file_name() == "target" && e.file_type().is_dir() {
                        found_w
                            .lock()
                            .unwrap()
                            .push(parent.join("target"));
                        return false; // prune — don't descend into target/
                    }
                    true
                });
            }
        })
        .into_iter()
        .for_each(|_| {});

    let dirs = Arc::try_unwrap(found).unwrap().into_inner().unwrap();

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
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if stop_rx.try_recv().is_ok() {
                break;
            }
            progress_cb(
                files_for_thread.load(Ordering::Relaxed),
                large_for_thread.load(Ordering::Relaxed),
            );
        }
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
        let is_dir = entry
            .file_type()
            .map(|ft| ft.is_dir())
            .unwrap_or(false);
        let mode = if is_dir { 0o700u32 } else { 0o600u32 };
        let _ = std::fs::set_permissions(
            entry.path(),
            std::fs::Permissions::from_mode(mode),
        );
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
    std::fs::remove_dir_all(path)?;
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
pub fn populate_tool_roots_metadata(
    defs: &[ToolRootDef],
    accs: &DashMap<PathBuf, ToolRootAcc>,
) {
    for def in defs {
        if let Some(acc) = accs.get(&def.path) {
            if let Ok(meta) = std::fs::symlink_metadata(&def.path) {
                if let Ok(created) = meta.created() {
                    acc.created_unix.store(crate::domain::time::system_time_to_unix(created), Ordering::Relaxed);
                }
                if let Ok(accessed) = meta.accessed() {
                    acc.accessed_unix.store(crate::domain::time::system_time_to_unix(accessed), Ordering::Relaxed);
                }
                if let Ok(modified) = meta.modified() {
                    acc.modified_unix.store(crate::domain::time::system_time_to_unix(modified), Ordering::Relaxed);
                }
                let ctime = std::os::unix::fs::MetadataExt::ctime(&meta);
                acc.ctime_unix.store(ctime, Ordering::Relaxed);
            }
        }
    }
}
