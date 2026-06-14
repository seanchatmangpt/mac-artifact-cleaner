use dashmap::DashMap;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use osx_clnr::domain::artifact::{
    artifact_candidates_from_snapshot, detect_project_from_snapshot, ArgsSnapshot, Candidate,
};
use osx_clnr::domain::audit::Stats;
use osx_clnr::domain::delete::{validate_plan_item, DeletionPlanAdjudicator, PlanSafetyWitness};
use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
use osx_clnr::domain::receipt::{DeletionReceipt, DeletionStatus};
use osx_clnr::integration::fs::scan_root;
use osx_clnr::integration::fs::{delete_dir_all, delete_file, read_dir_snapshot};
use wasm4pm_compat::admission::Admit;
use wasm4pm_compat::evidence::Evidence;
use wasm4pm_compat::state::Raw;

#[test]
fn test_project_detection_and_candidates() {
    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let project_dir = tmp.path().join("my-rust-project");
    fs::create_dir(&project_dir).unwrap();

    // Create rust project marker
    File::create(project_dir.join("Cargo.toml")).unwrap();
    // Create rebuildable build artifact folder
    let target_dir = project_dir.join("target");
    fs::create_dir(&target_dir).unwrap();

    // Integration layer builds the inert snapshot; domain receives only the DTO.
    let snap = read_dir_snapshot(&project_dir);
    let kind = detect_project_from_snapshot(&snap);
    assert!(kind.is_some());
    assert_eq!(kind.unwrap().names[0], "rust");

    let args = ArgsSnapshot {
        deps: true,
        aggressive: true,
        verbose: false,
        tool_roots: false,
        ignore_recent_hours: 1,
    };
    let snap2 = read_dir_snapshot(&project_dir);
    let project = detect_project_from_snapshot(&snap2).unwrap();
    let candidates = artifact_candidates_from_snapshot(&project_dir, &project, &args, &snap2);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].path, target_dir);
}

#[test]
fn test_plan_bound_deletion_validation() {
    let plan = DeletionPlan::new(
        vec![PathBuf::from("/Users/user")],
        false,
        true,
        vec![PlanItem {
            path: PathBuf::from("/Users/user/dev/project/target"),
            kind: PlanItemKind::Dir,
            reason: "rust target".to_string(),
            bytes: 0,
        }],
        vec![],
    );

    // Should validate the target path successfully
    assert!(validate_plan_item(
        Path::new("/Users/user/dev/project/target"),
        &plan
    ));

    // Should reject random files not present in the plan
    assert!(!validate_plan_item(
        Path::new("/Users/user/dev/project/src/main.rs"),
        &plan
    ));

    // Should refuse system folders even if manually injected into the plan
    let mut bad_plan = plan.clone();
    bad_plan.items.push(PlanItem {
        path: PathBuf::from("/System"),
        kind: PlanItemKind::Dir,
        reason: "fake target".to_string(),
        bytes: 0,
    });
    assert!(
        DeletionPlanAdjudicator::admit(Evidence::<_, Raw, PlanSafetyWitness>::raw(bad_plan))
            .is_err()
    );
}

