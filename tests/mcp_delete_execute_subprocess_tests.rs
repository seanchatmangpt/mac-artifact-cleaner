//! Regression test for a previously-fixed MCP `delete_execute` bug:
//!
//! `OclnrRunner::delete_run` (src/mcp/subprocess.rs) once invoked the CLI as
//! `oclnr delete run --plan <plan> [--confirm]` -- but the real CLI verb is
//! `execute` (not `run`), and the real flag is `--yes` (not `--confirm`).
//! Because of that mismatch, *every* MCP-level `delete_execute` call failed
//! with `Subprocess 'oclnr delete run' failed`, regardless of a plan's
//! approval state -- so the workflow's intended approval/state-machine
//! gating around deletion could never actually be exercised through the MCP
//! path (only by invoking the real `oclnr` binary directly, bypassing MCP).
//!
//! This test exercises the real `Command` construction in `delete_run` end
//! to end against a stub "oclnr" executable that echoes its argv back as
//! JSON, rather than asserting on source text -- so it fails the same way
//! the real bug did (a subprocess invocation error / wrong argv) rather than
//! passing trivially.
//!
//! The fixture (`stub_runner`) is state-parameterized on the `confirm` flag
//! so both the dry-run-shaped call (`confirm: false`, used internally by
//! `delete_dry_run`) and the real-delete-shaped call (`confirm: true`, used
//! by `delete_execute`) are constructed and asserted on demand, rather than
//! only extending one shared happy-path builder.

use std::{
    fs,
    path::{Path, PathBuf},
};

use osx_clnr::mcp::subprocess::OclnrRunner;

/// Writes a stub "oclnr" shell script into `dir` that ignores whatever
/// subcommand/flags it's given and just echoes its full argv (as a JSON
/// array) to stdout, then exits 0. Returns the script's path.
///
/// This stands in for the real `oclnr` binary so the test can assert on the
/// exact command line `OclnrRunner` sends without performing a real
/// filesystem deletion or requiring `oclnr` to be built/installed.
fn write_argv_echo_stub(dir: &Path) -> PathBuf {
    let script_path = dir.join("oclnr");
    let script = r#"#!/bin/sh
printf '['
first=1
for arg in "$@"; do
  if [ "$first" -eq 1 ]; then
    first=0
  else
    printf ','
  fi
  printf '"%s"' "$arg"
done
printf ']'
"#;
    fs::write(&script_path, script).expect("write stub script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path).expect("stat stub").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod stub");
    }
    script_path
}

/// Fixture: an `OclnrRunner` pointed at the argv-echoing stub, together with
/// the workspace/plan/receipt paths `delete_run` needs. Parameterized only
/// by the caller's choice of `confirm`, so both call shapes MCP actually
/// makes (`delete_dry_run` -> confirm=false, `delete_execute` -> confirm=true)
/// can be constructed on demand from the same fixture.
fn run_delete(tmp: &Path, confirm: bool) -> Vec<String> {
    let stub = write_argv_echo_stub(tmp);
    let runner = OclnrRunner::with_binary_path(stub);

    let workspace = tmp.to_path_buf();
    let plan_file = tmp.join("cleanup-plan.json");
    let receipt_file = tmp.join("deletion-receipt.json");

    let result = runner
        .delete_run(&workspace, &plan_file, &receipt_file, confirm, None, 30)
        .expect("delete_run should not itself return an ErrorResponse for a working stub");

    assert!(
        result.success(),
        "stub subprocess should exit 0 (this would fail the same way the real \
         'oclnr delete run' bug did, via a non-zero/failed subprocess): stderr={}",
        result.stderr
    );

    let argv: Vec<String> =
        serde_json::from_str(&result.stdout).expect("stub should echo back a JSON argv array");
    argv
}

/// The real-delete shape MCP's `delete_execute` uses (`confirm: true`) must
/// send `delete execute ... --yes` -- the exact CLI contract, not the old
/// `delete run ... --confirm` mismatch.
#[test]
fn delete_execute_confirm_true_uses_real_cli_syntax() {
    let tmp = std::env::temp_dir().join(format!("oclnr-mcp-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();

    let argv = run_delete(&tmp, true);

    assert_eq!(argv[0], "delete", "first arg must be the `delete` noun");
    assert_eq!(
        argv[1], "execute",
        "second arg must be the real CLI verb `execute`, not the nonexistent `run`"
    );
    assert!(argv.contains(&"--plan".to_string()));
    assert!(argv.contains(&"--receipt".to_string()));
    assert!(
        argv.contains(&"--yes".to_string()),
        "confirm=true must translate to the real `--yes` flag, argv={:?}",
        argv
    );
    assert!(
        !argv.contains(&"--confirm".to_string()),
        "must never send the nonexistent `--confirm` flag, argv={:?}",
        argv
    );
    assert!(!argv.contains(&"run".to_string()), "must never send the nonexistent `run` verb");

    fs::remove_dir_all(&tmp).ok();
}

/// The dry-run shape (`confirm: false`, used internally by `delete_dry_run`)
/// must omit `--yes` entirely rather than sending some other placeholder
/// flag -- and must still use the real `execute` verb.
#[test]
fn delete_execute_confirm_false_omits_yes_flag() {
    let tmp = std::env::temp_dir().join(format!("oclnr-mcp-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();

    let argv = run_delete(&tmp, false);

    assert_eq!(argv[1], "execute");
    assert!(
        !argv.contains(&"--yes".to_string()),
        "confirm=false must not pass --yes, argv={:?}",
        argv
    );
    assert!(!argv.contains(&"--confirm".to_string()));

    fs::remove_dir_all(&tmp).ok();
}
