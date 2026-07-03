//! Contract-level regression tests for the MCP workflow state machine.
//!
//! These tests exercise `osx_clnr::mcp::WorkflowContext` through its public
//! API (the same API `oclnr-mcp`'s `audit_scan`/`plan_build` handlers call
//! internally), using a state-parameterized fixture that can construct any
//! of the 18 workflow states on demand -- including the DELETE_FAILED
//! dead-end state that was previously unreachable from re-scan.

use std::path::PathBuf;

use osx_clnr::mcp::{WorkflowContext, WorkflowState};

/// Fixture: build a `WorkflowContext` already parked in an arbitrary state.
/// Parameterized so callers can construct the exact buggy state on demand
/// (e.g. `ctx_in_state(WorkflowState::DeleteFailed)`) rather than only
/// exercising a single shared happy-path builder.
fn ctx_in_state(state: WorkflowState) -> WorkflowContext {
    let mut ctx = WorkflowContext::new(PathBuf::from("/tmp/mcp-contract-test"));
    ctx.state = state;
    ctx
}

/// Regression test for the DELETE_FAILED dead-end bug:
///
/// Repro (pre-fix):
///   1. Workflow reaches DELETE_FAILED (e.g. delete_execute fails).
///   2. `audit_scan` calls `ctx.transition(WorkflowState::AuditNeeded)` as
///      its very first step, regardless of current state.
///   3. That transition was rejected with "Cannot transition from
///      DELETE_FAILED to AUDIT_NEEDED", so a fresh scan was impossible.
///   4. `plan_build` also refused ("Cannot build plan without completed
///      audit") because the context could never reach AUDIT_COMPLETE again.
///   5. Only `clear_artifacts` (which archives the audit file and resets the
///      whole context to UNSTARTED) could unstick the workflow -- an
///      undocumented mandatory recovery step for an otherwise ordinary
///      "try again" path.
///
/// This test asserts DELETE_FAILED can transition directly to AUDIT_NEEDED
/// (mirroring what `audit_scan` does internally), so a fresh scan is
/// possible without going through `clear_artifacts` first.
#[test]
fn delete_failed_allows_direct_rescan() {
    let mut ctx = ctx_in_state(WorkflowState::DeleteFailed);

    let result = ctx.transition(WorkflowState::AuditNeeded);
    assert!(
        result.is_ok(),
        "expected DELETE_FAILED -> AUDIT_NEEDED to succeed (re-scan after a failed delete), got {:?}",
        result
    );
    assert_eq!(ctx.state, WorkflowState::AuditNeeded);

    // From there the normal audit_scan sequence should proceed exactly as
    // it would from a fresh UNSTARTED context.
    assert!(ctx.transition(WorkflowState::AuditInProgress).is_ok());
    assert!(ctx.transition(WorkflowState::AuditComplete).is_ok());
}

/// The other terminal "in progress" / success states must NOT gain this
/// same shortcut as a side effect of the fix -- only DELETE_FAILED (and the
/// pre-existing UNSTARTED / CLEANUP_COMPLETE arms) may jump to AUDIT_NEEDED.
/// This is what would catch an overly broad fix (e.g. a `(_, AuditNeeded)`
/// catch-all) that accidentally lets deletion mid-flight be abandoned.
#[test]
fn only_documented_states_can_jump_to_audit_needed() {
    let allowed =
        [WorkflowState::Unstarted, WorkflowState::CleanupComplete, WorkflowState::DeleteFailed];

    let all_states = [
        WorkflowState::Unstarted,
        WorkflowState::AuditNeeded,
        WorkflowState::AuditInProgress,
        WorkflowState::AuditComplete,
        WorkflowState::AuditFailed,
        WorkflowState::PlanNeeded,
        WorkflowState::PlanInProgress,
        WorkflowState::PlanReady,
        WorkflowState::PlanValidationFailed,
        WorkflowState::PlanApproved,
        WorkflowState::DeleteNeeded,
        WorkflowState::DeleteInProgress,
        WorkflowState::DeleteComplete,
        WorkflowState::DeleteFailed,
        WorkflowState::ReceiptReady,
        WorkflowState::VerificationInProgress,
        WorkflowState::CleanupComplete,
        WorkflowState::CleanupFailed,
    ];

    for state in all_states {
        let ctx = ctx_in_state(state);
        let can_jump = ctx.can_transition_to(WorkflowState::AuditNeeded).is_ok();
        let expected = allowed.contains(&state);
        assert_eq!(
            can_jump,
            expected,
            "{} -> AUDIT_NEEDED: expected {}, got {}",
            state.as_str(),
            expected,
            can_jump
        );
    }
}