#[test]
fn test_end_to_end_artifact_scan_build_delete() {
    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let root = tmp.path();

    // 1. Set up mock project structure
    // Mock rust project
    let rust_proj = root.join("rust-proj");
    fs::create_dir(&rust_proj).unwrap();
    File::create(rust_proj.join("Cargo.toml")).unwrap();
    let rust_target = rust_proj.join("target");
    fs::create_dir(&rust_target).unwrap();
    File::create(rust_target.join("output.bin")).unwrap();

    // Mock python project
    let py_proj = root.join("py-proj");
    fs::create_dir(&py_proj).unwrap();
    File::create(py_proj.join("requirements.txt")).unwrap();
    let py_venv = py_proj.join("venv");
    fs::create_dir(&py_venv).unwrap();
    File::create(py_venv.join("pip")).unwrap();

    // Workaround for recent project ignoring: set mtimes to past for all files
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "find '{}' '{}' -exec touch -t 202001010000 {{}} +",
            rust_proj.display(),
            py_proj.display()
        ))
        .status();

    // 2. Perform file traversal scanning
    let args = ArgsSnapshot {
        deps: true,
        aggressive: true,
        verbose: true,
        tool_roots: false,
        ignore_recent_hours: 1,
    };
    let candidates = Arc::new(Mutex::new(BTreeSet::<Candidate>::new()));
    let stats = Arc::new(Stats::default());
    let tool_accs = Arc::new(DashMap::new());

    scan_root(
        root,
        &args,
        candidates.clone(),
        stats.clone(),
        &[],
        tool_accs.clone(),
    )
    .unwrap();

    let cand_list: Vec<Candidate> = candidates.lock().unwrap().iter().cloned().collect();
    assert_eq!(cand_list.len(), 2);

    let path_list: Vec<PathBuf> = cand_list.iter().map(|c| c.path.clone()).collect();
    assert!(path_list.contains(&rust_target));
    assert!(path_list.contains(&py_venv));

    // 3. Build deletion plan
    let mut plan_items = Vec::new();
    for c in &cand_list {
        let kind = if c.path.is_file() {
            PlanItemKind::File
        } else {
            PlanItemKind::Dir
        };
        plan_items.push(PlanItem {
            path: c.path.clone(),
            kind,
            reason: c.reason.clone(),
            bytes: 0,
        });
    }

    let plan = DeletionPlan::new(vec![root.to_path_buf()], true, true, plan_items, vec![]);
    assert!(
        DeletionPlanAdjudicator::admit(Evidence::<_, Raw, PlanSafetyWitness>::raw(plan.clone()))
            .is_ok()
    );

    // 4. Execute deletion strictly matching domain deletion rules
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut results = Vec::new();
    for item in &plan.items {
        assert!(validate_plan_item(&item.path, &plan));

        if !item.path.exists() {
            results.push(osx_clnr::domain::receipt::DeletionResult {
                path: item.path.clone(),
                status: DeletionStatus::SkippedMissing,
                error: None,
                blake3_hash: None,
                bytes_freed: 0,
            });
            continue;
        }

        // Delegate all filesystem mutations to the integration layer.
        let res = match item.kind {
            PlanItemKind::File => match delete_file(&item.path) {
                Ok(()) => osx_clnr::domain::receipt::DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Deleted,
                    error: None,
                    blake3_hash: None,
                    bytes_freed: 0,
                },
                Err(e) => osx_clnr::domain::receipt::DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Failed,
                    error: Some(e.to_string()),
                    blake3_hash: None,
                    bytes_freed: 0,
                },
            },
            PlanItemKind::Dir => match delete_dir_all(&item.path) {
                Ok(()) => osx_clnr::domain::receipt::DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Deleted,
                    error: None,
                    blake3_hash: None,
                    bytes_freed: 0,
                },
                Err(e) => osx_clnr::domain::receipt::DeletionResult {
                    path: item.path.clone(),
                    status: DeletionStatus::Failed,
                    error: Some(e.to_string()),
                    blake3_hash: None,
                    bytes_freed: 0,
                },
            },
            PlanItemKind::GithubRepo
            | PlanItemKind::GithubBranch
            | PlanItemKind::GithubRun
            | PlanItemKind::GithubRelease => osx_clnr::domain::receipt::DeletionResult {
                path: item.path.clone(),
                status: DeletionStatus::Failed,
                error: Some("Not supported".to_string()),
                blake3_hash: None,
                bytes_freed: 0,
            },

        };
        results.push(res);
    }

    let end_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let receipt = DeletionReceipt::new(
        "test-chain".to_string(),
        plan.created_unix,
        start_time,
        end_time,
        results,
        None,
        None,
    );
    assert_eq!(receipt.execution_record.results.len(), 2);
    assert!(receipt
        .execution_record
        .results
        .iter()
        .all(|r| r.status == DeletionStatus::Deleted));

    // Verify files were actually deleted from filesystem
    assert!(!rust_target.exists());
    assert!(!py_venv.exists());
}

#[test]
fn test_snapshot_query_thin_parsing() {
    use osx_clnr::domain::time::{
        identify_thinned_snapshots, parse_snapshot_date, SnapshotThinReceipt,
    };

    // Test parse_snapshot_date
    assert_eq!(
        parse_snapshot_date("com.apple.TimeMachine.2026-05-26-135630.local"),
        Some("2026-05-26-135630".to_string())
    );
    assert_eq!(parse_snapshot_date("invalid-snapshot-name"), None);

    // Test identify_thinned_snapshots
    let before = vec!["snap1".to_string(), "snap2".to_string()];
    let after = vec!["snap2".to_string()];
    let thinned = identify_thinned_snapshots(&before, &after);
    assert_eq!(thinned, vec!["snap1".to_string()]);

    let thinned_none = identify_thinned_snapshots(&before, &before);
    assert!(thinned_none.is_empty());

    // Test SnapshotThinReceipt
    let receipt = SnapshotThinReceipt::new("/".to_string(), 1000000, 1716768000, before, after);
    assert_eq!(receipt.snapshots_thinned, vec!["snap1".to_string()]);
}

