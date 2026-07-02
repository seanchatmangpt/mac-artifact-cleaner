//! Git repository health scanning.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitRepoHealth {
    pub path: PathBuf,
    pub pack_size_bytes: u64,
    pub loose_objects: u64,
    pub worktrees: Vec<String>,
    pub dangling_worktrees: Vec<String>,
}

/// Scans `root` for git repositories (up to 4 levels deep) and returns
/// health information for each one found.
pub fn scan_git_repos(root: &Path) -> Result<Vec<GitRepoHealth>> {
    let mut repo_paths: Vec<PathBuf> = Vec::new();
    find_git_repos(root, 4, &mut repo_paths);

    let mut results = Vec::new();
    for repo_path in repo_paths {
        match inspect_repo(&repo_path) {
            Ok(health) => results.push(health),
            Err(_) => {
                // Skip repos that fail inspection (e.g. permissions, corrupt)
            }
        }
    }
    Ok(results)
}

fn inspect_repo(repo_path: &Path) -> Result<GitRepoHealth> {
    let (pack_size_bytes, loose_objects) = count_objects(repo_path);
    let (worktrees, dangling_worktrees) = list_worktrees(repo_path);

    Ok(GitRepoHealth {
        path: repo_path.to_path_buf(),
        pack_size_bytes,
        loose_objects,
        worktrees,
        dangling_worktrees,
    })
}

fn count_objects(repo_path: &Path) -> (u64, u64) {
    let output = Command::new("git")
        .args([
            "-C",
            repo_path.to_str().unwrap_or(""),
            "count-objects",
            "-vH",
        ])
        .output();

    let Ok(out) = output else {
        return (0, 0);
    };

    let text = String::from_utf8_lossy(&out.stdout);
    let mut pack_size_bytes: u64 = 0;
    let mut loose_objects: u64 = 0;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("size-pack:") {
            pack_size_bytes = parse_human_size(rest.trim());
        } else if let Some(rest) = line.strip_prefix("count:") {
            loose_objects = rest.trim().parse().unwrap_or(0);
        }
    }

    (pack_size_bytes, loose_objects)
}

/// Parse a human-readable size string like "4.50 MiB", "12.00 KiB", "2.30 GiB",
/// "1023 bytes" into a byte count.
///
/// This is a thin alias over the single shared implementation in
/// [`crate::integration::progress::parse_human_size`].
fn parse_human_size(s: &str) -> u64 {
    crate::integration::progress::parse_human_size(s)
}

fn list_worktrees(repo_path: &Path) -> (Vec<String>, Vec<String>) {
    let output = Command::new("git")
        .args([
            "-C",
            repo_path.to_str().unwrap_or(""),
            "worktree",
            "list",
            "--porcelain",
        ])
        .output();

    let Ok(out) = output else {
        return (vec![], vec![]);
    };

    let text = String::from_utf8_lossy(&out.stdout);
    let mut worktrees: Vec<String> = Vec::new();
    let mut dangling_worktrees: Vec<String> = Vec::new();

    for line in text.lines() {
        if let Some(wt_path) = line.strip_prefix("worktree ") {
            let wt = wt_path.trim().to_string();
            let exists = Path::new(&wt).exists();
            worktrees.push(wt.clone());
            if !exists {
                dangling_worktrees.push(wt);
            }
        }
    }

    (worktrees, dangling_worktrees)
}

fn find_git_repos(dir: &Path, depth: u8, repos: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let git_dir = dir.join(".git");
    if git_dir.exists() {
        repos.push(dir.to_path_buf());
        return; // don't recurse into git repos
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !path.is_symlink() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !matches!(
                    name,
                    "node_modules" | "target" | ".venv" | "__pycache__" | ".cargo"
                ) {
                    find_git_repos(&path, depth - 1, repos);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_human_size_kib() {
        assert_eq!(parse_human_size("12.00 KiB"), 12288);
    }

    #[test]
    fn parse_human_size_mib() {
        // 4.50 MiB = 4718592 bytes
        assert_eq!(parse_human_size("4.50 MiB"), 4718592);
    }

    #[test]
    fn parse_human_size_bytes() {
        assert_eq!(parse_human_size("0 bytes"), 0);
    }

    #[test]
    fn parse_human_size_gib() {
        // 1.00 GiB = 1073741824
        assert_eq!(parse_human_size("1.00 GiB"), 1073741824);
    }
}