/// Sanity check that DELETE_FAILED is still reachable from any state (the
/// error catch-all), so the fix didn't accidentally remove the ability to
/// *enter* DELETE_FAILED while making it easier to *leave*.
#[test]
fn delete_failed_still_reachable_from_any_state() {
    let ctx = ctx_in_state(WorkflowState::PlanApproved);
    assert!(ctx.can_transition_to(WorkflowState::DeleteFailed).is_ok());
}

// ============================================================================
// audit_scan: no implicit full-home-directory scan
// ============================================================================
//
// Regression test for a bug where calling `audit_scan` with `{}` (no
// `roots`, no `workspace`) silently fell back to
// `crate::nouns::default_scan_roots()`, which resolves to the caller's
// *entire home directory* plus `/tmp`. A tool documented as scanning a
// "developer environment" / project directory instead performed an
// unbounded, unconfirmed real filesystem walk of the user's home dir and
// wrote `disk-audit.jsonocel` there as a side effect -- with no explicit
// path argument and no confirmation gate.
//
// The fixture below is state-parameterized over the JSON `arguments`
// payload passed to `tools/call`, so it can construct both the buggy
// input (`{}` / empty `roots`) and the fixed-shape input (`roots`
// explicitly supplied) on demand, rather than only extending a single
// shared happy-path builder.

use osx_clnr::mcp::OsxClnrMcpServer;
use serde_json::{json, Value};

/// Fixture: a fresh MCP server rooted at a scratch workspace, plus the
/// exact `arguments` payload a caller would send to `audit_scan`.
/// Parameterized on `args` so tests can drive both the empty-object repro
/// and a properly-scoped call through the same harness.
///
/// `call_tool` reports tool-level failures as `Ok` with an
/// `{"isError": true, "content": [...]}` envelope (matching MCP's
/// `tools/call` wire contract) rather than as `Result::Err`, so this
/// unwraps that envelope into a plain `Result<Value, String>` for the
/// tests below.
fn call_audit_scan(workspace: &std::path::Path, args: Value) -> Result<Value, String> {
    let mut server = OsxClnrMcpServer::new(workspace.to_path_buf())
        .map_err(|e| format!("server init failed: {}", e.message))?;
    let outer = server.call_tool("audit_scan", Some(args)).map_err(|e| e.message)?;
    let is_error = outer.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
    if is_error {
        let text = outer
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("<no error text>")
            .to_string();
        Err(text)
    } else {
        Ok(outer)
    }
}

#[test]
fn audit_scan_rejects_empty_arguments_instead_of_scanning_home_dir() {
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-audit-scan-empty");
    std::fs::create_dir_all(&workspace).unwrap();

    // The exact repro from the bug report: call `audit_scan` with `{}`.
    let result = call_audit_scan(&workspace, json!({}));

    let err = result.expect_err(
        "audit_scan({}) must be rejected, not silently default to a full home-directory scan",
    );
    assert!(
        err.to_lowercase().contains("roots"),
        "expected error to mention the missing `roots` argument, got: {}",
        err
    );

    // No audit file should have been written as a side effect of the
    // rejected call.
    assert!(
        !workspace.join("disk-audit.jsonocel").exists(),
        "audit_scan({{}}) must not write disk-audit.jsonocel as a side effect of a rejected call"
    );
}

#[test]
fn audit_scan_rejects_explicit_empty_roots_array() {
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-audit-scan-empty-roots");
    std::fs::create_dir_all(&workspace).unwrap();

    // Same bug, reached via an explicit empty array rather than an
    // altogether-missing field.
    let result = call_audit_scan(&workspace, json!({ "roots": [] }));

    let err = result.expect_err("audit_scan with roots: [] must be rejected");
    assert!(
        err.to_lowercase().contains("roots"),
        "expected error to mention the missing `roots` argument, got: {}",
        err
    );
}

