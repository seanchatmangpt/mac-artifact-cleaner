//! Read-only launchd plist scanner.
//!
//! Enumerates `*.plist` files in the given directories, converts each with
//! `plutil -convert json -o - <file>` (no plist crate dependency), and
//! produces inert [`LaunchdPlistFacts`] for the pure domain classifier. It
//! never loads, unloads, or deletes anything.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};

use crate::domain::launchd_orphans::LaunchdPlistFacts;

/// A plist or directory that could not be read or parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchdScanError {
    /// Path that failed.
    pub path: String,
    /// Reason for the failure.
    pub reason: String,
}

/// Result of scanning a set of launchd directories.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LaunchdScan {
    /// Facts for every plist that converted successfully.
    pub facts: Vec<LaunchdPlistFacts>,
    /// Plists/directories that could not be read (malformed plist, permissions).
    pub errors: Vec<LaunchdScanError>,
    /// Directories that do not exist (skipped, not an error).
    pub missing_dirs: Vec<String>,
}

/// The default launchd directories for the current user.
pub fn default_launchd_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Library/LaunchAgents"));
    }
    dirs.push(PathBuf::from("/Library/LaunchAgents"));
    dirs.push(PathBuf::from("/Library/LaunchDaemons"));
    dirs
}

/// Scans `dirs` (non-recursive) for `*.plist` files and gathers facts.
pub fn scan_launchd_dirs(dirs: &[PathBuf]) -> LaunchdScan {
    let mut scan = LaunchdScan::default();
    for dir in dirs {
        if !dir.exists() {
            scan.missing_dirs.push(dir.display().to_string());
            continue;
        }
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                scan.errors.push(LaunchdScanError {
                    path: dir.display().to_string(),
                    reason: format!("read_dir: {e}"),
                });
                continue;
            }
        };
        let mut plists: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "plist"))
            .collect();
        plists.sort();
        for p in plists {
            match read_facts(&p) {
                Ok(f) => scan.facts.push(f),
                Err(reason) => {
                    scan.errors.push(LaunchdScanError { path: p.display().to_string(), reason })
                }
            }
        }
    }
    scan
}

fn read_facts(plist: &Path) -> Result<LaunchdPlistFacts, String> {
    let out = Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(plist)
        .output()
        .map_err(|e| format!("spawn plutil: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(format!("plutil failed: {}", msg.trim()));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("json parse: {e}"))?;
    Ok(facts_from_json(plist, &v))
}

fn facts_from_json(plist: &Path, v: &serde_json::Value) -> LaunchdPlistFacts {
    let label = v.get("Label").and_then(|l| l.as_str()).map(String::from);
    let program = v
        .get("Program")
        .and_then(|p| p.as_str())
        .or_else(|| {
            v.get("ProgramArguments")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|x| x.as_str())
        })
        .map(String::from);
    let program_exists =
        program.as_deref().filter(|p| p.starts_with('/')).is_some_and(|p| Path::new(p).exists());
    LaunchdPlistFacts { label, plist_path: plist.display().to_string(), program, program_exists }
}