#[test]
fn test_size_parsing() {
    use osx_clnr::domain::time::parse_size_in_bytes;

    assert_eq!(parse_size_in_bytes("10GB").unwrap(), 10_000_000_000);
    assert_eq!(parse_size_in_bytes("500mb").unwrap(), 500_000_000);
    assert_eq!(parse_size_in_bytes("2048").unwrap(), 2048);
    assert_eq!(parse_size_in_bytes("2.5 GB").unwrap(), 2_500_000_000);
    assert_eq!(parse_size_in_bytes("100b").unwrap(), 100);

    assert!(parse_size_in_bytes("invalid_size").is_err());
    assert!(parse_size_in_bytes("").is_err());
    assert!(parse_size_in_bytes("abcGB").is_err());
}

#[test]
fn test_exclusions_plan_writing() {
    use osx_clnr::integration::tmutil::write_tm_exclusions_script;

    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let script_path = tmp.path().join("tm-exclusions.sh");

    // Create a mock directory for candidate
    let mock_dir = tmp.path().join("mock-npm-cache");
    fs::create_dir(&mock_dir).unwrap();

    let candidates = vec![
        Candidate {
            path: mock_dir.clone(),
            reason: "Mock npm cache".to_string(),
        },
        Candidate {
            path: tmp.path().join("nonexistent-dir"),
            reason: "Nonexistent".to_string(),
        },
    ];

    write_tm_exclusions_script(&script_path, &candidates).unwrap();

    assert!(script_path.exists());
    let content = fs::read_to_string(&script_path).unwrap();
    assert!(content.contains("tmutil addexclusion"));
    assert!(content.contains("mock-npm-cache"));
    // Since nonexistent-dir doesn't exist on disk, it shouldn't be added to script
    assert!(!content.contains("nonexistent-dir"));
}

#[test]
fn test_tool_root_recommendation_logic() {
    use osx_clnr::domain::tool_roots::{recommend_tool_root, ToolRootDef};

    let npm_def = ToolRootDef {
        path: PathBuf::from("/Users/user/.npm"),
        category: "node_package_cache",
        default_disposition: "cleanup_candidate",
    };

    let docker_def = ToolRootDef {
        path: PathBuf::from("/Users/user/.docker"),
        category: "container_state",
        default_disposition: "review_with_tool",
    };

    let kube_def = ToolRootDef {
        path: PathBuf::from("/Users/user/.kube"),
        category: "kubernetes_config",
        default_disposition: "keep",
    };

    // Stale and very large npm cache -> cleanup_candidate
    let (rec_npm, _) = recommend_tool_root(
        &npm_def,
        50 * 1024 * 1024 * 1024,
        Some(100),
        Some(100),
        Some(100),
    );
    assert_eq!(rec_npm, "cleanup_candidate");

    // Fresh and small npm cache -> low_priority
    let (rec_npm_fresh, _) = recommend_tool_root(&npm_def, 1024, Some(0), Some(0), Some(0));
    assert_eq!(rec_npm_fresh, "low_priority");

    // Stale docker container state -> review_with_tool
    let (rec_doc, _) = recommend_tool_root(
        &docker_def,
        15 * 1024 * 1024 * 1024,
        Some(90),
        Some(90),
        Some(90),
    );
    assert_eq!(rec_doc, "review_with_tool");

    // Kubernetes config -> always keep
    let (rec_kube, _) = recommend_tool_root(
        &kube_def,
        50 * 1024 * 1024 * 1024,
        Some(100),
        Some(100),
        Some(100),
    );
    assert_eq!(rec_kube, "keep");
}

