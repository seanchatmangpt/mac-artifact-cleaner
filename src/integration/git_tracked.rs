//! Git-tracked-content probe for deletion candidates.
//!
//! Directory-name classification (`dist`, `build`, `_build`, `.agents`, …)
//! cannot tell a regenerable output from committed content: a GitHub Action
//! checkout under `~/.cache/act/<action>@<ref>/dist` ships its compiled entry
//! point *in git*, and a repo may commit `.agents/` skill definitions or a
//! vendored `_build`. On 2026-09-22 a real plan held 60 such items (89 MB of
//! tracked files). Any candidate containing at least one tracked path is not
//! a cache — deleting it destroys committed work — so plan build drops it.

use std::path::Path;

/// Returns `true` when `path` (a file, or a directory at any depth) contains
/// at least one file tracked by the git repository enclosing it.
///
/// Returns `false` when `path` is not inside a work tree, when its parent
/// does not exist, or when `git` itself is unavailable — the same answer a
/// non-git directory gets, since there is no committed content to protect.
pub fn contains_git_tracked_files(path: &Path) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    if !parent.is_dir() {
        return false;
    }
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(parent)
        .args(["ls-files", "-z", "--"])
        .arg(name)
        .output();
    match out {
        Ok(o) if o.status.success() => !o.stdout.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git").arg("-C").arg(dir).args(args).status().expect("run git");
        assert!(st.success(), "git {args:?} failed");
    }

    #[test]
    fn tracked_dist_is_detected_and_untracked_build_is_not() {
        let repo = tempfile::tempdir().expect("tempdir");
        let r = repo.path();
        git(r, &["init", "-q"]);
        std::fs::create_dir_all(r.join("dist")).unwrap();
        std::fs::write(r.join("dist/index.js"), b"module.exports = 1;").unwrap();
        std::fs::create_dir_all(r.join("build/deep")).unwrap();
        std::fs::write(r.join("build/deep/out.o"), b"obj").unwrap();
        git(r, &["add", "dist/index.js"]);

        // Positive: committed-by-index content inside the candidate.
        assert!(contains_git_tracked_files(&r.join("dist")));
        // Negative: untracked build output in the same repo.
        assert!(!contains_git_tracked_files(&r.join("build")));
    }

    #[test]
    fn non_repo_and_missing_paths_are_not_tracked() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("target")).unwrap();
        assert!(!contains_git_tracked_files(&dir.path().join("target")));
        assert!(!contains_git_tracked_files(&dir.path().join("missing/target")));
    }
}