#[test]
fn audit_scan_rejects_single_blank_string_root() {
    // Regression test: `roots: [""]` is a *non-empty* array, so the naive
    // `input.roots.is_empty()` check sails right past it and the blank
    // entry reaches the real subprocess, reproducing the exact
    // full-home-directory scan the guard exists to prevent.
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-audit-scan-blank-root");
    std::fs::create_dir_all(&workspace).unwrap();

    let result = call_audit_scan(&workspace, json!({ "roots": [""] }));

    let err = result.expect_err("audit_scan with roots: [\"\"] must be rejected");
    assert!(
        err.to_lowercase().contains("roots"),
        "expected error to mention the blank `roots` entry, got: {}",
        err
    );
    assert!(
        !workspace.join("disk-audit.jsonocel").exists(),
        "audit_scan with a blank root must not write disk-audit.jsonocel as a side effect"
    );
}

#[test]
fn audit_scan_rejects_whitespace_only_root() {
    // Same bug, reached via a root that is non-empty as a string but
    // resolves to nothing meaningful once trimmed.
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-audit-scan-whitespace-root");
    std::fs::create_dir_all(&workspace).unwrap();

    let result = call_audit_scan(&workspace, json!({ "roots": ["   "] }));

    let err = result.expect_err("audit_scan with roots: [\"   \"] must be rejected");
    assert!(
        err.to_lowercase().contains("roots"),
        "expected error to mention the blank `roots` entry, got: {}",
        err
    );
}

#[test]
fn audit_scan_tool_roots_true_satisfies_the_guard_with_empty_roots() {
    // The guard's own error message advertises `tool_roots: true` as an
    // alternative to supplying explicit `roots`. Make sure that promise is
    // actually true: `tool_roots: true` with an empty `roots` array must
    // not be rejected by this guard (it scans known developer tool roots
    // instead of the home directory, so it's already scoped).
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-audit-scan-tool-roots");
    std::fs::create_dir_all(&workspace).unwrap();

    let result = call_audit_scan(&workspace, json!({ "tool_roots": true }));

    if let Err(err) = result {
        assert!(
            !err.to_lowercase().contains("roots is required"),
            "tool_roots: true should satisfy the guard on its own, got: {}",
            err
        );
    }
}

#[test]
fn audit_scan_with_explicit_roots_does_not_hit_the_guard() {
    // Sanity check that the fixture can also construct the *non-buggy*
    // shape: an explicit, scoped `roots` list must pass the new guard and
    // proceed to actually invoke the scanner (which will fail here only
    // because the `oclnr` CLI binary isn't necessarily on PATH in the test
    // environment / the root is trivial) -- the point is that it must NOT
    // fail with the "roots is required" guard error used above.
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-audit-scan-scoped");
    std::fs::create_dir_all(&workspace).unwrap();
    let scoped_root = workspace.join("project");
    std::fs::create_dir_all(&scoped_root).unwrap();

    let result =
        call_audit_scan(&workspace, json!({ "roots": [scoped_root.display().to_string()] }));

    if let Err(err) = result {
        assert!(
            !err.to_lowercase().contains("roots is required"),
            "explicit roots should not trip the empty-roots guard, got: {}",
            err
        );
    }
}

// ============================================================================
// clear_artifacts: must not fabricate success for an unscanned/unwritable
// workspace
// ============================================================================
//
// Regression test for a bug where `clear_artifacts` reported
// `{"success": true, "archive_location": "<workspace>/archive/<ts>", ...}`
// even though:
//   - the workspace had never been audited (no evidence to archive), and/or
//   - the archive directory was never actually created on disk (the
//     `std::fs::create_dir_all(&archive_dir).ok()` call silently swallowed
//     any I/O error, e.g. permission denied for a path like `/etc`).
//
// An unattended caller (e.g. an LLM driving the MCP tools) would believe
// artifacts were archived when nothing happened and the workspace argument
// was never validated at all.
//
// The fixture is state-parameterized on whether the workspace has real
// audit evidence (`seed_audit: bool`), so it can construct both the buggy
// state (no evidence, unwritable-in-spirit call) and the legitimate
// happy-path state (real evidence present) through the same harness --
// rather than only extending a single shared happy-path builder.