#[test]
fn test_receipt_verification_and_plan_correlation() {
    use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus, IssueType};
    use std::fs;

    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let file_to_delete = tmp.path().join("should_be_deleted.txt");
    fs::write(&file_to_delete, "some content").unwrap();

    let plan = DeletionPlan::new(
        vec![tmp.path().to_path_buf()],
        false,
        false,
        vec![PlanItem {
            path: file_to_delete.clone(),
            kind: PlanItemKind::File,
            reason: "temp file".to_string(),
            bytes: 0,
        }],
        vec![],
    );

    // 1. Consistent receipt case: file is deleted (doesn't exist)
    fs::remove_file(&file_to_delete).unwrap();
    let consistent_receipt = DeletionReceipt::new(
        "test-chain-1".to_string(),
        plan.created_unix,
        1716768000,
        1716768100,
        vec![DeletionResult {
            path: file_to_delete.clone(),
            status: DeletionStatus::Deleted,
            error: None,
            blake3_hash: None,
            bytes_freed: 0,
        }],
        None,
        None,
    );
    let report = consistent_receipt.verify(Some(&plan));
    assert!(report.is_consistent);
    assert!(report.issues.is_empty());

    // 2. Inconsistent receipt case: file is marked as Deleted, but still exists on disk
    fs::write(&file_to_delete, "some content").unwrap();
    let report2 = consistent_receipt.verify(Some(&plan));
    assert!(!report2.is_consistent);
    assert_eq!(report2.issues.len(), 1);
    assert_eq!(report2.issues[0].issue_type, IssueType::PathStillExists);
    assert_eq!(report2.issues[0].path, file_to_delete);

    // 3. Plan mismatch case: extra receipt item not in plan
    fs::remove_file(&file_to_delete).unwrap();
    let mismatched_receipt = DeletionReceipt::new(
        "test-chain-2".to_string(),
        plan.created_unix,
        1716768000,
        1716768100,
        vec![
            DeletionResult {
                path: file_to_delete.clone(),
                status: DeletionStatus::Deleted,
                error: None,
                blake3_hash: None,
                bytes_freed: 0,
            },
            DeletionResult {
                path: tmp.path().join("extra_file.txt"),
                status: DeletionStatus::Deleted,
                error: None,
                blake3_hash: None,
                bytes_freed: 0,
            },
        ],
        None,
        None,
    );
    let report3 = mismatched_receipt.verify(Some(&plan));
    assert!(!report3.is_consistent);
    assert!(report3
        .issues
        .iter()
        .any(|i| i.issue_type == IssueType::ExtraReceiptItem));
}

#[test]
fn test_bytes_freed_mismatch_on_zero_movement() {
    use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus, IssueType};

    // Receipt claims it freed 2 GB, but the volume free-space delta is zero
    // (available_before == available_after). Under the REALITY law this is a
    // BytesFreedMismatch hard-fail.
    let receipt = DeletionReceipt::new(
        "bytes-mismatch-chain".to_string(),
        0,
        1716768000,
        1716768100,
        vec![DeletionResult {
            path: std::path::PathBuf::from("/nonexistent/zero/movement"),
            status: DeletionStatus::SkippedMissing,
            error: None,
            blake3_hash: None,
            bytes_freed: 2_000_000_000,
        }],
        Some(5_000_000_000),
        Some(5_000_000_000),
    );

    let report = receipt.verify(None);
    assert!(!report.is_consistent);
    let issue = report
        .issues
        .iter()
        .find(|i| i.issue_type == IssueType::BytesFreedMismatch)
        .expect("BytesFreedMismatch must be raised on zero free-space movement");
    assert!(issue.message.contains("2000000000"));
    assert!(issue.message.contains("measured"));
}

#[test]
fn test_no_false_positive_within_tolerance() {
    use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus, IssueType};

    // Legit path: receipt claims 4 GB freed; volume free-space delta measured
    // 3 GB. That is 75% recovery, comfortably above the 50% floor (e.g. 1 GB
    // remained pinned by an APFS snapshot / measurement noise). This MUST NOT
    // raise BytesFreedMismatch -- proving no false positive on a real delta
    // that lands within tolerance of bytes_freed_total.
    let receipt = DeletionReceipt::new(
        "within-tolerance-chain".to_string(),
        0,
        1716768000,
        1716768100,
        vec![DeletionResult {
            path: std::path::PathBuf::from("/nonexistent/within/tolerance"),
            status: DeletionStatus::SkippedMissing,
            error: None,
            blake3_hash: None,
            bytes_freed: 4_000_000_000,
        }],
        Some(10_000_000_000),
        Some(13_000_000_000), // delta = +3 GB == 75% of 4 GB claim
    );

    let report = receipt.verify(None);
    assert!(
        report.is_consistent,
        "within-tolerance delta must stay consistent, got issues: {:?}",
        report.issues
    );
    assert!(!report
        .issues
        .iter()
        .any(|i| i.issue_type == IssueType::BytesFreedMismatch));
}

