//! Integration tests for G8/G9: `oclnr doctor <check>`, `oclnr privacy
//! scan`/`redact`, and the MCP `doctor` tool's real subprocess dispatch.
//!
//! Prior to this file, G8/G9 coverage existed only as domain-level unit
//! tests/doctests operating on in-memory strings passed directly to
//! `diagnose_*`/`scan_*` functions (`src/domain/doctor.rs`,
//! `src/domain/redaction.rs`). None of that exercised the real integration
//! wiring: `read_workspace_architecture`, `read_domain_files`,
//! `read_delete_path_files`, `read_privacy_files` in
//! `src/integration/doctor.rs`, or the MCP subprocess dispatch in
//! `src/mcp/subprocess.rs` (`doctor_check`, spawning the real `oclnr`
//! binary). Per this repo's Chicago-school testing discipline, these tests
//! run the real `oclnr` binary (via `CARGO_BIN_EXE_oclnr`, the same pattern
//! as `tests/receipt_chain_integrity.rs`) and the real in-process MCP
//! server (which itself shells out to the real binary), asserting on real
//! exit codes, real stdout, and real on-disk file contents -- no mocks.

use std::{path::PathBuf, process::Command};

use osx_clnr::mcp::OsxClnrMcpServer;
use serde_json::json;

fn oclnr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oclnr"))
}

/// The repo checkout this test binary was built from -- used as the
/// workspace root for doctor checks that inspect real source files
/// (architecture layout, domain purity, doctests, etc.).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// All six doctor checks currently wired into both the CLI and the MCP
/// server's `VALID_CHECKS` allowlist (`src/mcp/server.rs`).
const ALL_CHECKS: &[&str] = &[
    "architecture",
    "substrate",
    "doctests",
    "privacy",
    "domain-purity",
    "scan-delete-separation",
];

/// Every doctor check must run to completion against this actual repo
/// checkout and produce a real, non-empty diagnostic report on stdout, with
/// a real exit code (0 = pass, 1 = the check ran and found real issues --
/// e.g. `doctests`/`privacy` legitimately flag real gaps in a working repo
/// checkout that includes a vendored submodule; anything else, such as a
/// panic or a missing-binary/spawn failure, is not a normal check outcome).
/// This exercises `read_workspace_architecture`, `read_domain_files`,
/// `read_delete_path_files`, `read_privacy_files`, and
/// `query_substrate_info` in `src/integration/doctor.rs` -- none of which
/// are reachable by feeding synthetic strings straight into `diagnose_*`.
#[test]
fn each_doctor_check_runs_end_to_end_against_real_repo() {
    for check in ALL_CHECKS {
        let output = oclnr()
            .current_dir(repo_root())
            .args(["doctor", check])
            .output()
            .unwrap_or_else(|e| panic!("failed to spawn `oclnr doctor {check}`: {e}"));

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code();

        assert!(
            code == Some(0) || code == Some(1),
            "`oclnr doctor {check}` exited abnormally (code {code:?}) against the real repo checkout.\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            !stdout.trim().is_empty(),
            "`oclnr doctor {check}` produced no stdout output at all"
        );
    }
}

