//! Pressure-triggered reclaim: real-binary and real-process tests.
//!
//! No test here can thin a snapshot or delete a file: the CLI runs use a
//! threshold of 0 GB (free space is never below 0, so every decision is
//! `Idle`), and the live-cwd test only classifies plan items.

use std::{path::PathBuf, process::Command};

use osx_clnr::domain::{
    dcm::Reversibility,
    plan::{PlanItem, PlanItemKind},
    pressure::{exclude_live_cwds, significant_cwds},
};

fn oclnr() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oclnr"))
}

#[test]
fn bounded_watch_loop_decides_idle_and_writes_no_receipts() {
    let receipts = tempfile::tempdir().unwrap();
    let out = oclnr()
        .args([
            "monitor",
            "--watch",
            "--threshold-gb",
            "0",
            "--reclaim",
            "snapshots",
            "--interval-secs",
            "1",
            "--max-iterations",
            "2",
            "--receipt-dir",
        ])
        .arg(receipts.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stdout={stdout} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout.matches("snapshots decision: Idle").count(), 2, "{stdout}");
    assert!(!stdout.contains("Thin"), "{stdout}");
    assert_eq!(std::fs::read_dir(receipts.path()).unwrap().count(), 0);
}

#[test]
fn unknown_reclaim_strategy_is_refused_by_the_cli() {
    let out = oclnr()
        .args(["monitor", "--threshold-gb", "0", "--reclaim", "snapshots,docker"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown reclaim strategy 'docker'"));
}

#[test]
fn reclaim_and_trigger_autoclean_are_mutually_exclusive() {
    let out = oclnr()
        .args(["monitor", "--threshold-gb", "0", "--reclaim", "snapshots", "--trigger-autoclean"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("mutually exclusive"));
}

#[test]
fn live_build_process_cwd_excludes_its_target_dir() {
    if Command::new("lsof").arg("-v").output().is_err() {
        eprintln!("SKIP: lsof not available");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let busy = root.path().canonicalize().unwrap().join("busy-app");
    let idle = root.path().canonicalize().unwrap().join("idle-app");
    std::fs::create_dir_all(busy.join("target")).unwrap();
    std::fs::create_dir_all(idle.join("target")).unwrap();

    // A real process standing in for `cargo build`, with cwd = project root.
    let mut child = Command::new("sleep").arg("30").current_dir(&busy).spawn().unwrap();
    let cwds = osx_clnr::integration::pressure::live_process_cwds();
    let _ = child.kill();
    let _ = child.wait();
    let home = dirs::home_dir().unwrap();
    let cwds = significant_cwds(cwds.unwrap(), &home);

    let item = |p: PathBuf| PlanItem {
        path: p,
        kind: PlanItemKind::Dir,
        reason: "rust target".into(),
        bytes: 1,
        reversibility: Reversibility::Reversible,
    };
    let (kept, excluded) =
        exclude_live_cwds(vec![item(busy.join("target")), item(idle.join("target"))], &cwds);
    assert_eq!(
        excluded.iter().map(|i| i.path.clone()).collect::<Vec<_>>(),
        vec![busy.join("target")]
    );
    assert_eq!(kept.iter().map(|i| i.path.clone()).collect::<Vec<_>>(), vec![idle.join("target")]);
}
