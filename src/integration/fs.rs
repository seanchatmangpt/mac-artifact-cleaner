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
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

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
