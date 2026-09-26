//! Git repository health scanning.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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
            Err(e) => {
                // Skip repos that fail inspection (e.g. permissions,
                // corrupt) — but say so, rather than silently under-
                // reporting git bloat with no signal anything was skipped.
                eprintln!("warning: skipping git health check for {}: {e}", repo_path.display());
            }
        }
    }
    Ok(results)
}

fn inspect_repo(repo_path: &Path) -> Result<GitRepoHealth> {
    let (pack_size_bytes, loose_objects) = count_objects(repo_path)?;
    let (worktrees, dangling_worktrees) = list_worktrees(repo_path)?;

    Ok(GitRepoHealth {
        path: repo_path.to_path_buf(),
        pack_size_bytes,
        loose_objects,
        worktrees,
        dangling_worktrees,
    })
}

/// Runs `git count-objects -vH` in `repo_path` and parses pack size / loose
/// object count from its output. Returns `Err` (rather than silently
/// reporting `(0, 0)`, indistinguishable from a genuinely clean repo) if
/// the subprocess itself fails to spawn or run — a missing `git` binary,
/// a permission error, or a corrupt repo are real failures, not evidence
/// of zero git bloat.
fn count_objects(repo_path: &Path) -> Result<(u64, u64)> {
    let output = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or(""), "count-objects", "-vH"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git count-objects failed: {}", stderr.trim());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut pack_size_bytes: u64 = 0;
    let mut loose_objects: u64 = 0;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("size-pack:") {
            pack_size_bytes = parse_human_size(rest.trim());
        } else if let Some(rest) = line.strip_prefix("count:") {
            loose_objects = rest.trim().parse().with_context(|| {
                format!("unparsable `count:` line from git count-objects: {rest}")
            })?;
        }
    }

    Ok((pack_size_bytes, loose_objects))
}

/// Parse a human-readable size string like "4.50 MiB", "12.00 KiB", "2.30 GiB",
/// "1023 bytes" into a byte count.
///
/// This is a thin alias over the single shared implementation in
/// [`crate::integration::progress::parse_human_size`].
fn parse_human_size(s: &str) -> u64 {
    crate::integration::progress::parse_human_size(s)
}

/// Runs `git worktree list --porcelain` in `repo_path` and parses worktree
/// paths / dangling status from its output. Returns `Err` (rather than
/// silently reporting `(vec![], vec![])`, indistinguishable from a repo with
/// no worktrees at all) if the subprocess itself fails to spawn or run —
/// mirrors the same discipline as `count_objects` above.
fn list_worktrees(repo_path: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let output = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or(""), "worktree", "list", "--porcelain"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree list failed: {}", stderr.trim());
    }

    let text = String::from_utf8_lossy(&output.stdout);
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

    Ok((worktrees, dangling_worktrees))
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
                if !matches!(name, "node_modules" | "target" | ".venv" | "__pycache__" | ".cargo") {
                    find_git_repos(&path, depth - 1, repos);
                }
            }
        }
    }
}

// ── Worktree standing (read-only) ────────────────────────────────────────────
//
// Gathers facts for `crate::domain::git_worktree` and never mutates any
// repository: every git invocation is a query, and `GIT_OPTIONAL_LOCKS=0`
// stops `git status` from opportunistically rewriting the index. Removal of
// worktrees / `git gc` is UNSUPPORTED here (see the domain module docs).

use crate::domain::git_worktree::{
    classify_worktree, count_stash_refs, gc_signals, parse_count_objects, parse_status_porcelain,
    parse_worktree_porcelain, summarize, RepoStanding, WorktreeFacts, WorktreeStanding,
    WorktreeStandingReport,
};

/// Runs a read-only git query in `dir`.
fn git_query(dir: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .with_context(|| format!("failed to spawn git {} in {}", args.join(" "), dir.display()))
}

