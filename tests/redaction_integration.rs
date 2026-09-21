//! Integration test for G8 auto-redaction wiring: `oclnr audit run --redact`
//! against a real temp directory whose path embeds the real invoking
//! user's `$HOME` username, asserting the written `disk-audit.jsonocel`
//! contains no raw username and that the process reports a redaction count
//! on stderr.
//!
//! Per this repo's Chicago-school testing discipline, this runs the real
//! `oclnr` binary (via `CARGO_BIN_EXE_oclnr`) against a real filesystem —
//! no mocked `fs::write`, no stubbed redaction.

use std::{path::PathBuf, process::Command};

fn oclnr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oclnr"))
}

/// `audit run --redact` writes an OCEL log with no raw `/Users/<user>`
/// path in it, and reports a non-zero redaction count on stderr.
#[test]
fn audit_run_redact_strips_real_username_from_ocel_output() {
    let username = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    if username.is_empty() {
        // No real username to assert against in this environment -- refuse
        // to silently pass a test whose entire point is checking for one.
        panic!("USER/LOGNAME not set; cannot exercise real-username redaction");
    }

    let scan_root = tempfile_dir();
    // Give the scanned tree a path segment that embeds the real username,
    // mirroring what a real home-directory scan would surface.
    let project_dir = scan_root.join(format!("{}-project", username));
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let target = project_dir.join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("bin"), vec![0u8; 4096]).unwrap();

    let ocel_output = scan_root.join("disk-audit.jsonocel");

    let output = oclnr()
        .args(["audit", "run", "--root"])
        .arg(&project_dir)
        .args(["--ocel-output"])
        .arg(&ocel_output)
        .arg("--redact")
        .output()
        .expect("failed to spawn `oclnr audit run --redact`");

    assert!(
        output.status.success(),
        "`oclnr audit run --redact` failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("redacted") && stderr.contains("item(s)"),
        "expected a redaction-count line on stderr, got:\n{stderr}"
    );

    let written = std::fs::read_to_string(&ocel_output)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", ocel_output.display()));
    let raw_user_path = format!("/Users/{}", username);
    assert!(
        !written.contains(&raw_user_path),
        "expected the real username's path to be redacted from the OCEL log, got:\n{written}"
    );
    assert!(
        written.contains("/Users/<user>"),
        "expected a redacted placeholder path in the OCEL log, got:\n{written}"
    );
}

/// `audit run` without `--redact` writes the raw username unchanged (control
/// case, proving the assertions above are actually exercising redaction and
/// not some other path-normalization behavior).
#[test]
fn audit_run_without_redact_leaves_real_username_intact() {
    let username = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    if username.is_empty() {
        panic!("USER/LOGNAME not set; cannot exercise real-username redaction");
    }

    let scan_root = tempfile_dir();
    let project_dir = scan_root.join(format!("{}-project", username));
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(project_dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let target = project_dir.join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("bin"), vec![0u8; 4096]).unwrap();

    let ocel_output = scan_root.join("disk-audit.jsonocel");

    let output = oclnr()
        .args(["audit", "run", "--root"])
        .arg(&project_dir)
        .args(["--ocel-output"])
        .arg(&ocel_output)
        .output()
        .expect("failed to spawn `oclnr audit run`");

    assert!(
        output.status.success(),
        "`oclnr audit run` failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let written = std::fs::read_to_string(&ocel_output).unwrap();
    let raw_user_path = format!("/Users/{}", username);
    assert!(
        written.contains(&raw_user_path),
        "control case: expected the raw username path WITHOUT --redact, got:\n{written}"
    );
}

fn tempfile_dir() -> PathBuf {
    let dir = tempfile::tempdir().unwrap();
    // Leak the TempDir so it outlives the test body without needing to wire
    // a guard through every call site -- matches this repo's existing
    // integration test style of using tempfile for real on-disk fixtures.
    dir.keep()
}
