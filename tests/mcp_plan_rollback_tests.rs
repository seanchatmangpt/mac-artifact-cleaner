//! Regression tests for `plan_rollback`.
//!
//! `plan_rollback` must be scoped to the receipt/snapshot it is rolling
//! back: a missing, nonexistent, or malformed `receipt_file` must be
//! rejected the same way `receipt_parse` / `receipt_verify` reject it,
//! rather than silently ignored while still returning a normal-looking
//! success payload.

use osx_clnr::mcp::OsxClnrMcpServer;
use serde_json::json;
use tempfile::tempdir;

/// The receipt-file state under test. Parameterizing on this (rather than
/// only extending a single happy-path builder) lets the fixture construct
/// the previously-buggy state (`Absent`, `Empty`) on demand alongside the
/// valid-file case, so the same harness proves both the bug and the fix.
enum ReceiptFileState {
    /// No `receipt_file` argument supplied at all.
    Absent,
    /// `receipt_file` points at a path that does not exist.
    Nonexistent,
    /// `receipt_file` points at a 0-byte file (invalid JSON).
    Empty,
    /// `receipt_file` points at a file containing valid JSON but not a
    /// valid `DeletionReceipt` shape.
    InvalidShape,
}

/// Build the `plan_rollback` params for a given receipt-file state,
/// materializing any on-disk fixture file needed.
fn build_params(state: ReceiptFileState, dir: &std::path::Path) -> serde_json::Value {
    let mut params = json!({ "confirm": true });
    match state {
        ReceiptFileState::Absent => {}
        ReceiptFileState::Nonexistent => {
            let path = dir.join("does-not-exist-receipt.jsonocel");
            params["receipt_file"] = json!(path.to_string_lossy());
        }
        ReceiptFileState::Empty => {
            let path = dir.join("empty-receipt.jsonocel");
            std::fs::write(&path, "").unwrap();
            params["receipt_file"] = json!(path.to_string_lossy());
        }
        ReceiptFileState::InvalidShape => {
            let path = dir.join("bad-shape-receipt.jsonocel");
            std::fs::write(&path, r#"{"not":"a receipt"}"#).unwrap();
            params["receipt_file"] = json!(path.to_string_lossy());
        }
    }
    params
}

/// `call_tool` never returns `Err` itself: every tool error is surfaced as
/// `Ok({"isError": true, "content": [{"text": "<message>"}]})`. Extract that
/// message (panicking loudly if the call unexpectedly "succeeded") so tests
/// can assert on the actual error text regardless of that envelope.
fn call_plan_rollback_expect_error(params: serde_json::Value) -> String {
    let workspace = tempdir().unwrap().keep();
    let mut server = OsxClnrMcpServer::new(workspace).expect("server construction");
    let result = server.call_tool("plan_rollback", Some(params)).expect("call_tool itself failed");

    let is_error = result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
    let text = result["content"][0]["text"].as_str().unwrap_or_default().to_string();
    assert!(is_error, "expected plan_rollback to report isError:true, got: {result}");
    text
}

#[test]
fn plan_rollback_rejects_missing_receipt_file_argument() {
    let dir = tempdir().unwrap();
    let params = build_params(ReceiptFileState::Absent, dir.path());
    let err = call_plan_rollback_expect_error(params);
    assert!(
        err.to_lowercase().contains("receipt_file"),
        "expected error mentioning receipt_file, got: {err}"
    );
}

#[test]
fn plan_rollback_rejects_nonexistent_receipt_file() {
    let dir = tempdir().unwrap();
    let params = build_params(ReceiptFileState::Nonexistent, dir.path());
    let err = call_plan_rollback_expect_error(params);
    assert!(
        err.to_lowercase().contains("not found") || err.to_lowercase().contains("no such"),
        "expected file-not-found error, got: {err}"
    );
}

#[test]
fn plan_rollback_rejects_empty_receipt_file() {
    // This is the exact repro from the bug report: a 0-byte receipt_file
    // must not produce a normal-looking `{"restored": false, ...}` payload.
    let dir = tempdir().unwrap();
    let params = build_params(ReceiptFileState::Empty, dir.path());
    let err = call_plan_rollback_expect_error(params);
    assert!(
        err.to_lowercase().contains("invalid receipt json"),
        "expected 'invalid receipt JSON' error (matching receipt_parse/receipt_verify), got: {err}"
    );
}

#[test]
fn plan_rollback_rejects_wrong_shape_receipt_file() {
    let dir = tempdir().unwrap();
    let params = build_params(ReceiptFileState::InvalidShape, dir.path());
    let err = call_plan_rollback_expect_error(params);
    assert!(
        err.to_lowercase().contains("invalid receipt json"),
        "expected 'invalid receipt JSON' error, got: {err}"
    );
}

/// Sanity check that unrelated params don't accidentally satisfy the check
/// via a different key.
#[test]
fn plan_rollback_ignores_unrelated_keys_and_still_requires_receipt_file() {
    let dir = tempdir().unwrap();
    let mut params = build_params(ReceiptFileState::Absent, dir.path());
    params["mount"] = json!("/");
    params["some_other_field"] = json!("value");
    let err = call_plan_rollback_expect_error(params);
    assert!(err.to_lowercase().contains("receipt_file"));
}