#[test]
fn test_back_compat_none_samples_no_mismatch() {
    use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus, IssueType};

    // Back-compat: old receipts predate volume sampling and carry
    // available_before/after == None. Even with an enormous claimed reclaim
    // (5 GB, well over the 1 GB enforcement threshold), the absence of samples
    // means the REALITY law cannot run and MUST NOT raise BytesFreedMismatch.
    let receipt = DeletionReceipt::new(
        "old-receipt-chain".to_string(),
        0,
        1716768000,
        1716768100,
        vec![DeletionResult {
            path: std::path::PathBuf::from("/nonexistent/old/receipt"),
            status: DeletionStatus::SkippedMissing,
            error: None,
            blake3_hash: None,
            bytes_freed: 5_000_000_000,
        }],
        None, // available_before
        None, // available_after
    );

    let report = receipt.verify(None);
    assert!(
        report.is_consistent,
        "old receipt with None samples must stay consistent, got issues: {:?}",
        report.issues
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|i| i.issue_type == IssueType::BytesFreedMismatch),
        "None samples must NOT raise BytesFreedMismatch (back-compat)"
    );
}

#[test]
fn test_bytes_freed_mismatch_plan_bound_deleted_results() {
    use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    use osx_clnr::domain::receipt::{DeletionReceipt, DeletionResult, DeletionStatus, IssueType};

    // Plan-bound refusal: two Deleted results summing to 3 GB (> 2 GB), but the
    // volume free-space delta is zero (available_before == available_after).
    // Verified via verify(Some(&plan)). Under the REALITY law this MUST raise
    // BytesFreedMismatch and the report MUST be inconsistent.
    let p1 = std::path::PathBuf::from("/nonexistent/plan/bound/one");
    let p2 = std::path::PathBuf::from("/nonexistent/plan/bound/two");

    let plan = DeletionPlan::new(
        vec![std::path::PathBuf::from("/nonexistent/plan/bound")],
        false,
        false,
        vec![
            PlanItem {
                path: p1.clone(),
                kind: PlanItemKind::File,
                reason: "big artifact".to_string(),
                bytes: 2_000_000_000,
            },
            PlanItem {
                path: p2.clone(),
                kind: PlanItemKind::File,
                reason: "big artifact".to_string(),
                bytes: 1_000_000_000,
            },
        ],
        vec![],
    );

    let receipt = DeletionReceipt::new(
        "plan-bound-bytes-mismatch".to_string(),
        plan.created_unix,
        1716768000,
        1716768100,
        vec![
            DeletionResult {
                path: p1.clone(),
                status: DeletionStatus::Deleted,
                error: None,
                blake3_hash: None,
                bytes_freed: 2_000_000_000,
            },
            DeletionResult {
                path: p2.clone(),
                status: DeletionStatus::Deleted,
                error: None,
                blake3_hash: None,
                bytes_freed: 1_000_000_000,
            },
        ],
        Some(7_000_000_000),
        Some(7_000_000_000), // delta = 0, claim = 3 GB
    );

    let report = receipt.verify(Some(&plan));
    assert!(!report.is_consistent);
    let issue = report
        .issues
        .iter()
        .find(|i| i.issue_type == IssueType::BytesFreedMismatch)
        .expect("BytesFreedMismatch must be raised on plan-bound zero-movement deletion");
    assert!(issue.message.contains("3000000000"));
    assert!(issue.message.contains("measured"));
}

#[test]
fn test_ocel_validation_and_summarization() {
    use osx_clnr::domain::ocel::{
        build_tool_roots_ocel, summarize_ocel_log, OCELRelationship, OcelLogAdjudicator,
    };
    use wasm4pm_compat::admission::Admit;
    use wasm4pm_compat::evidence::Evidence;

    // 1. Valid empty log
    let log = build_tool_roots_ocel(&[]);
    let report = OcelLogAdjudicator::admit(Evidence::raw(log.clone()));
    assert!(report.is_ok());

    let summary = summarize_ocel_log(&log);
    assert_eq!(summary.total_events, 1);
    assert_eq!(summary.total_objects, 1);

    // 2. Schema violation - undefined event type
    let mut invalid_log = log.clone();
    invalid_log.events[0].event_type = "non_existent_event_type".to_string();
    let report2 = OcelLogAdjudicator::admit(Evidence::raw(invalid_log));
    assert!(report2.is_err());
    assert!(report2
        .unwrap_err()
        .reason
        .contains("is not defined in eventTypes schema"));

    // 3. Referential integrity violation - dangling object reference
    let mut invalid_log2 = log.clone();
    invalid_log2.events[0].relationships.push(OCELRelationship {
        object_id: "non_existent_object_id".to_string(),
        qualifier: "test-ref".to_string(),
    });
    let report3 = OcelLogAdjudicator::admit(Evidence::raw(invalid_log2));
    assert!(report3.is_err());
    assert!(report3
        .unwrap_err()
        .reason
        .contains("pointing to non-existent object"));
}

