//! Real-filesystem tests for the read-only `dedupe scan` capability.
//!
//! Every fixture lives in a real `tempfile::tempdir()`: identical copies, a
//! unique file, a hardlink, a symlink, and a genuine APFS clone made with
//! `cp -c` (clonefile). No test doubles; assertions are on the real report
//! and on the unchanged bytes of the files afterwards (scan is read-only).

use std::{fs, path::Path, process::Command};

use osx_clnr::{
    domain::dedupe::DupGroup,
    integration::dedupe::{private_size, scan_duplicates},
};

const SZ: usize = 256 * 1024;

fn write(path: &Path, byte: u8) {
    // Non-constant content so the file is not trivially compressible/sparse.
    let data: Vec<u8> = (0..SZ).map(|i| byte ^ (i % 251) as u8).collect();
    fs::write(path, data).unwrap();
}

fn group_containing<'a>(groups: &'a [DupGroup], name: &str) -> Option<&'a DupGroup> {
    groups.iter().find(|g| g.members.iter().any(|m| m.path.ends_with(name)))
}

#[test]
fn dedupe_scan_counts_copies_and_not_existing_clones() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Three independent copies of content A -> 2 * SZ reclaimable.
    write(&root.join("a1.bin"), 1);
    fs::create_dir(root.join("sub")).unwrap();
    write(&root.join("sub/a2.bin"), 1);
    write(&root.join("sub/a3.bin"), 1);
    // A hardlink to a1 must not count as a duplicate.
    fs::hard_link(root.join("a1.bin"), root.join("a1.hardlink")).unwrap();
    // A symlink to a1 must be skipped.
    std::os::unix::fs::symlink(root.join("a1.bin"), root.join("a1.symlink")).unwrap();

    // A unique file of the same size (different content) -> no group.
    write(&root.join("unique.bin"), 99);

    // An APFS clone pair of content B -> ~0 reclaimable when private size is known.
    write(&root.join("b_src.bin"), 2);
    let cp = Command::new("cp")
        .arg("-c")
        .arg(root.join("b_src.bin"))
        .arg(root.join("b_clone.bin"))
        .status()
        .expect("run cp -c");
    assert!(cp.success(), "cp -c (clonefile) failed; tempdir must be on APFS");

    // Below min size -> ignored.
    fs::write(root.join("tiny1"), b"same").unwrap();
    fs::write(root.join("tiny2"), b"same").unwrap();

    let before = fs::read(root.join("sub/a2.bin")).unwrap();
    let report = scan_duplicates(&[root.to_path_buf()], 64 * 1024).unwrap();

    // Two groups: content A (3 files) and the B clone pair.
    assert_eq!(report.group_count, 2, "{report:#?}");
    assert_eq!(report.duplicate_files, 5);
    assert!(report.execute_status.starts_with("UNSUPPORTED"));

    let a = group_containing(&report.groups, "a1.bin").expect("group A");
    assert_eq!(a.members.len(), 3, "hardlink/symlink must not join the group");
    assert!(a.keeper.ends_with("a1.bin") || a.keeper.ends_with("a1.hardlink"));
    assert!(group_containing(&report.groups, "unique.bin").is_none());
    assert!(group_containing(&report.groups, "tiny1").is_none());

    let b = group_containing(&report.groups, "b_clone.bin").expect("group B");
    assert_eq!(b.members.len(), 2);
    assert_eq!(report.logical_duplicate_bytes, 3 * SZ as u64);

    if private_size(&root.join("b_clone.bin")).is_some() {
        assert!(!b.estimate);
        // Clone pair shares extents: at most a few blocks of private data.
        assert!(b.reclaimable_bytes < 16 * 1024, "clone pair reclaim {}", b.reclaimable_bytes);
        // Independent copies each own all their blocks.
        assert!(a.reclaimable_bytes >= 2 * SZ as u64, "copies reclaim {}", a.reclaimable_bytes);
        assert!(!report.estimate);
        assert!(report.reclaimable_bytes < report.logical_duplicate_bytes);
    } else {
        // Fallback path: logical sizes, flagged as an estimate.
        assert!(report.estimate);
        assert_eq!(report.reclaimable_bytes, 3 * SZ as u64);
    }

    // Read-only: content untouched, clone still present.
    assert_eq!(fs::read(root.join("sub/a2.bin")).unwrap(), before);
    assert!(root.join("b_clone.bin").exists());
}

#[test]
fn dedupe_scan_refuses_missing_root() {
    let err = scan_duplicates(&["/nonexistent/dedupe/root/xyz".into()], 1).unwrap_err();
    assert!(err.to_string().contains("Cannot stat dedupe root"), "{err}");
}

#[test]
fn dedupe_cli_scan_writes_json_report() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("x1"), 5);
    write(&dir.path().join("x2"), 5);
    let out = dir.path().join("report.json");
    let status = Command::new(env!("CARGO_BIN_EXE_oclnr"))
        .args(["dedupe", "scan", "--root"])
        .arg(dir.path())
        .args(["--min-size", "65536", "--output"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("Duplicate groups:     1"), "{stdout}");
    let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(json["group_count"], 1);
    assert_eq!(json["duplicate_files"], 2);
}
