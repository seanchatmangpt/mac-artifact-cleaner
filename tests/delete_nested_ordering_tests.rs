//! Regression test for the parallel-deletion nested-path race.
//!
//! Bug: `delete execute`'s Rayon `par_iter()` deleted every plan item
//! concurrently with no dedup or parent/child ordering. When a plan
//! contained both a parent directory and a path nested inside it, deleting
//! the parent first (`delete_dir_all` removes the whole subtree) could make
//! the nested item's own delete call hit a path that had already vanished --
//! a TOCTOU race between the `!item.path.exists()` check and the actual
//! delete -- and the nested item was reported `Failed` even though nothing
//! was actually wrong.
//!
//! This test builds a *real* nested directory tree on disk, puts both the
//! parent and a child inside it into one plan, approves it for real with
//! `sign_approval`/`OCLNR_APPROVAL_SECRET`, executes it through the actual
//! `oclnr` CLI binary (`delete execute --yes`), and asserts on the real
//! resulting receipt and real filesystem state -- no mocking of the
//! collaborators, per this repo's Chicago-style testing discipline.

use std::{fs, path::PathBuf, process::Command};

use osx_clnr::domain::{
    dcm::Reversibility,
    plan::{DeletionPlan, PlanItem, PlanItemKind},
};

const TEST_SECRET: &[u8] = b"delete-nested-ordering-test-secret";

fn oclnr_bin() -> PathBuf {
    // Standard cargo integration-test layout: the test binary sits in
    // target/<profile>/deps, and the `oclnr` binary is one level up.
    let mut path = std::env::current_exe().expect("current test exe path");
    path.pop(); // deps/
    path.pop(); // <profile>/
    path.push("oclnr");
    assert!(path.exists(), "expected built oclnr binary at {}", path.display());
    path
}

/// Builds a plan whose items include a directory *and* a path nested inside
/// that same directory -- the exact overlapping-item shape that used to race
/// under concurrent deletion -- signs it, and writes it to `plan_path`.
fn write_nested_plan(root: &std::path::Path, plan_path: &std::path::Path) {
    let parent = root.join("parent-dir");
    let child = parent.join("nested-child");
    fs::create_dir_all(&child).expect("create nested dirs");
    fs::write(child.join("file.txt"), b"payload").expect("write nested file");

    let items = vec![
        PlanItem {
            path: parent.clone(),
            kind: PlanItemKind::Dir,
            reason: "parent".to_string(),
            bytes: 0,
            reversibility: Reversibility::Reversible,
        },
        PlanItem {
            path: child.clone(),
            kind: PlanItemKind::Dir,
            reason: "nested child, also independently planned".to_string(),
            bytes: 0,
            reversibility: Reversibility::Reversible,
        },
    ];

    let mut plan = DeletionPlan::new(vec![root.to_path_buf()], false, false, items, vec![]);
    plan.approval = Some(plan.sign_approval(TEST_SECRET, "test", "nested ordering regression"));

    fs::write(plan_path, serde_json::to_string_pretty(&plan).unwrap()).expect("write plan file");
}

/// Positive + refusal-of-false-failure: executing a plan with a nested
/// parent/child pair deletes the real tree from disk and never reports
/// `failed` for the nested item (or anything else) -- the receipt shows the
/// parent `deleted` and the child `skippedmissing` (already gone once its
/// ancestor was removed), which is the truthful outcome, not a spurious
/// `Failed`.
#[test]
fn nested_plan_items_delete_without_spurious_failure() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan_path = tmp.path().join("cleanup-plan.json");
    let receipt_path = tmp.path().join("deletion-receipt.jsonocel");

    write_nested_plan(tmp.path(), &plan_path);

    let output = Command::new(oclnr_bin())
        .args([
            "delete",
            "execute",
            "--plan",
            plan_path.to_str().unwrap(),
            "--receipt",
            receipt_path.to_str().unwrap(),
            "--yes",
        ])
        .env("OCLNR_APPROVAL_SECRET", std::str::from_utf8(TEST_SECRET).unwrap())
        .output()
        .expect("run oclnr delete execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "delete execute failed:\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );

    // Real filesystem state: the whole tree is actually gone.
    assert!(!tmp.path().join("parent-dir").exists(), "parent dir should be deleted");

    // Real receipt content: no item is reported Failed.
    let receipt_json = fs::read_to_string(&receipt_path).expect("read receipt");
    let receipt: serde_json::Value =
        serde_json::from_str(&receipt_json).expect("parse receipt json");
    let results = receipt["execution_record"]["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2, "receipt should still carry an entry per planned item");

    let statuses: Vec<&str> =
        results.iter().map(|r| r["status"].as_str().expect("status string")).collect();
    assert!(
        !statuses.contains(&"failed"),
        "no plan item should be reported Failed for a nested-path race: {:?}",
        statuses
    );
    // Exactly one item is the real `deleted` (whichever the partition kept
    // top-level -- the parent, per `partition_nested_items`), and the other
    // is truthfully accounted for as already gone.
    assert!(statuses.contains(&"deleted"), "expected the parent dir to be reported deleted");
    assert!(
        statuses.contains(&"skippedmissing"),
        "expected the nested child to be reported skippedmissing, not failed"
    );
}