#[test]
fn test_snapshot_and_exclusion_ocel_generation() {
    use osx_clnr::domain::ocel::{
        build_exclusion_plan_ocel, build_snapshot_audit_ocel, build_snapshot_thin_ocel,
        OcelLogAdjudicator,
    };
    use wasm4pm_compat::admission::Admit;
    use wasm4pm_compat::evidence::Evidence;

    // Test snapshot audit OCEL
    let audit_log = build_snapshot_audit_ocel(
        "/System/Volumes/Data",
        &["com.apple.TimeMachine.2026-05-26.local".to_string()],
    );
    println!("Object Types in Test: {:?}", audit_log.object_types);
    println!(
        "AUDIT_LOG_JSON: {}",
        serde_json::to_string_pretty(&audit_log).unwrap()
    );
    let audit_report = OcelLogAdjudicator::admit(Evidence::raw(audit_log.clone()));
    if audit_report.is_err() {
        println!(
            "OCEL Validation Errors: {:?}",
            audit_report.as_ref().err().unwrap().reason
        );
    }
    assert!(audit_report.is_ok());
    assert_eq!(audit_log.objects[0].object_type, "snapshot_state");
    assert_eq!(audit_log.events[0].event_type, "snapshot_state_observed");

    // Test snapshot thin OCEL
    let thin_log = build_snapshot_thin_ocel(
        "/",
        500_000_000,
        &["snap1".to_string(), "snap2".to_string()],
        &["snap2".to_string()],
        &["snap1".to_string()],
    );
    let thin_report = OcelLogAdjudicator::admit(Evidence::raw(thin_log.clone()));
    assert!(thin_report.is_ok());
    assert_eq!(thin_log.objects[0].object_type, "snapshot_state");
    assert_eq!(thin_log.events[0].event_type, "snapshot_thin_requested");

    // Test exclusion plan OCEL
    let exclusion_log = build_exclusion_plan_ocel("/Users/user/tm-exclusions.sh", 3);
    let exclusion_report = OcelLogAdjudicator::admit(Evidence::raw(exclusion_log.clone()));
    assert!(exclusion_report.is_ok());
    assert_eq!(exclusion_log.objects[0].object_type, "tm_exclusion_plan");
    assert_eq!(
        exclusion_log.events[0].event_type,
        "tm_exclusion_plan_written"
    );
}

#[test]
fn test_exclusion_planning_and_application() {
    use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    use osx_clnr::domain::tool_roots::ToolRootReport;
    use osx_clnr::nouns::exclusion::{handle, ExclusionAction};

    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let plan_path = tmp.path().join("deletion-plan.json");
    let script_path = tmp.path().join("tm-exclusions.sh");

    // Create directories so they exist and qualify for exclusions script
    let mock_project_dir = tmp.path().join("mock-project-target");
    let mock_tool_dir = tmp.path().join("mock-tool-cache");
    fs::create_dir(&mock_project_dir).unwrap();
    fs::create_dir(&mock_tool_dir).unwrap();

    let items = vec![PlanItem {
        path: mock_project_dir.clone(),
        kind: PlanItemKind::Dir,
        reason: "Mock rust target".to_string(),
        bytes: 0,
    }];

    let tool_roots = vec![ToolRootReport {
        path: mock_tool_dir.to_string_lossy().to_string(),
        category: "node_package_cache".to_string(),
        bytes: 1024,
        human: "1KB".to_string(),
        files: 5,
        dirs: 1,
        created_unix: None,
        last_accessed_unix: None,
        last_modified_unix: None,
        metadata_changed_unix: None,
        newest_descendant_modified_unix: None,
        newest_descendant_path: None,
        days_since_modified: None,
        days_since_accessed: None,
        days_since_newest_descendant_modified: None,
        recommendation: "cleanup_candidate".to_string(),
        rationale: "mock cache".to_string(),
    }];

    let plan = DeletionPlan::new(
        vec![tmp.path().to_path_buf()],
        false,
        false,
        items,
        tool_roots,
    );
    let plan_json = serde_json::to_string(&plan).unwrap();
    fs::write(&plan_path, plan_json).unwrap();

    // Run the exclusion plan action
    let action = ExclusionAction::Plan {
        from: plan_path,
        output: script_path.clone(),
        ocel: None,
    };
    handle(action).unwrap();

    assert!(script_path.exists());
    let script_content = fs::read_to_string(&script_path).unwrap();

    // Verify both mock_project_dir and mock_tool_dir are included in the generated script
    assert!(script_content.contains(&mock_project_dir.to_string_lossy().to_string()));
    assert!(script_content.contains(&mock_tool_dir.to_string_lossy().to_string()));
}

