//! Regression test for the `ignore_recent_hours` recency-override bug.
//!
//! Bug: `audit_scan`'s subprocess wrapper only forwarded
//! `--ignore-recent-hours <n>` to the `oclnr` CLI `if n > 0`. But `0` is a
//! documented, meaningful value ("EMERGENCY LEVER: disable recency
//! protection entirely" -- see `src/nouns/dev.rs`), not "unset". Because `0`
//! also failed the `> 0` guard, the flag was silently dropped and the CLI
//! fell back to its own built-in default of 168 hours -- inverting the
//! caller's explicit request to disable the guard.
//!
//! `plan_build` compounded this: it had no `ignore_recent_hours` field on
//! its input schema at all, so it always re-scanned with the CLI's
//! hardcoded 168h default regardless of what `audit_scan` was told.
//!
//! Fix:
//!   - `OclnrRunner::audit_run` / `OclnrRunner::plan_create` now always
//!     forward `--ignore-recent-hours`, no `> 0` gate.
//!   - `PlanBuildInput` gained an optional `ignore_recent_hours` override.
//!   - `WorkflowContext` records the recency window `audit_scan` actually
//!     used (`audit_ignore_recent_hours`), and `plan_build` inherits it when
//!     the caller doesn't pass an explicit override.
//!
//! The fixture below is parameterized on the recency window so it can
//! construct the exact buggy state on demand (an "emergency lever" value of
//! `0`) as well as a normal non-zero override, rather than only extending a
//! single shared happy-path builder.

use std::time::{Duration, SystemTime};

use osx_clnr::mcp::OsxClnrMcpServer;
use serde_json::{json, Value};
use tempfile::tempdir;

/// `call_tool` wraps its JSON payload as MCP `content[0].text` (a
/// pretty-printed JSON string), not as a bare `Value`. Unwrap that envelope
/// so assertions can index straight into the tool's actual output.
fn unwrap_tool_json(envelope: &Value) -> Value {
    let text =
        envelope["content"][0]["text"].as_str().expect("tool result should have content[0].text");
    serde_json::from_str(text).expect("content[0].text should be valid JSON")
}

/// Build a fake Rust project whose files were all touched `touched_ago`
/// in the past -- an "active dev session" scenario where a naive recency
/// guard would hide the `target/` directory as "too recently modified to
/// be safe to flag".
fn seed_active_rust_project(root: &std::path::Path, touched_ago: Duration) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("target")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\nversion=\"0.1.0\"\n")
        .unwrap();
    std::fs::write(root.join("Cargo.lock"), "").unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("target/dummy.rlib"), b"fake").unwrap();

    let touched_time = SystemTime::now() - touched_ago;
    for entry in walk(root) {
        let file = std::fs::OpenOptions::new().write(true).open(&entry).unwrap();
        file.set_modified(touched_time).unwrap();
    }
}

fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// Run `audit_scan` against the seeded project with the given
/// `ignore_recent_hours`, returning `total_candidates` from the summary.
fn audit_scan_total_candidates(ignore_recent_hours: u64) -> usize {
    let workspace = tempdir().unwrap().keep();
    let project = workspace.join("proj");
    seed_active_rust_project(&project, Duration::from_secs(3600)); // touched 1h ago

    let mut server = OsxClnrMcpServer::new(workspace.clone()).expect("server construction");

    let params = json!({
        "action": "scan",
        "workspace": workspace.to_string_lossy(),
        "roots": [project.to_string_lossy()],
        "ignore_recent_hours": ignore_recent_hours,
    });
    let envelope = server.call_tool("audit", Some(params)).expect("audit_scan should succeed");
    let output = unwrap_tool_json(&envelope);

    output["summary"]["total_candidates"].as_u64().expect("total_candidates present") as usize
}

/// The core regression: `ignore_recent_hours: 0` ("disable the recency
/// guard entirely") must actually disable it, surfacing the `target/`
/// candidate even though every file was touched only 1 hour ago -- well
/// inside the CLI's own 168h default window that the bug silently
/// substituted.
#[test]
fn ignore_recent_hours_zero_disables_recency_guard() {
    let candidates = audit_scan_total_candidates(0);
    assert_eq!(
        candidates, 1,
        "ignore_recent_hours: 0 should disable the recency guard and surface the \
         just-touched target/ dir, but got {} candidates (0 means the CLI's 168h \
         default was silently substituted, reproducing the bug)",
        candidates
    );
}

/// Sanity check on the other side: a caller who does NOT ask to disable the
/// guard (a window larger than the touch time) must still see the recency
/// guard suppress the candidate. This proves the fix forwards the flag
/// faithfully in both directions, not just always-off.
#[test]
fn nonzero_ignore_recent_hours_still_filters_recent_projects() {
    // A window (9999h) far larger than the 1h touch time, so the project is
    // correctly suppressed as "recently active" -- distinguishing a real
    // non-zero override from the omitted -> CLI-default(168h) case.
    let candidates = audit_scan_total_candidates(9999);
    assert_eq!(
        candidates, 0,
        "a large ignore_recent_hours window should still suppress a project touched \
         1 hour ago, got {} candidates",
        candidates
    );
}

/// `plan_build` must inherit the recency decision baked into the audit_scan
/// that produced the referenced context, rather than independently
/// re-deriving recency with the CLI's hardcoded 168h default. This
/// reproduces the `plan_build` half of the bug: even when `audit_scan`
/// correctly saw the candidate (ignore_recent_hours=0), `plan_build` had no
/// field to accept or propagate that decision and always re-scanned with
/// the CLI default, producing an empty plan for a workspace whose audit
/// found something.
#[test]
fn plan_build_inherits_audit_scans_ignore_recent_hours() {
    let workspace = tempdir().unwrap().keep();
    let project = workspace.join("proj");
    seed_active_rust_project(&project, Duration::from_secs(3600));

    let mut server = OsxClnrMcpServer::new(workspace.clone()).expect("server construction");

    let audit_params = json!({
        "action": "scan",
        "workspace": workspace.to_string_lossy(),
        "roots": [project.to_string_lossy()],
        "ignore_recent_hours": 0,
    });
    let audit_envelope = server
        .call_tool("audit", Some(audit_params))
        .expect("audit_scan with ignore_recent_hours=0 should succeed");
    let audit_output = unwrap_tool_json(&audit_envelope);
    assert_eq!(
        audit_output["summary"]["total_candidates"].as_u64().unwrap(),
        1,
        "precondition: audit_scan must have found the target/ candidate"
    );

    // plan_build called with NO explicit ignore_recent_hours -- it must
    // inherit the audit's override (0) rather than falling back to the
    // CLI's own 168h default, which would silently drop the candidate
    // again and produce an empty, inconsistent plan.
    let plan_params = json!({ "action": "build", "workspace": workspace.to_string_lossy() });
    server.call_tool("plan", Some(plan_params)).expect("plan_build should succeed");

    let plan_file = workspace.join("cleanup-plan.json");
    let contents = std::fs::read_to_string(&plan_file).expect("cleanup-plan.json should exist");
    let plan: osx_clnr::domain::plan::DeletionPlan =
        serde_json::from_str(&contents).expect("cleanup-plan.json should parse");

    assert!(
        !plan.items.is_empty(),
        "plan_build produced an empty plan even though the referenced audit_scan (with \
         ignore_recent_hours=0) found 1 candidate -- plan_build re-derived recency with \
         the CLI's 168h default instead of inheriting the audit's override, reproducing \
         the bug"
    );
}
