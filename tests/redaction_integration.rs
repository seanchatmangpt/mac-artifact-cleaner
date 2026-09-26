//! Integration test for G8 auto-redaction wiring: `oclnr audit run --redact`
//! against a real directory whose path embeds the real invoking user's
//! `/Users/<user>` home prefix, asserting the written `disk-audit.jsonocel`
//! contains no raw username and that the process reports a redaction count
//! on stderr.
//!
//! Per this repo's Chicago-school testing discipline, this runs the real
//! `oclnr` binary (via `CARGO_BIN_EXE_oclnr`) against a real filesystem —
//! no mocked `fs::write`, no stubbed redaction.
//!
//! Fixture law: the project MUST live under the real `$HOME` so the scanned
//! path contains the literal `/Users/<username>` — the exact shape the
//! redactor's contract (`find_path_matches`) covers. The original fixture
//! used `tempfile::tempdir()` (`/var/folders/...` on macOS), which can never
//! contain `/Users/<user>`: nothing was ever redacted, no count line was
//! ever printed, and the control assertion could never hold — the standing
//! red `test` job on main (CI 35663140416 fail-fast hid it behind the
//! mcp::server PATH failures; CI 36232495510 exposed it; reproduced
//! locally 2026-09-26 with the pre-existing `target/debug/oclnr`, zero
//! rebuild: OCEL contained `/tmp/.../<user>-project` and no `/Users` path).

use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn oclnr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oclnr"))
}

/// Refuses to silently pass a test whose entire point is exercising
/// `/Users/<user>` redaction when the environment cannot produce one.
fn require_users_home() -> String {
    let username = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    if username.is_empty() {
        panic!("USER/LOGNAME not set; cannot exercise real-username redaction");
    }
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.starts_with("/Users/") {
        panic!(
            "HOME ({home:?}) is not under /Users/; cannot build a fixture whose path \
             exercises the /Users/<user> redaction contract"
        );
    }
    username
}

/// Creates `<HOME>/.oclnr-redaction-test-<pid>-<nanos>/<username>-project`
/// with a `Cargo.toml` and a 4 KiB `target/bin`, and returns it plus the
/// fixture base directory (for cleanup).
fn username_home_project(username: &str) -> (PathBuf, PathBuf) {
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base = home.join(format!(".oclnr-redaction-test-{}-{}", std::process::id(), nanos));
    let project_dir = base.join(format!("{}-project", username));
    std::fs::create_dir_all(project_dir.join("target")).unwrap();
    std::fs::write(project_dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    std::fs::write(project_dir.join("target").join("bin"), vec![0u8; 4096]).unwrap();
    (project_dir, base)
}

/// Removes the fixture base when the test body finishes, success or failure.
struct Cleanup<'a>(&'a Path);

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

/// `audit run --redact` writes an OCEL log with no raw `/Users/<user>`
/// path in it, and reports a non-zero redaction count on stderr.
#[test]
fn audit_run_redact_strips_real_username_from_ocel_output() {
    let username = require_users_home();
    let (project_dir, base) = username_home_project(&username);
    let _cleanup = Cleanup(&base);

    let ocel_output = base.join("disk-audit.jsonocel");

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
    let username = require_users_home();
    let (project_dir, base) = username_home_project(&username);
    let _cleanup = Cleanup(&base);

    let ocel_output = base.join("disk-audit.jsonocel");

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