/// Fixture: a fresh MCP server rooted at `workspace`. When `seed_audit` is
/// true, first drives a real `audit_scan` over a trivial scoped root so the
/// workflow context ends up holding a genuine `audit_file`, mirroring a
/// workspace that has actually been scanned. Returns the server so the
/// caller can immediately issue `clear_artifacts` against the same
/// in-memory workflow state.
fn server_with_optional_audit(workspace: &std::path::Path, seed_audit: bool) -> OsxClnrMcpServer {
    std::fs::create_dir_all(workspace).unwrap();
    let mut server = OsxClnrMcpServer::new(workspace.to_path_buf())
        .expect("server init failed -- is `oclnr` on PATH?");

    if seed_audit {
        let scoped_root = workspace.join("project");
        std::fs::create_dir_all(&scoped_root).unwrap();
        let result = server
            .call_tool("audit_scan", Some(json!({ "roots": [scoped_root.display().to_string()] })))
            .expect("call_tool itself should not fail at the transport level");
        assert_eq!(
            result["isError"],
            json!(false),
            "fixture setup: audit_scan must succeed to seed real evidence, got: {}",
            result
        );
    }

    server
}

#[test]
fn clear_artifacts_on_unscanned_workspace_errors_and_creates_nothing() {
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-clear-artifacts-unscanned");
    let _ = std::fs::remove_dir_all(&workspace);
    let mut server = server_with_optional_audit(&workspace, /* seed_audit */ false);

    // Exact repro shape from the bug report (workspace substituted for a
    // scratch dir instead of /etc, since the assertion under test --  no
    // archive directory materializes and no fabricated success is
    // reported -- does not depend on the target being unwritable).
    let result = server
        .call_tool("clear_artifacts", Some(json!({ "confirm": true, "dry_run": false })))
        .expect("call_tool itself should not fail at the transport level");

    // `call_tool` always returns `Ok`, embedding tool-level failures as
    // `isError: true` in the JSON-RPC content payload -- so the actual
    // assertion is on that flag, not on the outer `Result`.
    assert_eq!(
        result["isError"],
        json!(true),
        "clear_artifacts on a workspace with no audit evidence must report isError: true, \
         not fabricate a success + archive_location: {}",
        result
    );
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.to_lowercase().contains("no audit")
            || text.to_lowercase().contains("nothing to archive"),
        "expected error to explain there is nothing to archive, got: {}",
        text
    );

    // The archive directory must never have been created as a side effect
    // of the rejected call.
    let archive_root = workspace.join("archive");
    assert!(
        !archive_root.exists(),
        "clear_artifacts must not create {} when there is nothing to archive",
        archive_root.display()
    );
}

#[test]
fn clear_artifacts_with_real_audit_evidence_actually_archives_it() {
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-clear-artifacts-seeded");
    let _ = std::fs::remove_dir_all(&workspace);
    let mut server = server_with_optional_audit(&workspace, /* seed_audit */ true);

    let result = server
        .call_tool("clear_artifacts", Some(json!({ "confirm": true, "dry_run": false })))
        .expect("clear_artifacts must succeed when real audit evidence exists");

    let archive_location = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .expect("tool_result must carry a text payload");
    let parsed: Value = serde_json::from_str(archive_location).expect("payload must be JSON");

    assert_eq!(parsed["success"], json!(true));
    let location = parsed["archive_location"].as_str().expect("archive_location must be a string");
    assert!(
        std::path::Path::new(location).is_dir(),
        "reported archive_location {} must actually exist on disk when success=true",
        location
    );
    let archived = parsed["archived_files"].as_array().expect("archived_files must be an array");
    assert!(
        !archived.is_empty(),
        "when real audit evidence exists, archived_files must not be empty"
    );
    for entry in archived {
        let dest = entry["destination"].as_str().expect("destination must be a string");
        assert!(
            std::path::Path::new(dest).is_file(),
            "archived file destination {} must actually exist on disk",
            dest
        );
    }
}

// ============================================================================
// clear_artifacts: dry_run:true (the tool's own advertised default) must be
// a real no-op preview -- no filesystem writes, no workflow state reset.
// ============================================================================
//
// Regression test for a bug where `ClearArtifactsInput.dry_run` was
// deserialized but never consulted by the handler: only `confirm` gated
// execution, so `dry_run: true` (or even the field's own default, when
// omitted) still performed real `std::fs::create_dir_all` + `std::fs::copy`
// writes and reset in-memory workflow state to UNSTARTED -- identical to
// `dry_run: false`. This directly contradicted the documented "dry run
// default" invariant that destructive ops require explicit confirmation and
// that preview phases must not mutate anything.

