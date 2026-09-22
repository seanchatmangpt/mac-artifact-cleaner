//! End-to-end: `oclnr tools git-worktrees` against a real temp git repo
//! built with the real `git` binary. No doubles.

use std::{path::Path, process::Command};

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
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn cli_reports_merged_and_unmerged_worktrees_and_writes_json() {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    g(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    g(&repo, &["add", "a.txt"]);
    g(&repo, &["commit", "-q", "-m", "init"]);

    let merged = base.join("wt-merged");
    g(&repo, &["worktree", "add", "-q", "-b", "done", merged.to_str().unwrap()]);
    let open = base.join("wt-open");
    g(&repo, &["worktree", "add", "-q", "-b", "open", open.to_str().unwrap()]);
    std::fs::write(open.join("b.txt"), "b").unwrap();
    g(&open, &["add", "b.txt"]);
    g(&open, &["commit", "-q", "-m", "wip"]);

    let out_json = base.join("report.json");
    let output = Command::new(env!("CARGO_BIN_EXE_oclnr"))
        .args(["tools", "git-worktrees", "--root"])
        .arg(&repo)
        .arg("--output")
        .arg(&out_json)
        .output()
        .expect("run oclnr");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stdout={stdout} stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("reclaimable: 1 worktrees"), "{stdout}");
    assert!(stdout.contains("MERGED_CLEAN"), "{stdout}");

    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out_json).unwrap()).unwrap();
    assert_eq!(v["removal"], "UNSUPPORTED");
    assert_eq!(v["summary"]["linked_worktrees"], 2);
    assert_eq!(v["summary"]["reclaimable_worktrees"], 1);
    assert_eq!(v["summary"]["unmerged"], 1);
    // Read-only: both worktree directories still exist.
    assert!(merged.is_dir() && open.is_dir());
}