/// Runs a git query and returns stdout, or `Err` on non-zero exit.
fn git_stdout(dir: &Path, args: &[&str]) -> Result<String> {
    let out = git_query(dir, args)?;
    if !out.status.success() {
        anyhow::bail!(
            "git {} failed in {}: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A main repository located from a discovered checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainRepo {
    /// Directory to run git in (main worktree, or the bare repo itself).
    pub repo: PathBuf,
    /// Absolute common git dir.
    pub git_dir: PathBuf,
}

/// Discovers main repositories under `roots` (up to `depth` levels). Linked
/// worktrees found during the walk are resolved to their owning repo via
/// `git rev-parse --git-common-dir`, and repos are de-duplicated by common
/// git dir, so a root containing only linked worktrees still reports their
/// main repo once.
pub fn discover_main_repos(roots: &[PathBuf], depth: u8) -> Vec<MainRepo> {
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        find_git_repos(root, depth, &mut found);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for checkout in found {
        let common = match git_stdout(
            &checkout,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        ) {
            Ok(s) => PathBuf::from(s.trim()),
            Err(e) => {
                eprintln!("warning: skipping {}: {e}", checkout.display());
                continue;
            }
        };
        let common = std::fs::canonicalize(&common).unwrap_or(common);
        if !seen.insert(common.clone()) {
            continue;
        }
        let repo = if common.file_name().and_then(|n| n.to_str()) == Some(".git") {
            common.parent().map(Path::to_path_buf).unwrap_or_else(|| common.clone())
        } else {
            common.clone()
        };
        out.push(MainRepo { repo, git_dir: common });
    }
    out
}

fn ref_exists(repo: &Path, full_ref: &str) -> bool {
    git_query(repo, &["show-ref", "--verify", "--quiet", full_ref])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Resolves the default branch name and the refs a branch must be an
/// ancestor of (local and/or `origin/`) to count as merged.
fn resolve_default_branch(repo: &Path) -> Option<(String, Vec<String>)> {
    let name =
        git_stdout(repo, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
            .ok()
            .and_then(|s| s.trim().strip_prefix("origin/").map(str::to_string))
            .or_else(|| {
                ["main", "master"]
                    .into_iter()
                    .find(|b| ref_exists(repo, &format!("refs/heads/{b}")))
                    .map(str::to_string)
            })?;
    let refs: Vec<String> = [format!("refs/heads/{name}"), format!("refs/remotes/origin/{name}")]
        .into_iter()
        .filter(|r| ref_exists(repo, r))
        .collect();
    if refs.is_empty() {
        return None;
    }
    Some((name, refs))
}

/// `Some(true)` if `branch` is an ancestor of any default ref, `Some(false)`
/// if every check answered "not an ancestor", `None` if any check errored
/// without a positive answer.
fn merged_into(repo: &Path, branch: &str, default_refs: &[String]) -> Option<bool> {
    let branch_ref = format!("refs/heads/{branch}");
    let mut uncertain = false;
    for r in default_refs {
        match git_query(repo, &["merge-base", "--is-ancestor", &branch_ref, r]) {
            Ok(o) if o.status.code() == Some(0) => return Some(true),
            Ok(o) if o.status.code() == Some(1) => {}
            _ => uncertain = true,
        }
    }
    if uncertain {
        None
    } else {
        Some(false)
    }
}

fn last_commit_unix(repo: &Path, head: &str) -> Option<i64> {
    git_stdout(repo, &["log", "-1", "--format=%ct", head]).ok()?.trim().parse().ok()
}

fn inspect_repo_standing(main: &MainRepo) -> RepoStanding {
    use rayon::prelude::*;

    let repo = &main.repo;
    let mut errors = Vec::new();

    let porcelain = match git_stdout(repo, &["worktree", "list", "--porcelain"]) {
        Ok(t) => parse_worktree_porcelain(&t),
        Err(e) => {
            errors.push(e.to_string());
            Vec::new()
        }
    };
    let default = resolve_default_branch(repo);
    if default.is_none() {
        errors.push("default branch could not be resolved".to_string());
    }
    let stash_subjects: Option<Vec<String>> =
        match git_stdout(repo, &["stash", "list", "--format=%gs"]) {
            Ok(t) => Some(t.lines().map(str::to_string).collect()),
            Err(e) => {
                errors.push(e.to_string());
                None
            }
        };
    let count_objects = match git_stdout(repo, &["count-objects", "-v"]) {
        Ok(t) => {
            let parsed = parse_count_objects(&t);
            if parsed.is_none() {
                errors.push("unparsable `git count-objects -v` output".to_string());
            }
            parsed
        }
        Err(e) => {
            errors.push(e.to_string());
            None
        }
    };
    let gc = count_objects.as_ref().map(gc_signals).unwrap_or_default();
    let git_dir_bytes = crate::integration::fs::physical_dir_size(&main.git_dir);
    let sub_size = |name: &str| {
        let p = main.git_dir.join(name);
        if p.is_dir() {
            crate::integration::fs::physical_dir_size(&p)
        } else {
            0
        }
    };
    let modules_bytes = sub_size("modules");
    let worktrees_admin_bytes = sub_size("worktrees");

    let worktrees: Vec<WorktreeStanding> = porcelain
        .par_iter()
        .enumerate()
        .filter(|(i, wt)| !(*i == 0 && wt.bare))
        .map(|(i, wt)| {
            let path = PathBuf::from(&wt.path);
            let is_main = i == 0;
            let exists = path.is_dir();
            let status = if exists && !wt.prunable && !is_main {
                git_stdout(
                    &path,
                    &[
                        "status",
                        "--porcelain=v1",
                        "--untracked-files=normal",
                        "--ignore-submodules=none",
                    ],
                )
                .ok()
                .map(|t| parse_status_porcelain(&t))
            } else {
                None
            };
            let merged = match (&wt.branch, &default) {
                (Some(b), Some((_, refs))) if !is_main => merged_into(repo, b, refs),
                _ => None,
            };
            let stash_refs = match (&wt.branch, &stash_subjects) {
                (Some(b), Some(subj)) => Some(count_stash_refs(subj, b)),
                (None, Some(_)) => Some(0),
                _ => None,
            };
            let facts = WorktreeFacts {
                is_main,
                prunable: wt.prunable,
                locked: wt.locked,
                exists,
                branch: wt.branch.clone(),
                detached: wt.detached,
                default_branch: default.as_ref().map(|(n, _)| n.clone()),
                merged_into_default: merged,
                status,
                stash_refs,
            };
            let verdict = classify_worktree(&facts);
            let last = wt.head.as_deref().and_then(|h| last_commit_unix(repo, h));
            // The main worktree's directory contains the repo itself (and
            // often nested worktrees); it is never reclaimable, so it is
            // not walked.
            let size_bytes = if exists && !is_main {
                crate::integration::fs::physical_dir_size(&path)
            } else {
                0
            };
            WorktreeStanding {
                path: wt.path.clone(),
                branch: wt.branch.clone(),
                head: wt.head.clone(),
                last_commit_unix: last,
                last_commit: last
                    .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                    .map(|d| d.to_rfc3339()),
                size_bytes,
                facts,
                verdict,
            }
        })
        .collect();

    RepoStanding {
        repo: repo.display().to_string(),
        git_dir: main.git_dir.display().to_string(),
        git_dir_bytes,
        modules_bytes,
        worktrees_admin_bytes,
        default_branch: default.map(|(n, _)| n),
        count_objects,
        gc_signals: gc,
        worktrees,
        errors,
    }
}

/// Builds the read-only worktree standing report for every main repo
/// discovered under `roots`.
pub fn worktree_standing(roots: &[PathBuf], depth: u8) -> WorktreeStandingReport {
    let repos: Vec<RepoStanding> =
        discover_main_repos(roots, depth).iter().map(inspect_repo_standing).collect();
    let summary = summarize(&repos);
    WorktreeStandingReport {
        version: 1,
        generated_unix: chrono::Utc::now().timestamp(),
        roots: roots.iter().map(|r| r.display().to_string()).collect(),
        repos,
        summary,
        removal: "UNSUPPORTED".to_string(),
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

    // ── Worktree standing: real git, real tempdirs (no doubles) ─────────────

    use crate::domain::git_worktree::{WorktreeClass, WorktreeReason};

    /// Runs the real `git` binary with an isolated config so the user's
    /// global settings (signing, hooks) cannot affect fixture creation.
    fn g(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) {
        std::fs::write(dir.join(name), body).expect("write");
        g(dir, &["add", name]);
        g(dir, &["commit", "-q", "-m", msg]);
    }

    /// Builds a repo with one worktree per classification and returns
    /// (tempdir guard, repo path, worktree parent path).
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canon");
        let repo = base.join("repo");
        let wts = base.join("wts");
        std::fs::create_dir_all(&repo).expect("mkdir");
        std::fs::create_dir_all(&wts).expect("mkdir");
        g(&repo, &["init", "-q", "-b", "main"]);
        commit_file(&repo, "a.txt", "a", "init");

        // merged: branch at main's HEAD, plus a merged feature commit.
        let merged = wts.join("merged");
        g(&repo, &["worktree", "add", "-q", "-b", "merged", merged.to_str().unwrap()]);
        commit_file(&merged, "m.txt", "m", "feature");
        g(&repo, &["merge", "-q", "--no-ff", "-m", "merge", "merged"]);

        // unmerged: a commit not in main.
        let unmerged = wts.join("unmerged");
        g(&repo, &["worktree", "add", "-q", "-b", "unmerged", unmerged.to_str().unwrap()]);
        commit_file(&unmerged, "u.txt", "u", "wip");

        // dirty: merged branch but a modified tracked file.
        let dirty = wts.join("dirty");
        g(&repo, &["worktree", "add", "-q", "-b", "dirty", dirty.to_str().unwrap()]);
        std::fs::write(dirty.join("a.txt"), "changed").expect("write");

        // untracked: merged branch with an untracked, non-ignored file.
        let untracked = wts.join("untracked");
        g(&repo, &["worktree", "add", "-q", "-b", "untracked", untracked.to_str().unwrap()]);
        std::fs::write(untracked.join("new.txt"), "x").expect("write");

        // ignored: merged branch with only an ignored file -> still clean.
        let ignored = wts.join("ignored");
        g(&repo, &["worktree", "add", "-q", "-b", "ignored", ignored.to_str().unwrap()]);
        std::fs::write(ignored.join("junk.log"), "x").expect("write");
        std::fs::write(repo.join(".git/info/exclude"), "*.log\n").expect("write");

        // stashed: merged and clean, but a stash references the branch.
        let stashed = wts.join("stashed");
        g(&repo, &["worktree", "add", "-q", "-b", "stashed", stashed.to_str().unwrap()]);
        std::fs::write(stashed.join("a.txt"), "stash me").expect("write");
        g(&stashed, &["stash", "push", "-q"]);

        // detached HEAD.
        let detached = wts.join("detached");
        g(&repo, &["worktree", "add", "-q", "--detach", detached.to_str().unwrap()]);

        // locked.
        let locked = wts.join("locked");
        g(&repo, &["worktree", "add", "-q", "-b", "locked", locked.to_str().unwrap()]);
        g(&repo, &["worktree", "lock", locked.to_str().unwrap()]);

        // prunable: directory removed out from under git (inside tempdir).
        let gone = wts.join("gone");
        g(&repo, &["worktree", "add", "-q", "-b", "gone", gone.to_str().unwrap()]);
        std::fs::remove_dir_all(&gone).expect("rm fixture dir");

        (tmp, repo, wts)
    }

    fn class_of(report: &WorktreeStandingReport, suffix: &str) -> (WorktreeClass, WorktreeReason) {
        let w = report
            .repos
            .iter()
            .flat_map(|r| r.worktrees.iter())
            .find(|w| w.path.ends_with(suffix))
            .unwrap_or_else(|| panic!("worktree {suffix} missing from report"));
        (w.verdict.class, w.verdict.reason.clone())
    }

    #[test]
    fn worktree_standing_classifies_real_worktrees() {
        let (_tmp, repo, _wts) = fixture();
        let report = worktree_standing(std::slice::from_ref(&repo), 4);
        assert_eq!(report.repos.len(), 1);
        let r = &report.repos[0];
        assert_eq!(r.default_branch.as_deref(), Some("main"));
        assert!(r.git_dir_bytes > 0);
        assert!(r.count_objects.is_some());
        assert_eq!(report.removal, "UNSUPPORTED");

        assert_eq!(class_of(&report, "/repo").0, WorktreeClass::Main);
        assert_eq!(class_of(&report, "/merged").0, WorktreeClass::MergedClean);
        assert_eq!(class_of(&report, "/ignored").0, WorktreeClass::MergedClean);
        assert_eq!(
            class_of(&report, "/unmerged"),
            (WorktreeClass::Unmerged, WorktreeReason::NotMerged { default_branch: "main".into() })
        );
        assert_eq!(
            class_of(&report, "/dirty"),
            (WorktreeClass::Dirty, WorktreeReason::TrackedChanges { count: 1 })
        );
        assert_eq!(
            class_of(&report, "/untracked"),
            (WorktreeClass::Dirty, WorktreeReason::UntrackedFiles { count: 1 })
        );
        assert_eq!(
            class_of(&report, "/stashed"),
            (WorktreeClass::Dirty, WorktreeReason::StashEntries { count: 1 })
        );
        assert_eq!(class_of(&report, "/detached").0, WorktreeClass::Detached);
        assert_eq!(class_of(&report, "/locked").0, WorktreeClass::Locked);
        assert_eq!(class_of(&report, "/gone").0, WorktreeClass::Prunable);

        let merged = r.worktrees.iter().find(|w| w.path.ends_with("/merged")).unwrap();
        assert!(merged.size_bytes > 0);
        assert!(merged.last_commit_unix.is_some());

        let s = report.summary;
        assert_eq!(s.linked_worktrees, 9);
        assert_eq!(s.reclaimable_worktrees, 3); // merged, ignored, gone
        assert_eq!(s.merged_clean, 2);
        assert_eq!(s.prunable, 1);
        assert_eq!(s.dirty, 3);
    }

    #[test]
    fn worktree_standing_is_read_only() {
        let (_tmp, repo, wts) = fixture();
        let list = |d: &Path| {
            Command::new("git")
                .arg("-C")
                .arg(d)
                .args(["worktree", "list", "--porcelain"])
                .output()
                .expect("git")
                .stdout
        };
        let before = list(&repo);
        let status_before = std::fs::read_to_string(wts.join("dirty/a.txt")).unwrap();
        let _ = worktree_standing(std::slice::from_ref(&repo), 4);
        assert_eq!(list(&repo), before, "worktree metadata must be unchanged");
        assert_eq!(std::fs::read_to_string(wts.join("dirty/a.txt")).unwrap(), status_before);
        assert!(!wts.join("gone").exists(), "prunable dir must not be recreated");
    }

    #[test]
    fn root_of_linked_worktrees_resolves_to_main_repo_once() {
        let (_tmp, repo, wts) = fixture();
        let found = discover_main_repos(&[wts.clone(), repo.clone()], 4);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].repo, repo);
    }

    #[test]
    fn no_default_branch_refuses_merged_classification() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("r");
        std::fs::create_dir_all(&repo).unwrap();
        g(&repo, &["init", "-q", "-b", "trunk"]);
        commit_file(&repo, "a.txt", "a", "init");
        let wt = repo.parent().unwrap().join("wt");
        g(&repo, &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()]);
        let report = worktree_standing(std::slice::from_ref(&repo), 2);
        assert_eq!(
            class_of(&report, "/wt"),
            (WorktreeClass::Unknown, WorktreeReason::DefaultBranchUnknown)
        );
        assert_eq!(report.summary.reclaimable_worktrees, 0);
    }
}
