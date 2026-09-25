//! Projections written by oclnr must be admitted by the fleet receipt
//! validator (`~/.claude/dfcm/validate_receipt.py` against
//! `receipt.schema.json`). Uses the real validator and a real git repo (this
//! checkout, via the build-stamped commit); skips with a printed reason when
//! the validator is not installed on the machine, rather than faking a pass.

use std::path::PathBuf;

use osx_clnr::{
    domain::{
        r_projection::{project_deletion, project_snapshot_thin},
        receipt::{DeletionReceipt, DeletionResult, DeletionStatus},
        time::SnapshotThinReceipt,
    },
    integration::r_projection::{context_for, write_projection},
};

fn validator() -> Option<PathBuf> {
    let p = dirs::home_dir()?.join(".claude/dfcm/validate_receipt.py");
    p.exists().then_some(p)
}

fn validate(path: &std::path::Path) -> (bool, String) {
    let v = validator().expect("checked by caller");
    let out = std::process::Command::new("python3").arg(v).arg(path).output().expect("run python3");
    let text =
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

#[test]
fn deletion_and_thin_projections_are_admitted_by_fleet_validator() {
    if validator().is_none() {
        eprintln!("SKIPPED: ~/.claude/dfcm/validate_receipt.py not installed on this machine");
        return;
    }
    let dir = tempfile::tempdir().unwrap();

    let del = DeletionReceipt::new(
        0,
        1,
        2,
        vec![DeletionResult {
            path: "/tmp/p/target".into(),
            status: DeletionStatus::Deleted,
            error: None,
            blake3_hash: None,
            bytes_freed: 4096,
            reversibility: Default::default(),
        }],
        Some(0),
        Some(4096),
    );
    let native = dir.path().join("deletion-receipt.jsonocel");
    std::fs::write(&native, serde_json::to_vec_pretty(&del).unwrap()).unwrap();
    let ctx =
        context_for(&native, "test-approver", "plan-approval hmac:test", "oclnr-plan:test", 0)
            .unwrap();
    let out = write_projection(&native, &project_deletion(&del, &ctx)).unwrap();
    let (ok, text) = validate(&out);
    assert!(ok, "deletion projection refused: {text}");
    assert!(text.contains("ADMITTED"), "{text}");

    let thin = SnapshotThinReceipt::new("/".into(), 10, 0, vec![], vec![]);
    let native = dir.path().join("snapshot-thin-receipt.json");
    std::fs::write(&native, serde_json::to_vec_pretty(&thin).unwrap()).unwrap();
    let ctx = context_for(&native, "oclnr", "pressure-policy: test", "oclnr-pressure-policy:/", 0)
        .unwrap();
    let out = write_projection(&native, &project_snapshot_thin(&thin, &ctx)).unwrap();
    let (ok, text) = validate(&out);
    assert!(ok, "thin projection refused: {text}");
}

#[test]
fn refused_projection_is_still_schema_valid() {
    // An empty grant yields REFUSED(no-grant) + broken_term — the schema's
    // if/then requires broken_term for REFUSED, so this must still admit
    // structurally (the receipt honestly records its own refusal).
    if validator().is_none() {
        eprintln!("SKIPPED: ~/.claude/dfcm/validate_receipt.py not installed on this machine");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let thin = SnapshotThinReceipt::new("/".into(), 10, 0, vec![], vec![]);
    let native = dir.path().join("t.json");
    std::fs::write(&native, serde_json::to_vec(&thin).unwrap()).unwrap();
    let ctx = context_for(&native, "oclnr", "", "oclnr-snapshot-thin:operator:/", 0).unwrap();
    let r = project_snapshot_thin(&thin, &ctx);
    assert_eq!(r.standing.value, "REFUSED(no-grant)");
    let out = write_projection(&native, &r).unwrap();
    let (ok, text) = validate(&out);
    assert!(ok, "{text}");
}

#[test]
fn projection_without_work_order_refuses_itself_and_stays_schema_valid() {
    // Falsifier for the v2 fields: a missing work order must surface as a typed
    // REFUSED in the receipt, never as a blank id the schema would reject.
    if validator().is_none() {
        eprintln!("SKIPPED: ~/.claude/dfcm/validate_receipt.py not installed on this machine");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let thin = SnapshotThinReceipt::new("/".into(), 10, 0, vec![], vec![]);
    let native = dir.path().join("t.json");
    std::fs::write(&native, serde_json::to_vec(&thin).unwrap()).unwrap();
    let ctx = context_for(&native, "oclnr", "operator-invocation: test", "", 0).unwrap();
    let r = project_snapshot_thin(&thin, &ctx);
    assert_eq!(r.standing.value, "REFUSED(no-work-order)");
    assert_eq!(r.standing.broken_term.as_deref(), Some("R_missing_authority"));
    let out = write_projection(&native, &r).unwrap();
    let (ok, text) = validate(&out);
    assert!(ok, "{text}");
}