#[test]
fn test_privacy_subcommands() {
    use osx_clnr::nouns::privacy::{handle, PrivacyAction};

    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let file_to_redact = tmp.path().join("leak.txt");
    fs::write(
        &file_to_redact,
        "Some password: \"my-secret-pw\" and secret: \"my-secret-val\" and /Users/some_other_user/path",
    )
    .unwrap();

    // Test redact subcommand
    let redact_action = PrivacyAction::Redact {
        file: file_to_redact.clone(),
    };
    handle(redact_action).unwrap();

    let redacted_content = fs::read_to_string(&file_to_redact).unwrap();
    assert!(redacted_content.contains("password: \"[REDACTED]\""));
    assert!(redacted_content.contains("secret: \"[REDACTED]\""));
    assert!(redacted_content.contains("/Users/<user>/path"));

    let _ = handle(PrivacyAction::Scan);
}

#[test]
fn test_traversal_barriers() {
    let tmp = tempfile::Builder::new().tempdir_in(".").unwrap();
    let root = tmp.path();

    // Create a mock directory with node_modules and a nested project structure inside it
    let node_modules_dir = root.join("node_modules");
    let nested_dist = node_modules_dir.join("nested-pkg/dist");
    fs::create_dir_all(&nested_dist).unwrap();
    fs::write(nested_dist.join("index.js"), "console.log('test')").unwrap();

    let args = ArgsSnapshot {
        deps: true,
        aggressive: false,
        verbose: false,
        tool_roots: false,
        ignore_recent_hours: 1,
    };

    let candidates = Arc::new(Mutex::new(BTreeSet::new()));
    let stats = Arc::new(Stats::default());
    let tool_defs = vec![];
    let tool_accs = Arc::new(DashMap::new());

    scan_root(
        root,
        &args,
        candidates,
        stats.clone(),
        &tool_defs,
        tool_accs,
    )
    .unwrap();

    // Traversal should stop at node_modules and NOT enter nested-pkg/dist
    let pruned = stats.pruned_dirs.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        pruned >= 1,
        "Expected traversal barrier to prune node_modules"
    );
}

#[test]
fn test_verify_unsupported_version() {
    use osx_clnr::domain::receipt::{DeletionReceipt, IssueType};

    // new() always stamps version 1; deserialized/tampered receipts can carry
    // another version, so verify must still flag it. Mutate the record to simulate.
    let mut receipt = DeletionReceipt::new("c".to_string(), 0, 1, 2, vec![], None, None);
    receipt.execution_record.version = 2;

    let report = receipt.verify(None);
    assert!(!report.is_consistent);
    assert!(report
        .issues
        .iter()
        .any(|i| i.issue_type == IssueType::UnsupportedVersion));
}

#[test]
fn test_verify_invalid_timestamps() {
    use osx_clnr::domain::receipt::{DeletionReceipt, IssueType};

    // completed (1) before started (2) is an impossible lifecycle.
    let receipt = DeletionReceipt::new("c".to_string(), 0, 2, 1, vec![], None, None);

    let report = receipt.verify(None);
    assert!(!report.is_consistent);
    assert!(report
        .issues
        .iter()
        .any(|i| i.issue_type == IssueType::InvalidTimestamps));
}

#[test]
fn test_verify_missing_plan_item() {
    use osx_clnr::domain::plan::{DeletionPlan, PlanItem, PlanItemKind};
    use osx_clnr::domain::receipt::{DeletionReceipt, IssueType};

    // Plan schedules a path that the receipt's results never mention -> the plan
    // item is missing from execution evidence.
    let plan = DeletionPlan::new(
        vec![PathBuf::from("/Users/user")],
        false,
        true,
        vec![PlanItem {
            path: PathBuf::from("/Users/user/dev/project/target"),
            kind: PlanItemKind::Dir,
            reason: "rust target".to_string(),
            bytes: 0,
        }],
        vec![],
    );
    let receipt = DeletionReceipt::new("c".to_string(), 0, 1, 2, vec![], None, None);

    let report = receipt.verify(Some(&plan));
    assert!(!report.is_consistent);
    assert!(report
        .issues
        .iter()
        .any(|i| i.issue_type == IssueType::MissingPlanItem));
}