#[test]
fn clear_artifacts_dry_run_true_previews_without_writing_or_resetting_state() {
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-clear-artifacts-dry-run");
    let _ = std::fs::remove_dir_all(&workspace);
    let mut server = server_with_optional_audit(&workspace, /* seed_audit */ true);

    // Sanity: the fixture actually produced real audit evidence.
    let audit_file = workspace.join("disk-audit.json");
    assert!(
        audit_file.exists() || workspace.read_dir().unwrap().count() > 0,
        "fixture setup: expected some audit evidence to exist under {}",
        workspace.display()
    );

    // dry_run: true, even with confirm: true -- dry_run must win.
    let result = server
        .call_tool("clear_artifacts", Some(json!({ "confirm": true, "dry_run": true })))
        .expect("call_tool itself should not fail at the transport level");

    assert_eq!(
        result["isError"],
        json!(false),
        "clear_artifacts dry_run:true with real audit evidence should succeed as a preview: {}",
        result
    );

    let text = result["content"][0]["text"].as_str().expect("tool_result must carry text payload");
    let parsed: Value = serde_json::from_str(text).expect("payload must be JSON");

    assert_eq!(parsed["success"], json!(true));
    assert_eq!(
        parsed["dry_run"],
        json!(true),
        "response must report dry_run: true so callers can tell this was a preview: {}",
        parsed
    );

    // The archive_location described in the preview must NOT exist -- a
    // preview must not touch the filesystem at all.
    let archive_root = workspace.join("archive");
    assert!(
        !archive_root.exists(),
        "clear_artifacts with dry_run:true must not create {} -- it must only preview",
        archive_root.display()
    );

    let archived = parsed["archived_files"].as_array().expect("archived_files must be an array");
    assert!(!archived.is_empty(), "preview should still describe what would be archived");
    for entry in archived {
        let dest = entry["destination"].as_str().expect("destination must be a string");
        assert!(
            !std::path::Path::new(dest).exists(),
            "previewed destination {} must NOT exist on disk under dry_run:true",
            dest
        );
    }

    // The whole point of the bug: workflow state must not be reset by a
    // dry-run preview. Query the workflow state directly to confirm the
    // audit evidence is still tracked (not wiped back to UNSTARTED).
    let state_result = server
        .call_tool(
            "query_workflow_state",
            Some(json!({ "workspace": workspace.display().to_string() })),
        )
        .expect("query_workflow_state must not fail");
    let state_text =
        state_result["content"][0]["text"].as_str().expect("tool_result must carry text payload");
    let state_parsed: Value = serde_json::from_str(state_text).expect("payload must be JSON");
    assert_ne!(
        state_parsed["state"],
        json!("UNSTARTED"),
        "clear_artifacts dry_run:true must not reset workflow state to UNSTARTED: {}",
        state_parsed
    );
    assert!(
        state_parsed["audit_file"].is_string(),
        "clear_artifacts dry_run:true must not clear the tracked audit_file: {}",
        state_parsed
    );
}

#[test]
fn clear_artifacts_dry_run_false_confirm_true_still_archives_for_real() {
    // Companion happy-path check alongside the dry_run:true preview test
    // above: dry_run:false with confirm:true must still perform the real
    // archive + state reset (unchanged existing behavior), so the fix to
    // dry_run handling didn't regress the destructive path.
    let workspace = std::env::temp_dir().join("oclnr-mcp-contract-clear-artifacts-real-run");
    let _ = std::fs::remove_dir_all(&workspace);
    let mut server = server_with_optional_audit(&workspace, /* seed_audit */ true);

    let result = server
        .call_tool("clear_artifacts", Some(json!({ "confirm": true, "dry_run": false })))
        .expect("call_tool itself should not fail at the transport level");
    assert_eq!(result["isError"], json!(false), "expected success: {}", result);

    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["dry_run"], json!(false));

    let location = parsed["archive_location"].as_str().unwrap();
    assert!(
        std::path::Path::new(location).is_dir(),
        "real run must actually create the archive directory on disk: {}",
        location
    );

    // State reset must have happened for the real run.
    let state_result = server
        .call_tool(
            "query_workflow_state",
            Some(json!({ "workspace": workspace.display().to_string() })),
        )
        .expect("query_workflow_state must not fail");
    let state_text = state_result["content"][0]["text"].as_str().unwrap();
    let state_parsed: Value = serde_json::from_str(state_text).unwrap();
    assert_eq!(
        state_parsed["state"],
        json!("UNSTARTED"),
        "real (non-dry-run) clear_artifacts must reset workflow state to UNSTARTED: {}",
        state_parsed
    );
}
