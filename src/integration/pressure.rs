//! Pressure-triggered reclaim: I/O side.
//!
//! Gathers the facts the pure decisions in [`crate::domain::pressure`] need —
//! live process cwds (`lsof`), shallow newest-mtime per candidate, and the
//! persisted last-action stamps that make the cooldown survive a restart of
//! the launchd job.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::UNIX_EPOCH,
};

use crate::domain::pressure::parse_lsof_cwds;

/// Current working directories of every process `lsof` can see for this user,
/// via `lsof -a -d cwd -Fn` (field output, `n<path>` lines).
///
/// `lsof` exits 1 whenever it could not inspect *some* process (common for
/// other users' processes), so a nonzero exit with usable stdout is accepted;
/// only a spawn failure or an empty result with a nonzero exit is an error.
/// Callers must treat an error as "cwds unknown" and refuse the builds
/// reclaim rather than proceed without the exclusion.
pub fn live_process_cwds() -> anyhow::Result<Vec<PathBuf>> {
    let out = Command::new("lsof").args(["-a", "-d", "cwd", "-Fn"]).output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let cwds = parse_lsof_cwds(&stdout);
    if cwds.is_empty() && !out.status.success() {
        anyhow::bail!(
            "lsof -a -d cwd -Fn failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(cwds)
}

fn mtime_unix(meta: &std::fs::Metadata) -> Option<i64> {
    let t = meta.modified().ok()?;
    let secs = t.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()
}

/// Newest mtime (Unix seconds) of `path` and its descendants down to `depth`
/// levels, without following symlinks. Build tools write into
/// `target/debug/...`, `_build/dev/...` rather than the top dir itself, so
/// the top dir's own mtime alone understates recency. Returns `None` if
/// `path` itself cannot be stat'ed.
pub fn newest_mtime_shallow(path: &Path, depth: usize) -> Option<i64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    let mut newest = mtime_unix(&meta)?;
    if depth == 0 || !meta.is_dir() {
        return Some(newest);
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Some(m) = newest_mtime_shallow(&entry.path(), depth - 1) {
                newest = newest.max(m);
            }
        }
    }
    Some(newest)
}

/// `~/.oclnr/<name>` — per-strategy last-action stamp location.
pub fn stamp_path(name: &str) -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    Ok(home.join(".oclnr").join(name))
}

/// Reads a Unix-seconds stamp file; `None` if absent or unparsable.
pub fn read_stamp(path: &Path) -> Option<i64> {
    std::fs::read_to_string(path).ok()?.trim().parse::<i64>().ok()
}

/// Writes `now_unix` to a stamp file, creating its parent directory.
pub fn write_stamp(path: &Path, now_unix: i64) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{now_unix}\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_roundtrip_in_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested/stamp");
        assert_eq!(read_stamp(&p), None);
        write_stamp(&p, 1_234_567).unwrap();
        assert_eq!(read_stamp(&p), Some(1_234_567));
        std::fs::write(&p, "garbage").unwrap();
        assert_eq!(read_stamp(&p), None);
    }

    #[test]
    fn newest_mtime_sees_nested_writes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let debug = target.join("debug");
        std::fs::create_dir_all(&debug).unwrap();
        let f = debug.join("out.rlib");
        std::fs::write(&f, b"x").unwrap();
        // Push the file's mtime into the future so it is strictly the newest.
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&f).unwrap().set_modified(future).unwrap();
        let expected = future.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;

        assert_eq!(newest_mtime_shallow(&target, 2), Some(expected));
        // Depth 0 sees only the top dir, not the nested write.
        assert!(newest_mtime_shallow(&target, 0).unwrap() < expected);
        assert_eq!(newest_mtime_shallow(&dir.path().join("missing"), 2), None);
    }

    #[test]
    fn live_process_cwds_includes_a_real_child_cwd() {
        // Real process: spawn `sleep` with a tempdir cwd and confirm lsof
        // reports it. Skips visibly if lsof is unavailable.
        if Command::new("lsof").arg("-v").output().is_err() {
            eprintln!("SKIP: lsof not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let mut child = Command::new("sleep").arg("30").current_dir(&canonical).spawn().unwrap();
        let cwds = live_process_cwds();
        let _ = child.kill();
        let _ = child.wait();
        let cwds = cwds.unwrap();
        assert!(cwds.contains(&canonical), "expected {} in lsof cwds", canonical.display());
    }
}
