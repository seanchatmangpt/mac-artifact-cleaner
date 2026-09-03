//! CLI-level regression tests for `oclnr plan approve`.
//!
//! Before this command existed, `plan_approve` (the HMAC-signing step
//! `delete execute` gates on — see `tests/plan_approval_tests.rs`) only
//! lived in the MCP server layer, with no CLI equivalent. That made it
//! impossible for an unattended, non-MCP caller (a launchd job, a plain
//! shell script) to complete the full audit→plan→approve→delete pipeline —
//! it could build a plan but never legitimately sign one. This exercises
//! the new `oclnr plan approve` subcommand end-to-end through the real
//! binary (real files on disk, real subprocess, no mocks), confirming it
//! produces a plan `delete execute` actually accepts, and refuses the same
//! unsafe paths the MCP tool refuses.

use std::{fs, path::PathBuf, process::Command};

use osx_clnr::domain::{delete::require_plan_approved, plan::DeletionPlan};

const TEST_SECRET: &str = "cli-approve-test-secret-does-not-leave-this-process";

fn oclnr_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oclnr"));
    cmd.env("OCLNR_APPROVAL_SECRET", TEST_SECRET);
    cmd
}

/// Writes a minimal, valid, unapproved plan JSON directly (bypassing `plan
/// build`'s real filesystem scan, which this test doesn't need) so each test
/// can start from a known, controlled plan state.
fn write_unapproved_plan(path: &PathBuf, reversibility: &str) {
    let plan = format!(
        r#"{{
  "version": 1,
  "created_unix": 1700000000,
  "roots": ["/tmp/plan-approve-cli-test"],
  "deps": false,
  "aggressive": false,
  "tool_roots": [],
  "items": [
    {{
      "path": "/tmp/plan-approve-cli-test/target",
      "kind": "dir",
      "reason": "rust target",
      "bytes": 1024,
      "reversibility": "{reversibility}"
    }}
  ],
  "exclusions": [],
  "approval": null
}}"#
    );
    fs::write(path, plan).expect("write test plan");
}

#[test]
fn approve_without_yes_is_refused_and_plan_is_left_unsigned() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan_path = tmp.path().join("plan.json");
    write_unapproved_plan(&plan_path, "reversible");

    let output = oclnr_cmd()
        .args(["plan", "approve", "--plan", plan_path.to_str().unwrap(), "--reason", "test"])
        .output()
        .expect("run oclnr plan approve");

    assert!(!output.status.success(), "approve without --yes must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--yes"), "refusal must name the missing --yes flag: {stderr}");

    let content = fs::read_to_string(&plan_path).unwrap();
    let plan: DeletionPlan = serde_json::from_str(&content).unwrap();
    assert!(plan.approval.is_none(), "a refused approval must leave the plan file unsigned");
}

#[test]
fn approve_with_unknown_reversibility_requires_explicit_acknowledgement() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan_path = tmp.path().join("plan.json");
    write_unapproved_plan(&plan_path, "unknown");

    let refused = oclnr_cmd()
        .args([
            "plan",
            "approve",
            "--plan",
            plan_path.to_str().unwrap(),
            "--reason",
            "test",
            "--yes",
        ])
        .output()
        .expect("run oclnr plan approve");
    assert!(
        !refused.status.success(),
        "approving an Unknown-reversibility plan without acknowledgement must fail"
    );

    let acknowledged = oclnr_cmd()
        .args([
            "plan",
            "approve",
            "--plan",
            plan_path.to_str().unwrap(),
            "--reason",
            "test",
            "--yes",
            "--acknowledge-unknown-reversibility",
        ])
        .output()
        .expect("run oclnr plan approve");
    assert!(
        acknowledged.status.success(),
        "approving with explicit acknowledgement must succeed: {}",
        String::from_utf8_lossy(&acknowledged.stderr)
    );
}

/// End-to-end: a plan signed by the real `oclnr plan approve` CLI must be
/// accepted by the same `require_plan_approved` gate `delete execute` uses,
/// when verified with the same secret — and rejected when verified with a
/// different one (the CLI path must be bound to the real, keyed secret, not
/// some hardcoded literal).
#[test]
fn plan_signed_by_cli_is_accepted_by_the_real_approval_gate() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plan_path = tmp.path().join("plan.json");
    write_unapproved_plan(&plan_path, "reversible");

    let approve = oclnr_cmd()
        .args([
            "plan",
            "approve",
            "--plan",
            plan_path.to_str().unwrap(),
            "--approver",
            "autoclean-test",
            "--reason",
            "scheduled test cleanup",
            "--yes",
        ])
        .output()
        .expect("run oclnr plan approve");
    assert!(
        approve.status.success(),
        "legitimate approve must succeed: {}",
        String::from_utf8_lossy(&approve.stderr)
    );

    let content = fs::read_to_string(&plan_path).unwrap();
    let signed: DeletionPlan = serde_json::from_str(&content).unwrap();
    assert!(signed.approval.is_some(), "plan file must carry an approval block after approve");

    assert!(
        require_plan_approved(&signed, TEST_SECRET.as_bytes()).is_ok(),
        "a plan signed by the CLI with the real secret must pass the same gate delete execute uses"
    );
    assert!(
        require_plan_approved(&signed, b"a-different-wrong-secret").is_err(),
        "a plan signed with one secret must be refused when checked against a different one"
    );
}
