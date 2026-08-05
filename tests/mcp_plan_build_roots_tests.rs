//! Regression test for `plan_build` root-scope inheritance.
//!
//! Bug: `plan_build`, when called without an explicit `roots` argument,
//! fell back to `default_scan_roots()` (broad defaults like the user's home
//! directory and `/tmp`) instead of inheriting the roots that the
//! referenced `audit_scan` actually used. This let a plan silently diverge
//! in scope from the audit it was built from: an audit scoped to a small
//! test directory could still produce a plan referencing unrelated real
//! files elsewhere on disk (cargo registry sources, other scratch
//! workspaces, etc.) with no explicit signal to the caller that the scope
//! had drifted.
//!
//! Fix: `WorkflowContext` now records the `roots` that `audit_scan` used
//! (`audit_roots`), and `plan_build` inherits them when the caller omits
//! `roots`, only falling back to `default_scan_roots()` if no audit ever
//! ran in this workspace at all.
//!
//! The fixture below is parameterized on whether an audit was scoped
//! narrowly or never run, so it can construct both the buggy state
//! (narrow audit + broad plan fallback) and the correct state on demand,
//! rather than only extending a single shared happy-path builder.

use std::path::PathBuf;

use osx_clnr::mcp::OsxClnrMcpServer;
use serde_json::json;
use tempfile::tempdir;

/// Which audit-scan state to set up before calling `plan_build`.
enum AuditState {
    /// `audit_scan` was run scoped to a narrow directory inside the
    /// workspace -- the exact repro scenario from the bug report.
    ScopedToSubdir,
}

/// Run `audit_scan` (if requested) then `plan_build` with no explicit
/// `roots`, returning the roots recorded in the resulting `cleanup-plan.json`.
fn plan_build_roots_after(state: AuditState) -> Vec<PathBuf> {
    let workspace = tempdir().unwrap().keep();

    // A narrow subdirectory that stands in for "the small test directory"
    // from the bug report -- deliberately NOT the workspace root, home
    // directory, or /tmp, so we can tell whether the plan's roots came from
    // this scope or from a global default.
    let scoped_root = workspace.join("scoped-scan-target");
    std::fs::create_dir_all(&scoped_root).unwrap();
    std::fs::write(scoped_root.join("junk.o"), b"fake build artifact").unwrap();

    let mut server = OsxClnrMcpServer::new(workspace.clone()).expect("server construction");

    match state {
        AuditState::ScopedToSubdir => {
            let audit_params = json!({
                "action": "scan",
                "workspace": workspace.to_string_lossy(),
                "roots": [scoped_root.to_string_lossy()],
            });
            server
                .call_tool("audit", Some(audit_params))
                .expect("audit_scan should succeed when scoped to the subdir");
        }
    }

    // plan_build called WITHOUT explicit roots -- this is the call that
    // must inherit the audit's scope rather than silently falling back to
    // default_scan_roots().
    let plan_params = json!({ "action": "build", "workspace": workspace.to_string_lossy() });
    let plan_result = server.call_tool("plan", Some(plan_params));

    match plan_result {
        Ok(_) => {
            let plan_file = workspace.join("cleanup-plan.json");
            let contents = std::fs::read_to_string(&plan_file)
                .expect("cleanup-plan.json should exist after a successful plan_build");
            let plan: osx_clnr::domain::plan::DeletionPlan =
                serde_json::from_str(&contents).expect("cleanup-plan.json should parse");
            plan.roots
        }
        Err(e) => {
            panic!("plan_build failed unexpectedly: {:?}", e);
        }
    }
}

#[test]
fn plan_build_inherits_scoped_audit_roots_when_roots_omitted() {
    let roots = plan_build_roots_after(AuditState::ScopedToSubdir);

    // The regression: the plan's roots must be exactly the narrow directory
    // the audit was scoped to, not a global default such as the user's home
    // directory or /tmp.
    assert_eq!(
        roots.len(),
        1,
        "expected plan to inherit exactly the one scoped audit root, got {:?}",
        roots
    );
    let root_str = roots[0].to_string_lossy();
    assert!(
        root_str.contains("scoped-scan-target"),
        "plan roots {:?} do not reflect the scoped audit directory -- scope diverged from the audit",
        roots
    );

    // Also assert the buggy broad defaults are NOT present.
    let home = std::env::var("HOME").unwrap_or_default();
    for root in &roots {
        let s = root.to_string_lossy();
        assert!(
            s != "/tmp" && (home.is_empty() || s != home),
            "plan roots {:?} contain an unscoped global default ({}), reproducing the bug",
            roots,
            s
        );
    }
}

#[test]
fn plan_build_without_any_audit_is_rejected_not_silently_broadened() {
    let workspace = tempdir().unwrap().keep();
    let mut server = OsxClnrMcpServer::new(workspace.clone()).expect("server construction");

    let plan_params = json!({ "action": "build", "workspace": workspace.to_string_lossy() });
    let result = server
        .call_tool("plan", Some(plan_params))
        .expect("call_tool itself should not fail at the transport level");

    // `call_tool` reports tool-level failures as an `isError: true` payload
    // rather than a Rust `Err`, so assert on that shape.
    assert_eq!(
        result.get("isError").and_then(|v| v.as_bool()),
        Some(true),
        "plan_build must refuse to run before any audit_scan has completed, got: {:?}",
        result
    );
    let text = result["content"][0]["text"].as_str().unwrap_or_default().to_lowercase();
    assert!(
        text.contains("audit"),
        "expected an audit-not-complete style error, got: {:?}",
        result
    );
}
