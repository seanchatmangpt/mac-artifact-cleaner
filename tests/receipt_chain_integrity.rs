//! Regression test for the affidavit `chain_integrity` check.
//!
//! Repro (pre-fix):
//!   1. `oclnr delete execute` produces `deletion-receipt.jsonocel` +
//!      `deletion-receipt.affidavit.json` (the latter carrying a real
//!      `chain_hash`).
//!   2. An attacker (or an innocent hand-edit) overwrites `chain_hash` in the
//!      `.affidavit.json` file with an arbitrary value unrelated to the
//!      actual event payloads (e.g. `"deadbeef" * 8`).
//!   3. `oclnr receipt verify` / `oclnr receipt certify` never read that
//!      on-disk file at all -- they rebuilt a fresh affidavit `Receipt` from
//!      the (untampered) `.jsonocel` receipt and self-certified it, so the
//!      tampered `chain_hash` was never compared against anything. The
//!      command always printed "chain_hash matches" and exited 0/ACCEPTED,
//!      even though the freshly recomputed hash plainly differed from the
//!      stored one.
//!
//! Fix: both CLI paths now read back the on-disk `.affidavit.json` (if
//! present) and fold a `chain_integrity` comparison into the verdict via
//! `domain::affidavit_integration::verify_chain_hash`. A mismatch rejects.
//!
//! This test is state-parameterized: [`write_receipt_and_affidavit`] can
//! construct either the healthy state (affidavit file matches) or the buggy
//! state (affidavit file tampered) on demand, rather than only extending a
//! single shared happy-path builder.

use std::{path::Path, process::Command};

use osx_clnr::domain::{
    affidavit_integration::{build_deletion_affidavit, serialize_receipt},
    receipt::{DeletionReceipt, DeletionResult, DeletionStatus},
};

/// Tamper mode for the affidavit file written alongside the receipt.
#[derive(Clone, Copy)]
enum AffidavitState {
    /// The affidavit file's `chain_hash` matches what a fresh recompute
    /// would produce (the honest, never-touched-after-sealing state).
    Untampered,
    /// The affidavit file's `chain_hash` has been overwritten with an
    /// arbitrary value bearing no relation to the actual event payloads --
    /// the exact tamper described in the bug report.
    Tampered,
}

/// Fixture: write a `DeletionReceipt` (`.jsonocel`) and its sealed affidavit
/// (`.affidavit.json`) into `dir`, constructing either state on demand.
/// Returns the receipt path.
fn write_receipt_and_affidavit(dir: &Path, state: AffidavitState) -> std::path::PathBuf {
    let receipt = DeletionReceipt::new(
        0,
        1,
        2,
        vec![DeletionResult {
            path: "/tmp/build-artifact".into(),
            status: DeletionStatus::Deleted,
            error: None,
            blake3_hash: None,
            bytes_freed: 4096,
        }],
        Some(8_000_000_000),
        Some(8_000_004_096),
    );

    let receipt_path = dir.join("deletion-receipt.jsonocel");
    std::fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();

    let sealed = build_deletion_affidavit(&receipt);
    let mut bytes = serialize_receipt(&sealed);

    if let AffidavitState::Tampered = state {
        // Hand-edit chain_hash to an arbitrary value with no relation to the
        // actual event payloads -- exactly the repro from the bug report.
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["chain_hash"] = serde_json::Value::String("deadbeef".repeat(8));
        bytes = serde_json::to_vec_pretty(&value).unwrap();
    }

    let affidavit_path = receipt_path.with_extension("affidavit.json");
    std::fs::write(&affidavit_path, bytes).unwrap();

    receipt_path
}

fn oclnr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oclnr"))
}

#[test]
fn receipt_verify_rejects_tampered_chain_hash() {
    let dir = tempfile::tempdir().unwrap();
    let receipt_path = write_receipt_and_affidavit(dir.path(), AffidavitState::Tampered);

    let output =
        oclnr().args(["receipt", "verify", "--receipt"]).arg(&receipt_path).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !output.status.success(),
        "receipt verify must fail when the stored chain_hash was tampered with, but exited 0.\nstdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("chain_integrity") || stdout.contains("❌"),
        "expected a failing chain_integrity outcome in the output, got:\n{stdout}"
    );
}

#[test]
fn receipt_verify_accepts_untampered_chain_hash() {
    let dir = tempfile::tempdir().unwrap();
    let receipt_path = write_receipt_and_affidavit(dir.path(), AffidavitState::Untampered);

    let output =
        oclnr().args(["receipt", "verify", "--receipt"]).arg(&receipt_path).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "receipt verify must pass when the stored chain_hash matches the recomputed one.\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn receipt_certify_rejects_tampered_chain_hash() {
    let dir = tempfile::tempdir().unwrap();
    let receipt_path = write_receipt_and_affidavit(dir.path(), AffidavitState::Tampered);

    let output =
        oclnr().args(["receipt", "certify", "--receipt"]).arg(&receipt_path).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !output.status.success(),
        "receipt certify must REJECT when the stored chain_hash was tampered with (a freshly \
         recomputed hash differing from the stored one is exactly the tamper signal), but it \
         exited 0.\nstdout:\n{stdout}"
    );
}

#[test]
fn receipt_certify_accepts_untampered_chain_hash() {
    let dir = tempfile::tempdir().unwrap();
    let receipt_path = write_receipt_and_affidavit(dir.path(), AffidavitState::Untampered);

    let output =
        oclnr().args(["receipt", "certify", "--receipt"]).arg(&receipt_path).output().unwrap();

    assert!(
        output.status.success(),
        "receipt certify must ACCEPT when the stored chain_hash matches the recomputed one.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