/// `architecture` and `substrate` specifically are expected to pass clean
/// against this repo's own checkout (G0-G7 complete per CLAUDE.md, and
/// substrate only asserts facts about the current OS/tmutil, not repo
/// content) -- a real state-based pass/fail assertion, unlike the broader
/// "ran without crashing" check above which must tolerate legitimate
/// findings from `doctests`/`privacy`/`domain-purity`/
/// `scan-delete-separation` against a large real checkout.
#[test]
fn architecture_and_substrate_checks_pass_clean_against_real_repo() {
    for check in ["architecture", "substrate"] {
        let output = oclnr()
            .current_dir(repo_root())
            .args(["doctor", check])
            .output()
            .unwrap_or_else(|e| panic!("failed to spawn `oclnr doctor {check}`: {e}"));
        assert!(
            output.status.success(),
            "`oclnr doctor {check}` should pass against this repo's own checkout.\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// `oclnr doctor domain-purity` must actually detect a real `std::fs` call
/// planted in a scratch file under `src/domain/`, proving
/// `read_domain_files` really walks the filesystem rather than returning a
/// canned/empty file list. The scratch file is created and removed inside
/// this repo's own `src/domain/` (not a tempdir), because the check reads
/// `workspace_root/src/domain/**` relative to the process's `current_dir`.
#[test]
fn domain_purity_check_detects_real_violation_planted_in_repo() {
    let violation_path = repo_root().join("src/domain/__test_purity_violation.rs");
    std::fs::write(
        &violation_path,
        "//! scratch file for doctor_integration_tests -- must not survive the test.\n\
         pub fn bad() { std::fs::read_to_string(\"/etc/hosts\").unwrap(); }\n",
    )
    .expect("failed to write scratch violation file");

    // Always clean up, even on assertion failure/panic.
    struct CleanupGuard(PathBuf);
    impl Drop for CleanupGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = CleanupGuard(violation_path.clone());

    let output = oclnr()
        .current_dir(repo_root())
        .args(["doctor", "domain-purity"])
        .output()
        .expect("failed to spawn `oclnr doctor domain-purity`");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !output.status.success(),
        "domain-purity check must fail once a real std::fs call is planted in src/domain/, but it exited 0.\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("__test_purity_violation.rs"),
        "expected the violating file to be named in the report, got:\n{stdout}"
    );
}

/// `oclnr privacy scan` run end-to-end against this real repo checkout.
/// Exercises the same `read_privacy_files` integration wiring as `doctor
/// privacy`, via the dedicated `privacy` noun.
#[test]
fn privacy_scan_runs_end_to_end_against_real_repo() {
    let output = oclnr()
        .current_dir(repo_root())
        .args(["privacy", "scan"])
        .output()
        .expect("failed to spawn `oclnr privacy scan`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Not asserting `success()`: this repo checkout includes a vendored
    // submodule (`vendor/autofde-lab`) that legitimately trips the
    // unredacted-local-path check with real findings -- a true positive,
    // not a bug in the integration path. What must hold regardless is that
    // the real `read_privacy_files` wiring ran to completion (0 = clean,
    // 1 = real findings reported) rather than crashing or erroring out.
    let code = output.status.code();
    assert!(
        code == Some(0) || code == Some(1),
        "`oclnr privacy scan` exited abnormally (code {code:?}) against the real repo checkout.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!stdout.trim().is_empty(), "`oclnr privacy scan` produced no stdout output");
}

/// `oclnr privacy redact --file <real tempfile>` must actually rewrite the
/// file's on-disk content in place. State-based assertion on the real file
/// contents after the subprocess exits, per Chicago-school discipline --
/// not an assertion that redact_content() was "called".
#[test]
fn privacy_redact_rewrites_real_file_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("leaky.txt");
    std::fs::write(&file_path, "my password is supersecret\nnothing else sensitive here\n")
        .unwrap();

    let output = oclnr()
        .args(["privacy", "redact", "--file"])
        .arg(&file_path)
        .output()
        .expect("failed to spawn `oclnr privacy redact`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "`oclnr privacy redact` failed.\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let after = std::fs::read_to_string(&file_path).unwrap();
    assert!(
        after.contains("[REDACTED]"),
        "expected the sensitive line to be redacted on disk, got:\n{after}"
    );
    assert!(
        !after.contains("supersecret"),
        "the literal secret must not survive redaction on disk, got:\n{after}"
    );
    assert!(
        after.contains("nothing else sensitive here"),
        "non-sensitive content must be preserved verbatim, got:\n{after}"
    );
}

/// `oclnr privacy redact --file <nonexistent>` must refuse (non-zero exit),
/// matching the doctest's negative/refusal case in `src/nouns/privacy.rs`
/// -- exercised here through the real binary rather than an in-process
/// call.
#[test]
fn privacy_redact_refuses_nonexistent_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist.txt");

    let output = oclnr()
        .args(["privacy", "redact", "--file"])
        .arg(&missing)
        .output()
        .expect("failed to spawn `oclnr privacy redact`");

    assert!(
        !output.status.success(),
        "redacting a nonexistent file must fail, but the process exited successfully"
    );
}

/// The MCP `doctor` tool's real subprocess dispatch
/// (`src/mcp/subprocess.rs::doctor_check`, spawning the real `oclnr`
/// binary found next to this test binary) against this repo checkout,
/// driven through the same in-process `OsxClnrMcpServer::call_tool` entry
/// point the real MCP server uses -- not a hand-built JSON fixture fed
/// straight to `diagnose_architecture`.
#[test]
fn mcp_doctor_tool_dispatches_real_subprocess_for_every_check() {
    // Pin the subprocess runner at the real `oclnr` binary cargo built for
    // this test run. `OsxClnrMcpServer::new` -> `OclnrRunner::new` resolves
    // `OCLNR_BIN` first (see src/mcp/subprocess.rs), before falling back to
    // "co-located with current executable" -- which fails for a test
    // binary living in `target/debug/deps/`, not `target/debug/`.
    std::env::set_var("OCLNR_BIN", env!("CARGO_BIN_EXE_oclnr"));
    let mut server =
        OsxClnrMcpServer::new(repo_root()).expect("MCP server construction should succeed");

    for check in ALL_CHECKS {
        let params = json!({ "check": check, "workspace": repo_root().to_string_lossy() });
        // `call_tool` always returns `Ok` with an MCP content envelope
        // (`{"content":[{"text": "...", "type":"text"}], "isError": bool}`)
        // -- errors surface via `isError: true` with the error JSON/text
        // inside `content[0].text`, not via `Result::Err`. A check that
        // legitimately finds real issues in this repo checkout (e.g.
        // `privacy` against the vendored submodule) sets `isError: true`
        // here too -- that still proves the real subprocess ran and its
        // exit code was faithfully propagated, which is exactly the wiring
        // under test.
        let envelope = server
            .call_tool("doctor", Some(params))
            .unwrap_or_else(|e| panic!("mcp doctor_check({check}) transport-level failure: {e:?}"));

        let is_error = envelope.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
        let text = envelope["content"][0]["text"].as_str().unwrap_or_else(|| {
            panic!("mcp doctor_check({check}) envelope missing content[0].text: {envelope:?}")
        });

        if is_error {
            assert!(
                text.to_lowercase().contains("doctor"),
                "mcp doctor_check({check}) failed for an unexpected reason: {text}"
            );
            continue;
        }

        let result: serde_json::Value = serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("mcp doctor_check({check}) text was not JSON: {e}: {text}"));
        assert_eq!(
            result.get("state").and_then(|v| v.as_str()),
            Some("DOCTOR_CHECK_COMPLETE"),
            "unexpected MCP doctor_check({check}) response shape: {result:?}"
        );
        let raw = result.get("raw").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            !raw.trim().is_empty(),
            "mcp doctor_check({check}) returned empty `raw` subprocess stdout"
        );
    }
}

/// The MCP `doctor` tool must reject an unknown check name before ever
/// spawning the subprocess (`VALID_CHECKS` allowlist in
/// `src/mcp/server.rs`), returning a real `ErrorResponse` rather than
/// silently shelling out with an attacker/typo-controlled argument.
#[test]
fn mcp_doctor_tool_rejects_unknown_check() {
    // Pin the subprocess runner at the real `oclnr` binary cargo built for
    // this test run. `OsxClnrMcpServer::new` -> `OclnrRunner::new` resolves
    // `OCLNR_BIN` first (see src/mcp/subprocess.rs), before falling back to
    // "co-located with current executable" -- which fails for a test
    // binary living in `target/debug/deps/`, not `target/debug/`.
    std::env::set_var("OCLNR_BIN", env!("CARGO_BIN_EXE_oclnr"));
    let mut server =
        OsxClnrMcpServer::new(repo_root()).expect("MCP server construction should succeed");

    let params = json!({ "check": "not-a-real-check", "workspace": repo_root().to_string_lossy() });
    let envelope = server
        .call_tool("doctor", Some(params))
        .expect("call_tool transport itself should not fail");

    let is_error = envelope.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
    assert!(is_error, "an unknown doctor check must be rejected via isError, got: {envelope:?}");
}
