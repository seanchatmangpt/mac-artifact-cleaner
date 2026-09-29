//! `delete execute --thin-after`: opt-in snapshot thinning before free space is
//! sampled. Real `oclnr` binary, real plan on disk, real receipt and
//! filesystem state -- no mocks.
//!
//! The one case that would really run `tmutil thinlocalsnapshots` against the
//! machine's APFS snapshots (flag on, `--yes`) is `#[ignore]`d by name: thinning
//! is a destructive side effect on host state that is infeasible to isolate
//! in-process. Run it deliberately with `cargo test -- --ignored`.

use std::{fs, path::PathBuf, process::Command};

use osx_clnr::domain::{
    dcm::Reversibility,
    plan::{DeletionPlan, PlanItem, PlanItemKind},
};

const TEST_SECRET: &[u8] = b"delete-thin-after-test-secret";

fn oclnr_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("current test exe path");
    path.pop();
    path.pop();
    path.push("oclnr");
    assert!(path.exists(), "expected built oclnr binary at {}", path.display());
    path
}

fn write_plan(root: &std::path::Path, plan_path: &std::path::Path) -> PathBuf {
    let victim = root.join("victim-dir");
    fs::create_dir_all(&victim).unwrap();
    fs::write(victim.join("f.txt"), b"payload").unwrap();
    let items = vec![PlanItem {
        path: victim.clone(),
        kind: PlanItemKind::Dir,
        reason: "thin-after fixture".to_string(),
        bytes: 0,
        reversibility: Reversibility::Reversible,
    }];
    let mut plan = DeletionPlan::new(vec![root.to_path_buf()], false, false, items, vec![]);
    plan.approval = Some(plan.sign_approval(TEST_SECRET, "test", "thin-after fixture"));
    fs::write(plan_path, serde_json::to_string_pretty(&plan).unwrap()).unwrap();
    victim
}

fn run(args: &[&str], plan: &std::path::Path, receipt: &std::path::Path) -> std::process::Output {
    Command::new(oclnr_bin())
        .args(["delete", "execute", "--plan"])
        .arg(plan)
        .arg("--receipt")
        .arg(receipt)
        .args(args)
        .env("OCLNR_APPROVAL_SECRET", std::str::from_utf8(TEST_SECRET).unwrap())
        .output()
        .expect("run oclnr delete execute")
}

/// Default (flag off) deletes for real and writes no thin receipt.
#[test]
fn flag_off_deletes_and_writes_no_thin_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let plan = tmp.path().join("plan.json");
    let receipt = tmp.path().join("receipt.jsonocel");
    let victim = write_plan(tmp.path(), &plan);

    let out = run(&["--yes"], &plan, &receipt);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    assert!(!victim.exists(), "victim should be deleted");
    assert!(receipt.exists(), "receipt should be written");
    assert!(!receipt.with_extension("thin.json").exists(), "no thin receipt without --thin-after");
}

/// Dry run (no `--yes`) with `--thin-after` deletes nothing and thins nothing.
#[test]
fn dry_run_with_thin_after_deletes_and_thins_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let plan = tmp.path().join("plan.json");
    let receipt = tmp.path().join("receipt.jsonocel");
    let victim = write_plan(tmp.path(), &plan);

    let out = run(&["--thin-after"], &plan, &receipt);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    assert!(victim.exists(), "dry run must not delete");
    assert!(!receipt.with_extension("thin.json").exists(), "dry run must not thin");
}

/// Flag on with `--yes` really thins local snapshots and writes a sealed thin
/// receipt beside the deletion receipt. Ignored by default: real `tmutil`
/// side effect (see module docs).
#[test]
#[ignore = "runs real tmutil thinlocalsnapshots on the host; run deliberately with --ignored"]
fn flag_on_with_yes_writes_sealed_thin_receipt() {
    let tmp = tempfile::tempdir().unwrap();
    let plan = tmp.path().join("plan.json");
    let receipt = tmp.path().join("receipt.jsonocel");
    let victim = write_plan(tmp.path(), &plan);

    let out = run(&["--yes", "--thin-after"], &plan, &receipt);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    assert!(!victim.exists());
    let thin = receipt.with_extension("thin.json");
    assert!(thin.exists(), "thin receipt should be written");
    assert!(thin.with_extension("affidavit.json").exists(), "thin receipt should be sealed");
}