#[test]
fn test_check_reclaim_shared_law() {
    use osx_clnr::domain::receipt::{check_reclaim, ReclaimCheck};

    // Witnessed: measured delta recovers the full claim.
    assert_eq!(
        check_reclaim(3_000_000_000, Some(10_000_000_000), Some(13_000_000_000)),
        ReclaimCheck::Witnessed
    );

    // Shortfall: large claim, zero movement (the snapshot-pinned / never-freed case
    // the emergency path warns on and verify() flags as BytesFreedMismatch).
    assert_eq!(
        check_reclaim(3_000_000_000, Some(7_000_000_000), Some(7_000_000_000)),
        ReclaimCheck::Shortfall {
            claimed: 3_000_000_000,
            measured: 0
        }
    );

    // NotApplicable: below the 1 GB witness floor, and missing samples (back-compat).
    assert_eq!(
        check_reclaim(500_000_000, Some(0), Some(0)),
        ReclaimCheck::NotApplicable
    );
    assert_eq!(
        check_reclaim(5_000_000_000, None, None),
        ReclaimCheck::NotApplicable
    );
}

#[test]
fn test_size_string_roundtrip_convention_resolved() {
    // RESOLVED SEAM (was: type-identical inverses with divergent bases). The
    // canonical size base for this crate is now SI/1000 everywhere:
    // `parse_size_in_bytes` (parser) and both `human_bytes` definitions (display)
    // are inverses on the SAME base, so the round-trip is lossless at unit
    // boundaries. This test now witnesses CONVERGENCE; if either side ever drifts
    // back to binary/1024, the round-trip breaks here and forces a conscious fix.
    use osx_clnr::domain::time::parse_size_in_bytes;
    use osx_clnr::domain::tool_roots::human_bytes as hb_domain;
    use osx_clnr::integration::progress::human_bytes as hb_integration;

    assert_eq!(parse_size_in_bytes("1GB"), Ok(1_000_000_000));

    // Clean round-trip: "1GB" -> bytes -> "1.00 GB".
    let bytes = parse_size_in_bytes("1GB").unwrap();
    assert_eq!(hb_integration(bytes), "1.00 GB");

    // The two byte-identical definitions agree (same canonical base).
    assert_eq!(hb_integration(bytes), hb_domain(bytes));
    assert_eq!(hb_domain(2_500_000), "2.50 MB");
}

#[test]
fn test_snapshot_delete_ocel_is_truthfully_typed() {
    // WITNESS for the selective-delete OCEL edge. The delete path must emit a
    // `snapshot_delete_requested` event — NOT `snapshot_thin_requested` — so the
    // event log does not conflate a selection-driven delete with a byte-driven
    // thin (model-vs-log truth). The log must also be admissible.
    use osx_clnr::domain::ocel::{
        build_snapshot_delete_ocel, build_snapshot_thin_ocel, OcelLogAdjudicator,
    };
    use wasm4pm_compat::admission::Admit;
    use wasm4pm_compat::evidence::Evidence;

    let before = vec!["snap-a".to_string(), "snap-b".to_string()];
    let after = vec!["snap-b".to_string()];
    let deleted = vec!["snap-a".to_string()];

    let delete_log = build_snapshot_delete_ocel("/", &before, &after, &deleted);

    // Admissible (passes the OCEL law).
    assert!(OcelLogAdjudicator::admit(Evidence::raw(delete_log.clone())).is_ok());

    // Truthfully typed — a delete, not a thin.
    assert_eq!(delete_log.events[0].event_type, "snapshot_delete_requested");
    assert_eq!(delete_log.objects[0].object_type, "snapshot_state");
    assert!(delete_log.events[0]
        .attributes
        .iter()
        .any(|a| a.name == "deleted_count"));

    // The two operations are now distinguishable in the log.
    let thin_log = build_snapshot_thin_ocel("/", 500_000_000, &before, &after, &deleted);
    assert_ne!(
        delete_log.events[0].event_type,
        thin_log.events[0].event_type
    );
}
